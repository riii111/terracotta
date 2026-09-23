use std::{
    ffi::OsString,
    fmt, fs,
    io::{self, IsTerminal, Write},
    panic::{self, AssertUnwindSafe},
    path::Path,
    process::ExitCode,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Instant,
};

mod environments;
mod event_loop;
pub(crate) mod invocation;
mod synthetic;

use crate::{
    app::{
        execution::{
            ApplyStatus, ExecutionContext, ExecutionEvent, ExecutionEventKind, ExecutionPhase,
            ExecutionStage, ExecutionState, HistoryKey, Tool, VariableSources,
        },
        review::{PlanMetadata, PlanReviewMessage},
        session::SessionOutcome,
    },
    infra::{CancellationToken, ClipboardExecutor, history::HistoryStore, terraform},
};

#[cfg(feature = "test-support")]
use crate::test_support;

const EXECUTION_FAILURE: u8 = 1;
const INTERRUPTED: u8 = 130;

pub(crate) fn run_plan(root: &Path, compare_ref: Option<&str>) -> ExitCode {
    if compare_ref.is_some() {
        report_error("--compare-ref is unavailable while Git comparison is paused");
        return ExitCode::from(EXECUTION_FAILURE);
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        report_error("terracotta plan requires an interactive terminal");
        return ExitCode::from(EXECUTION_FAILURE);
    }
    let Ok(executable) = terraform::resolve_executable(Tool::Terraform) else {
        report_error("terraform was not found in PATH");
        return ExitCode::from(EXECUTION_FAILURE);
    };
    run_managed_invocation(
        &executable,
        Tool::Terraform,
        root,
        root,
        &[],
        &[OsString::from("-detailed-exitcode")],
        &[],
        false,
        false,
        false,
        invocation::variable_sources(root, &[]).unwrap_or_default(),
    )
}

