use std::{ffi::OsString, io, path::Path, process::ExitCode, sync::mpsc, thread, time::Duration};

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{Terminal, backend::Backend};

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
    let mut dirty = true;
    let result = super::run_terminal(|terminal| -> io::Result<()> {
        loop {
            if let Ok(completion) = receiver.try_recv() {
                worker
                    .join()
                    .map_err(|_| io::Error::other("environment worker panicked"))?;
                accept_completion(&mut state, completion);
                dirty = true;
            } else if let Some(result) = worker.poll_finished() {
                result.map_err(|_| io::Error::other("environment worker panicked"))?;
                let completion = receiver
                    .try_recv()
                    .map_err(|_| io::Error::other("environment worker returned no result"))?;
                accept_completion(&mut state, completion);
                dirty = true;
            }
            if cancellation.is_cancelled() {
                state.interrupt();
                break;
            }
            if let Some(index) = state.start_next() {
                dirty = true;
                if let Err(error) =
                    start_worker(invocation, &state, index, &mut plans, &mut worker, &sender)
                {
                    state.complete(index, PlanResult::Error(error.to_string()), Vec::new());
                }
            }
            dirty |= state.clear_expired_copy_feedback(std::time::Instant::now());
            draw_if_needed(&state, &mut view, terminal, &mut dirty)?;
            if !event::poll(Duration::from_millis(50))? {
                continue;
            }
            let input_event = event::read()?;
            if !event_requires_draw(&input_event) {
                continue;
            }
            dirty = true;
            if let Event::Key(key) = input_event {
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

fn should_draw(state: &EnvironmentSession, dirty: bool) -> bool {
    dirty || state.acquiring()
}

fn draw_if_needed<B: Backend>(
    state: &EnvironmentSession,
    view: &mut EnvironmentView,
    terminal: &mut Terminal<B>,
    dirty: &mut bool,
) -> Result<bool, B::Error> {
    if !should_draw(state, *dirty) {
        return Ok(false);
    }
    terminal.draw(|frame| view.render(frame, state))?;
    *dirty = false;
    Ok(true)
}

fn event_requires_draw(event: &Event) -> bool {
    match event {
        Event::Resize(_, _) => true,
        Event::Key(key) => key.kind != KeyEventKind::Release,
        _ => false,
    }
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

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Instant};

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::app::{
        copy::{CopyResult, CopyTarget},
        environments::{EnvironmentAvailability, EnvironmentIdentity},
        review::{PlanMetadata, PlanReview, test_support::plan_document},
    };

    fn available(name: &str) -> Environment {
        Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from(name),
                workspace: "default".to_owned(),
            }),
        }
    }

    fn ready() -> PlanResult {
        PlanResult::Ready {
            review: Box::new(PlanReview::new(
                PathBuf::from("/test"),
                "default".to_owned(),
                plan_document("No changes.\n".to_owned()),
                PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
                Vec::new(),
            )),
            changed: false,
        }
    }

    fn ready_session() -> EnvironmentSession {
        let mut state = EnvironmentSession::new(vec![available("a")], false);
        let index = state.start_next().expect("environment should start");
        assert!(state.complete(index, ready(), Vec::new()));
        state
    }

    fn record_copy(state: &mut EnvironmentSession, now: Instant) {
        let Some(Effect::WriteClipboard(effect)) =
            state.update_review(0, Action::Copy(CopyTarget::Plan), now)
        else {
            panic!("plan copy should produce a clipboard effect");
        };
        assert!(
            state
                .update_review(
                    0,
                    Action::CopyCompleted {
                        target: effect.target(),
                        result: CopyResult::Written,
                    },
                    now,
                )
                .is_none()
        );
    }

    #[test]
    fn completed_environments_skip_idle_draws_and_redraw_for_events_and_copy_expiration() {
        let mut state = ready_session();
        let mut view = EnvironmentView::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut dirty = true;
        let mut draws = 0;

        assert!(
            draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("initial draw should succeed")
        );
        draws += 1;
        assert!(
            !draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("empty poll should succeed")
        );
        assert_eq!(draws, 1);

        for event in [
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Event::Resize(120, 40),
        ] {
            assert!(event_requires_draw(&event));
            dirty = true;
            assert!(
                draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                    .expect("input and resize should draw")
            );
            draws += 1;
        }
        assert!(!event_requires_draw(&Event::Key(KeyEvent::new_with_kind(
            KeyCode::Down,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ))));
        assert!(!event_requires_draw(&Event::FocusGained));

        let copied_at = Instant::now();
        let copy_event = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(event_requires_draw(&copy_event));
        dirty = true;
        record_copy(&mut state, copied_at);
        assert!(should_draw(&state, dirty));
        assert!(
            draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("copy notice should draw")
        );
        draws += 1;
        assert!(
            !draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("pending copy notice should not draw on an empty poll")
        );

        dirty |= state.clear_expired_copy_feedback(copied_at + Duration::from_secs(3));
        assert!(dirty);
        assert!(
            draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("expired copy notice should draw once")
        );
        draws += 1;
        assert!(
            !draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("idle poll after expiration should succeed")
        );
        assert_eq!(draws, 5);
    }

    #[test]
    fn acquisition_draws_during_poll_and_retry_and_completion_request_a_draw() {
        let mut state = EnvironmentSession::new(vec![available("a")], false);
        let mut view = EnvironmentView::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut dirty = true;
        let mut draws = 0;

        assert!(
            draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("initial draw should succeed")
        );
        draws += 1;
        let index = state.start_next().expect("environment should start");
        dirty = true;
        assert!(
            draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("worker start should draw")
        );
        draws += 1;
        assert!(should_draw(&state, false));
        assert!(
            draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("acquisition poll should draw")
        );
        draws += 1;

        assert!(state.complete(index, PlanResult::Error("failed".to_owned()), Vec::new()));
        dirty = true;
        assert!(
            draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("worker result should draw")
        );
        draws += 1;
        assert!(
            !draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("completed poll should be idle")
        );

        assert!(state.retry(index));
        dirty = true;
        assert!(
            draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("retry should draw")
        );
        draws += 1;
        assert!(should_draw(&state, false));

        let retry_index = state.start_next().expect("retry should start");
        assert_eq!(retry_index, index);
        dirty = true;
        assert!(state.complete(retry_index, ready(), Vec::new()));
        assert!(
            draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("retry result should draw")
        );
        draws += 1;
        assert!(
            !draw_if_needed(&state, &mut view, &mut terminal, &mut dirty)
                .expect("idle poll after retry should not draw")
        );
        assert_eq!(draws, 6);
    }
}
