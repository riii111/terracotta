use std::{
    io::{self, IsTerminal, Write},
    path::Path,
    process::ExitCode,
    sync::mpsc,
    thread::{self, JoinHandle},
    time::Instant,
};

use crate::{
    app::{
        execution::{ExecutionContext, ExecutionState},
        progress::{ExecutionEvent, ExecutionEventKind, ExecutionPhase},
        review::PlanReviewMessage,
    },
    infra::{
        review,
        terraform::{CancellationToken, TerraformExecutionErrorKind},
    },
    ui::{self, UiOutcome},
};

const EXECUTION_FAILURE: u8 = 1;
const INTERRUPTED: u8 = 130;

#[allow(
    clippy::redundant_pub_crate,
    reason = "the library facade is the only public runtime entry point"
)]
pub(crate) fn run_plan(root: &Path, compare_ref: Option<&str>) -> ExitCode {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        report_error("terracotta plan requires an interactive terminal");
        return ExitCode::from(EXECUTION_FAILURE);
    }

    let cancellation = CancellationToken::new();
    let (sender, receiver) = mpsc::channel();
    let worker_cancellation = cancellation.clone();
    let worker_root = root.to_owned();
    let worker_compare_ref = compare_ref.map(str::to_owned);
    let worker = match thread::Builder::new()
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
            let result = review::run_review_with_events_and_phases(
                &worker_root,
                worker_compare_ref.as_deref(),
                &worker_cancellation,
                &mut event_sink,
                &mut phase_sink,
            );
            let message = match result {
                Ok(review) => PlanReviewMessage::Completed(review),
                Err(error) => PlanReviewMessage::Failed {
                    message: error.to_string(),
                    interrupted: matches!(
                        error.kind(),
                        TerraformExecutionErrorKind::Interrupted { .. }
                    ),
                },
            };
            let _ = sender.send(message);
        }) {
        Ok(worker) => worker,
        Err(error) => {
            report_error(&format!("failed to start the plan worker: {error}"));
            return ExitCode::from(EXECUTION_FAILURE);
        }
    };
    let mut worker = WorkerGuard {
        cancellation: cancellation.clone(),
        handle: Some(worker),
    };
    let context = initial_execution_context(root, compare_ref);
    let ui_result = ratatui::run(|terminal| {
        ui::run_connected(
            terminal,
            ExecutionState::with_context(Instant::now(), context),
            &receiver,
            &mut || cancellation.cancel(),
        )
    });
    if ui_result.is_err() {
        cancellation.cancel();
    }
    let worker_panicked = worker.join();

    match (ui_result, worker_panicked) {
        (Ok(UiOutcome::Reviewed), Ok(())) => ExitCode::SUCCESS,
        (Ok(UiOutcome::Interrupted), Ok(())) => ExitCode::from(INTERRUPTED),
        (Ok(UiOutcome::Failed), Ok(())) => ExitCode::from(EXECUTION_FAILURE),
        (Err(error), Ok(())) => {
            report_error(&format!("TUI failed: {error}"));
            ExitCode::from(EXECUTION_FAILURE)
        }
        (_, Err(_)) => {
            report_error("the plan worker terminated unexpectedly");
            ExitCode::from(EXECUTION_FAILURE)
        }
    }
}

fn initial_execution_context(root: &Path, compare_ref: Option<&str>) -> ExecutionContext {
    ExecutionContext::known(
        root.display().to_string(),
        "loading...",
        "loading...",
        comparison_label(compare_ref),
    )
}

fn comparison_label(compare_ref: Option<&str>) -> String {
    compare_ref.map_or_else(
        || "working tree vs HEAD".to_owned(),
        |compare_ref| format!("HEAD vs merge-base({compare_ref})"),
    )
}

fn report_error(message: &str) {
    let _ = writeln!(io::stderr(), "{message}");
}

struct WorkerGuard {
    cancellation: CancellationToken,
    handle: Option<JoinHandle<()>>,
}

impl WorkerGuard {
    fn join(&mut self) -> thread::Result<()> {
        self.handle
            .take()
            .expect("plan worker should be present")
            .join()
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