pub(crate) fn run_invocation(
    executable: &Path,
    invocation: &invocation::Invocation,
    variable_sources: VariableSources,
) -> ExitCode {
    run_managed_invocation(
        executable,
        invocation.tool(),
        invocation.launch_root(),
        invocation.directory(),
        invocation.global_arguments(),
        &invocation.plan_arguments(),
        &invocation.apply_arguments(),
        invocation.is_apply(),
        invocation.detailed_exitcode(),
        invocation.initial_overview(),
        variable_sources,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the managed invocation keeps launch, display, and Terraform argument boundaries"
)]
fn run_managed_invocation(
    executable: &Path,
    tool: Tool,
    launch_root: &Path,
    display_root: &Path,
    global_arguments: &[OsString],
    plan_arguments: &[OsString],
    apply_arguments: &[OsString],
    apply_entry: bool,
    detailed_exitcode: bool,
    initial_overview: bool,
    variable_sources: VariableSources,
) -> ExitCode {
    let (saved_plan, plan_arguments) =
        match terraform::saved_plan_for_plan(display_root, plan_arguments) {
            Ok(result) => result,
            Err(error) => {
                report_error(&format!(
                    "failed to prepare the {} plan: {error}",
                    tool.display_name()
                ));
                return ExitCode::from(EXECUTION_FAILURE);
            }
        };
    let (plan_run, status) = match terraform::run_passthrough_plan(
        executable,
        launch_root,
        global_arguments,
        &plan_arguments,
        saved_plan,
    ) {
        Ok(result) => result,
        Err(error) => {
            report_error(&format!(
                "failed to run {} plan: {error}",
                tool.display_name()
            ));
            return ExitCode::from(EXECUTION_FAILURE);
        }
    };
    if !status.is_plan_success() {
        let exit = if status == terraform::ProcessStatus::Signaled
            || status.code() == Some(i32::from(INTERRUPTED))
        {
            INTERRUPTED
        } else {
            EXECUTION_FAILURE
        };
        let _ = plan_run.saved_plan.cleanup();
        return ExitCode::from(exit);
    }
    run_saved_plan_review(
        tool,
        launch_root,
        display_root,
        global_arguments,
        apply_arguments,
        apply_entry,
        plan_run,
        detailed_exitcode,
        initial_overview,
        variable_sources,
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "the review lifecycle owns worker joins, outcome mapping, and cleanup"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "the runtime passes each execution boundary to the review worker"
)]
fn run_saved_plan_review(
    tool: Tool,
    launch_root: &Path,
    display_root: &Path,
    global_arguments: &[OsString],
    apply_arguments: &[OsString],
    apply_entry: bool,
    plan_run: terraform::PlanRun,
    detailed_exitcode: bool,
    initial_overview: bool,
    variable_sources: VariableSources,
) -> ExitCode {
    let changed = plan_run.changed;
    let review_root = match fs::canonicalize(display_root) {
        Ok(root) => root,
        Err(error) => {
            report_error(&format!(
                "failed to resolve the {} execution directory before review: {error}",
                tool.display_name()
            ));
            let _ = plan_run.saved_plan.cleanup();
            return ExitCode::from(EXECUTION_FAILURE);
        }
    };
    let cancellation = CancellationToken::new();
    let (sender, receiver) = mpsc::channel();
    let history = HistoryStore::platform();
    let saved_plan_slot = Arc::new(Mutex::new(Some(plan_run.saved_plan)));
    let Some(plan_path) = saved_plan_slot
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|plan| plan.path().to_owned()))
    else {
        report_error(&format!(
            "the reviewed {} plan is unavailable",
            tool.display_name()
        ));
        return ExitCode::from(EXECUTION_FAILURE);
    };
    let worker = match spawn_review_worker(
        tool,
        display_root,
        launch_root,
        global_arguments,
        &plan_path,
        changed,
        apply_entry,
        ExecutionContext::loading(review_root.display().to_string())
            .with_tool(tool)
            .with_launch_root(launch_root)
            .with_variable_sources(variable_sources.clone()),
        &cancellation,
        history.as_ref(),
        sender.clone(),
    ) {
        Ok(worker) => worker,
        Err(error) => {
            report_error(&format!("failed to start the plan worker: {error}"));
            let _ = take_saved_plan(&saved_plan_slot).map_or(Ok(()), terraform::SavedPlan::cleanup);
            return ExitCode::from(EXECUTION_FAILURE);
        }
    };
    let mut worker = WorkerGuard {
        cancellation: cancellation.clone(),
        handle: Some(worker),
    };
    let mut apply_worker = WorkerGuard {
        cancellation: cancellation.clone(),
        handle: None,
    };
    let mut clipboard = ClipboardExecutor::new();
    let context = ExecutionContext::loading(review_root.display().to_string())
        .with_tool(tool)
        .with_launch_root(launch_root)
        .with_variable_sources(variable_sources);
    let effects = event_loop::RuntimeEffects {
        tool,
        root: launch_root,
        display_root,
        global_arguments,
        apply_arguments,
        sender: &sender,
        saved_plan_slot: &saved_plan_slot,
        cancellation: &cancellation,
        clipboard: &mut clipboard,
        apply_worker: &mut apply_worker,
        history: history.as_ref(),
    };
    let ui_result = run_interactive(context, &receiver, &mut worker, effects, initial_overview);
    if ui_result.is_err() {
        cancellation.cancel();
    }
    let apply_join = apply_worker.join();
    let plan_join = worker.join();
    let ui_result = finalize_ui_result(ui_result, &apply_join, &plan_join);
    let cleanup_result =
        take_saved_plan(&saved_plan_slot).map_or(Ok(()), terraform::SavedPlan::cleanup);
    let primary_exit = match ui_result {
        Ok(SessionOutcome::Reviewed(metadata)) => {
            report_reviewed(&metadata);
            if !apply_entry && detailed_exitcode && changed {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            }
        }
        Ok(SessionOutcome::NoChanges) => {
            report_no_changes();
            ExitCode::SUCCESS
        }
        Ok(SessionOutcome::ApplyCanceled) => {
            report_apply_canceled();
            ExitCode::from(EXECUTION_FAILURE)
        }
        Ok(SessionOutcome::Applied {
            status: ApplyStatus::Succeeded,
            summary_line,
        }) => {
            report_apply_success(summary_line.as_deref());
            ExitCode::SUCCESS
        }
        Ok(SessionOutcome::Applied {
            status: ApplyStatus::Failed,
            ..
        }) => {
            report_apply_failure(false);
            ExitCode::from(EXECUTION_FAILURE)
        }
        Ok(SessionOutcome::Applied {
            status: ApplyStatus::Interrupted,
            ..
        }) => {
            report_apply_failure(true);
            ExitCode::from(INTERRUPTED)
        }
        Ok(SessionOutcome::Interrupted(phase)) => {
            report_interrupted(phase);
            ExitCode::from(INTERRUPTED)
        }
        Ok(SessionOutcome::Failed(phase)) => {
            report_error(&format!("{} failed.", phase.title()));
            ExitCode::from(EXECUTION_FAILURE)
        }
        Err(error) => {
            report_error(&format!("TUI failed: {error}"));
            ExitCode::from(EXECUTION_FAILURE)
        }
    };
    if let Err(error) = cleanup_result {
        report_error(&format!(
            "failed to remove the temporary {} plan: {error}",
            tool.display_name()
        ));
        ExitCode::from(EXECUTION_FAILURE)
    } else {
        primary_exit
    }
}

