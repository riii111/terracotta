use std::{ffi::OsString, io, path::Path, process::ExitCode, sync::mpsc, thread, time::Duration};

use crossterm::event::{self, Event, KeyEventKind};

use crate::{
    app::{
        environments::{Environment, EnvironmentSession, PlanResult},
        execution::{Diagnostic, ExecutionContext, Tool},
        session::{Action, Effect},
    },
    infra::{
        CancellationToken, ClipboardExecutor,
        terraform::{
            self,
            configuration::{self, ExecutionLocation},
        },
    },
    ui::features::environments::{EnvironmentInput, EnvironmentView},
};

use super::{WorkerGuard, invocation::Invocation};

struct Completion {
    index: usize,
    result: PlanResult,
    diagnostics: Vec<Diagnostic>,
}

pub(super) fn run(invocation: &Invocation, environments: Vec<Environment>) -> io::Result<ExitCode> {
    let mut state = EnvironmentSession::new(environments, invocation.detailed_exitcode());
    let cancellation = CancellationToken::new();
    let (sender, receiver) = mpsc::channel();
    let mut plans: Vec<Option<terraform::SavedPlan>> =
        (0..state.plans().len()).map(|_| None).collect();
    let mut worker = WorkerGuard {
        cancellation: cancellation.clone(),
        handle: None,
    };
    let mut view = EnvironmentView::default();
    let mut clipboard = ClipboardExecutor::new();
    let result = super::run_terminal(|terminal| -> io::Result<()> {
        loop {
            if let Ok(completion) = receiver.try_recv() {
                worker
                    .join()
                    .map_err(|_| io::Error::other("environment worker panicked"))?;
                accept_completion(&mut state, completion);
            } else if let Some(result) = worker.poll_finished() {
                result.map_err(|_| io::Error::other("environment worker panicked"))?;
                let completion = receiver
                    .try_recv()
                    .map_err(|_| io::Error::other("environment worker returned no result"))?;
                accept_completion(&mut state, completion);
            }
            if cancellation.is_cancelled() {
                state.interrupt();
                break;
            }
            if let Some(index) = state.start_next()
                && let Err(error) =
                    start_worker(invocation, &state, index, &mut plans, &mut worker, &sender)
            {
                state.complete(index, PlanResult::Error(error.to_string()), Vec::new());
            }
            terminal.draw(|frame| view.render(frame, &state))?;
            if !event::poll(Duration::from_millis(50))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            let size = terminal.size()?;
            let Some(input) = view.handle_key(key, size, &state) else {
                continue;
            };
            match input {
                EnvironmentInput::Quit => break,
                EnvironmentInput::Interrupt => {
                    state.interrupt();
                    break;
                }
                EnvironmentInput::Retry(index) => {
                    state.retry(index);
                }
                EnvironmentInput::Review(index, action) => {
                    if let Some(Effect::WriteClipboard(effect)) =
                        state.update_review(index, *action, std::time::Instant::now())
                    {
                        let result = clipboard.execute(&effect);
                        state.update_review(
                            index,
                            Action::CopyCompleted {
                                target: effect.target(),
                                result,
                            },
                            std::time::Instant::now(),
                        );
                    }
                }
            }
        }
        Ok(())
    });
    cancellation.cancel();
    let joined = worker.join();
    let cleanup = cleanup_plans(plans);
    result?;
    joined.map_err(|_| io::Error::other("environment worker panicked"))?;
    cleanup?;
    Ok(ExitCode::from(state.exit_code()))
}

fn accept_completion(state: &mut EnvironmentSession, completion: Completion) {
    state.complete(completion.index, completion.result, completion.diagnostics);
}

fn start_worker(
    invocation: &Invocation,
    state: &EnvironmentSession,
    index: usize,
    plans: &mut [Option<terraform::SavedPlan>],
    worker: &mut WorkerGuard,
    sender: &mpsc::Sender<Completion>,
) -> io::Result<()> {
    if let Some(old_plan) = plans[index].take() {
        old_plan.cleanup()?;
    }
    let environment = &state.plans()[index];
    let root = environment.directory().to_owned();
    let tool = environment.tool;
    let (saved_plan, arguments) =
        terraform::saved_plan_for_plan(&root, &invocation.plan_arguments())?;
    let plan_path = saved_plan.path().to_owned();
    plans[index] = Some(saved_plan);
    let cancellation = worker.cancellation.clone();
    let sender = sender.clone();
    let launch_root = invocation.directory().to_owned();
    worker.set_handle(
        thread::Builder::new()
            .name("terracotta-environment".to_owned())
            .spawn(move || {
                let mut diagnostics = Vec::new();
                let result = acquire(
                    tool,
                    &root,
                    &launch_root,
                    &arguments,
                    &plan_path,
                    &cancellation,
                    &mut diagnostics,
                )
                .unwrap_or_else(PlanResult::Error);
                let _ = sender.send(Completion {
                    index,
                    result,
                    diagnostics,
                });
            })?,
    );
    Ok(())
}

fn acquire(
    tool: Tool,
    root: &Path,
    launch_root: &Path,
    arguments: &[OsString],
    plan_path: &Path,
    cancellation: &CancellationToken,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<PlanResult, String> {
    let config =
        configuration::read_configuration(root, tool, None).map_err(|error| error.to_string())?;
    if config.execution_location == ExecutionLocation::HcpCandidate {
        return Ok(PlanResult::ExcludedHcp);
    }
    if !config.has_backend {
        return Err("The environment no longer has backend configuration.".to_owned());
    }
    let changed = terraform::run_environment_plan(
        tool,
        root,
        arguments,
        cancellation,
        &terraform::SystemProcessRunner,
        diagnostics,
    )
    .map_err(|error| environment_failure(&error, cancellation))?;
    let variables =
        super::invocation::variable_sources(root, arguments).map_err(|error| error.to_string())?;
    let context = ExecutionContext::loading(root.display().to_string())
        .with_tool(tool)
        .with_launch_root(launch_root)
        .with_variable_sources(variables);
    let review = terraform::read_saved_plan_review(
        tool,
        root,
        root,
        &[],
        plan_path,
        changed,
        false,
        context,
        cancellation,
        &terraform::SystemProcessRunner,
        &mut |_| {},
        &mut |_| {},
    )
    .map_err(|error| environment_failure(&error, cancellation))?;
    Ok(PlanResult::Ready {
        review: Box::new(review),
        changed,
    })
}

fn environment_failure(
    error: &terraform::TerraformExecutionError,
    cancellation: &CancellationToken,
) -> String {
    if error.is_interrupted() {
        cancellation.cancel();
    }
    error.to_string()
}

fn cleanup_plans(plans: Vec<Option<terraform::SavedPlan>>) -> io::Result<()> {
    let mut first_error = None;
    for plan in plans.into_iter().flatten() {
        if let Err(error) = plan.cleanup() {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}