pub(crate) fn run_synthetic() -> io::Result<()> {
    synthetic::run_synthetic()
}

pub(crate) fn run_synthetic_execution() -> io::Result<()> {
    synthetic::run_synthetic_execution()
}

fn run_terminal<F, R>(callback: F) -> R
where
    F: FnOnce(&mut ratatui::DefaultTerminal) -> R,
{
    match panic::catch_unwind(AssertUnwindSafe(|| ratatui::run(callback))) {
        Ok(result) => result,
        Err(payload) => {
            let _ = crossterm::execute!(io::stdout(), crossterm::cursor::Show);
            panic::resume_unwind(payload);
        }
    }
}

fn run_interactive(
    context: ExecutionContext,
    receiver: &mpsc::Receiver<PlanReviewMessage>,
    plan_worker: &mut WorkerGuard,
    effects: event_loop::RuntimeEffects<'_, ClipboardExecutor>,
    initial_overview: bool,
) -> io::Result<SessionOutcome> {
    run_terminal(|terminal| {
        #[cfg(feature = "test-support")]
        if test_support::panic_after_draw_requested() {
            terminal.draw(|_| {})?;
            panic!("synthetic terminal panic");
        }

        event_loop::run_connected(
            terminal,
            ExecutionState::with_context(Instant::now(), context),
            receiver,
            plan_worker,
            effects,
            initial_overview,
        )
    })
}

fn report_reviewed(metadata: &PlanMetadata) {
    if metadata.has_changes() {
        let _ = writeln!(
            io::stdout(),
            "Plan: {} to add, {} to change, {} to replace, {} to destroy.\nApply was not run.",
            metadata.additions(),
            metadata.changes(),
            metadata.replacements(),
            metadata.deletions()
        );
    } else {
        let _ = writeln!(io::stdout(), "No changes.");
    }
}

fn report_no_changes() {
    let _ = writeln!(io::stdout(), "No changes.");
}

fn report_apply_canceled() {
    let _ = writeln!(io::stdout(), "Apply canceled.");
}

fn report_apply_success(summary_line: Option<&str>) {
    let _ = writeln!(
        io::stdout(),
        "{}",
        summary_line.unwrap_or("Apply complete.")
    );
}

fn report_apply_failure(interrupted: bool) {
    let result = if interrupted {
        "Apply interrupted. Changes may already be applied."
    } else {
        "Apply failed. Changes may already be applied."
    };
    let _ = writeln!(io::stdout(), "{result}");
}

fn report_interrupted(phase: ExecutionStage) {
    let message = match phase {
        ExecutionStage::Initializing => "Initialization cancelled.",
        _ => "Plan cancelled.",
    };
    let _ = writeln!(io::stdout(), "{message}");
}

fn finalize_ui_result(
    ui_result: io::Result<SessionOutcome>,
    apply_join: &thread::Result<()>,
    plan_join: &thread::Result<()>,
) -> io::Result<SessionOutcome> {
    let ui_worker_panic = ui_result.as_ref().err().and_then(worker_panic_kind);
    if ui_result.is_err() && ui_worker_panic.is_none() {
        return ui_result;
    }
    if apply_join.is_err() {
        return Err(worker_panic_error(WorkerKind::Apply));
    }
    if ui_worker_panic.is_some() {
        return ui_result;
    }
    if plan_join.is_err() {
        return Err(worker_panic_error(WorkerKind::Plan));
    }
    ui_result
}

fn worker_panic_kind(error: &io::Error) -> Option<WorkerKind> {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<WorkerPanic>())
        .map(|panic| panic.0)
}

fn worker_panic_error(worker: WorkerKind) -> io::Error {
    io::Error::other(WorkerPanic(worker))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerKind {
    Plan,
    Apply,
}

#[derive(Debug)]
struct WorkerPanic(WorkerKind);

impl fmt::Display for WorkerPanic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let worker = match self.0 {
            WorkerKind::Plan => "plan",
            WorkerKind::Apply => "apply",
        };
        write!(formatter, "{worker} worker panicked")
    }
}

impl std::error::Error for WorkerPanic {}

#[expect(
    clippy::too_many_arguments,
    reason = "the worker receives the explicit plan execution boundaries"
)]
fn spawn_review_worker(
    tool: Tool,
    display_root: &Path,
    launch_root: &Path,
    global_arguments: &[OsString],
    plan_path: &Path,
    plan_changed: bool,
    apply_entry: bool,
    initial_context: ExecutionContext,
    cancellation: &CancellationToken,
    history: Option<&HistoryStore>,
    sender: mpsc::Sender<PlanReviewMessage>,
) -> io::Result<JoinHandle<()>> {
    let worker_cancellation = cancellation.clone();
    let worker_display_root = display_root.to_owned();
    let worker_launch_root = launch_root.to_owned();
    let worker_global_arguments = global_arguments.to_vec();
    let worker_plan_path = plan_path.to_owned();
    let worker_initial_context = initial_context;
    let worker_history = history.cloned();
    thread::Builder::new()
        .name("terracotta-plan".to_owned())
        .spawn(move || {
            let mut event_sink = |event| {
                let _ = sender.send(PlanReviewMessage::Event(event));
            };
            let mut phase_sink = |phase: ExecutionPhase| {
                let _ = sender.send(PlanReviewMessage::Event(ExecutionEvent {
                    received_at: Instant::now(),
                    kind: ExecutionEventKind::Phase(phase),
                }));
            };
            match terraform::read_saved_plan_review(
                tool,
                &worker_display_root,
                &worker_launch_root,
                &worker_global_arguments,
                &worker_plan_path,
                plan_changed,
                apply_entry,
                worker_initial_context,
                &worker_cancellation,
                &terraform::SystemProcessRunner,
                &mut event_sink,
                &mut phase_sink,
            ) {
                Ok(review) => {
                    let review = if let Some(history) = worker_history.as_ref() {
                        let keys: Vec<_> = review
                            .metadata()
                            .apply_targets()
                            .iter()
                            .map(|target| HistoryKey::for_target(review.context(), target))
                            .collect();
                        let previous_durations = history.load_many(&keys);
                        review.with_previous_durations(previous_durations)
                    } else {
                        review
                    };
                    if !worker_cancellation.is_cancelled() {
                        let _ = sender.send(PlanReviewMessage::Completed(review));
                    }
                }
                Err(error) => {
                    let _ = sender.send(PlanReviewMessage::Failed {
                        message: error.to_string(),
                        interrupted: worker_cancellation.is_cancelled(),
                    });
                }
            }
        })
}

fn take_saved_plan(
    slot: &Arc<Mutex<Option<terraform::SavedPlan>>>,
) -> Option<terraform::SavedPlan> {
    slot.lock().ok().and_then(|mut slot| slot.take())
}

pub(super) fn spawn_apply_worker(
    tool: Tool,
    root: &Path,
    global_arguments: &[OsString],
    apply_arguments: &[OsString],
    plan_path: &Path,
    cancellation: &CancellationToken,
    sender: &mpsc::Sender<PlanReviewMessage>,
) -> io::Result<JoinHandle<()>> {
    let worker_root = root.to_owned();
    let worker_global_arguments = global_arguments.to_vec();
    let worker_apply_arguments = apply_arguments.to_vec();
    let worker_plan_path = plan_path.to_owned();
    let worker_cancellation = cancellation.clone();
    let worker_sender = sender.clone();
    thread::Builder::new()
        .name("terracotta-apply".to_owned())
        .spawn(move || {
            let mut event_sink = |event| {
                let _ = worker_sender.send(PlanReviewMessage::ApplyEvent(event));
            };
            match terraform::run_apply_with_arguments(
                tool,
                &worker_root,
                &worker_global_arguments,
                &worker_apply_arguments,
                &worker_plan_path,
                &worker_cancellation,
                &terraform::SystemProcessRunner,
                &mut event_sink,
            ) {
                Ok(result) => {
                    let _ = worker_sender.send(PlanReviewMessage::ApplyCompleted {
                        status: result.status(),
                        summary_line: result.summary_line().map(str::to_owned),
                    });
                }
                Err(error) => {
                    let _ = worker_sender.send(PlanReviewMessage::ApplyFailed {
                        message: error.to_string(),
                    });
                }
            }
        })
}

fn report_error(message: &str) {
    let _ = writeln!(io::stderr(), "{message}");
}

pub(super) struct WorkerGuard {
    cancellation: CancellationToken,
    handle: Option<JoinHandle<()>>,
}

impl WorkerGuard {
    fn set_handle(&mut self, handle: JoinHandle<()>) {
        debug_assert!(self.handle.is_none());
        self.handle = Some(handle);
    }

    fn poll_finished(&mut self) -> Option<thread::Result<()>> {
        self.handle
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            .then(|| self.join())
    }

    fn join(&mut self) -> thread::Result<()> {
        self.handle.take().map_or(Ok(()), JoinHandle::join)
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panic_join() -> thread::Result<()> {
        Err(Box::new("worker panic"))
    }

    #[test]
    fn ui_error_takes_precedence_over_worker_panics() {
        let error = finalize_ui_result(
            Err(io::Error::other("terminal failed")),
            &panic_join(),
            &panic_join(),
        )
        .expect_err("the UI error should be returned");

        assert_eq!(error.to_string(), "terminal failed");
    }

    #[test]
    fn ui_error_with_worker_panic_text_is_not_reclassified() {
        let ui_error = io::Error::other("apply worker panicked");
        let error = finalize_ui_result(Err(ui_error), &panic_join(), &panic_join())
            .expect_err("the original UI error should be returned");

        assert_eq!(error.to_string(), "apply worker panicked");
        assert!(worker_panic_kind(&error).is_none());
    }

    #[test]
    fn apply_panic_takes_precedence_over_plan_panic_after_successful_ui() {
        let error = finalize_ui_result(
            Ok(SessionOutcome::Interrupted(ExecutionStage::Initializing)),
            &panic_join(),
            &panic_join(),
        )
        .expect_err("a worker panic should fail a successful UI result");

        assert_eq!(error.to_string(), "apply worker panicked");
    }

    #[test]
    fn apply_join_panic_takes_precedence_over_an_earlier_plan_panic() {
        let apply_join = panic_join();
        let plan_join = Ok(());
        let error = finalize_ui_result(
            Err(worker_panic_error(WorkerKind::Plan)),
            &apply_join,
            &plan_join,
        )
        .expect_err("a worker panic should fail the runtime");

        assert_eq!(error.to_string(), "apply worker panicked");
    }

    #[test]
    fn apply_ui_panic_takes_precedence_over_a_later_plan_join_panic() {
        let apply_join = Ok(());
        let plan_join = panic_join();
        let error = finalize_ui_result(
            Err(worker_panic_error(WorkerKind::Apply)),
            &apply_join,
            &plan_join,
        )
        .expect_err("the earlier apply panic should remain primary");

        assert_eq!(error.to_string(), "apply worker panicked");
        assert_eq!(worker_panic_kind(&error), Some(WorkerKind::Apply));
    }

    #[test]
    fn successful_worker_joins_preserve_the_ui_outcome() {
        let outcome = SessionOutcome::Interrupted(ExecutionStage::Initializing);
        let apply_join = Ok(());
        let plan_join = Ok(());
        let actual = finalize_ui_result(Ok(outcome.clone()), &apply_join, &plan_join)
            .expect("successful worker joins should preserve the UI result");
        assert_eq!(actual, outcome);
    }
}
