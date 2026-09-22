use std::ffi::OsString;
use std::path::Path;

use crate::app::execution::{ApplyStatus, ExecutionEvent};
use crate::infra::CancellationToken;

use super::command::{
    ProcessRunner, ProcessStatus, TerraformCommand, TerraformExecutionError, interrupted_error,
    run_command_with_text_events,
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApplyResult {
    status: ApplyStatus,
    summary_line: Option<String>,
}

impl ApplyResult {
    #[must_use]
    pub(crate) const fn status(&self) -> ApplyStatus {
        self.status
    }

    #[must_use]
    pub(crate) fn summary_line(&self) -> Option<&str> {
        self.summary_line.as_deref()
    }
}

pub(crate) fn run_apply_with_arguments(
    root: &Path,
    global_arguments: &[OsString],
    apply_arguments: &[OsString],
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) -> Result<ApplyResult, TerraformExecutionError> {
    let mut arguments = global_arguments.to_vec();
    arguments.push(OsString::from("apply"));
    arguments.push(OsString::from("-input=false"));
    arguments.extend(apply_arguments.iter().cloned());
    arguments.push(plan_path.as_os_str().to_owned());
    let output = run_command_with_text_events(
        root,
        TerraformCommand::Apply,
        &arguments,
        cancellation,
        runner,
        Some(event_sink),
    )?;
    if output.interrupted {
        return Ok(ApplyResult {
            status: ApplyStatus::Interrupted,
            summary_line: None,
        });
    }

    let Some(status) = output.status else {
        return Err(interrupted_error(TerraformCommand::Apply, output));
    };
    if !matches!(status, ProcessStatus::Exited(0)) {
        return Ok(ApplyResult {
            status: ApplyStatus::Failed,
            summary_line: None,
        });
    }

    let summary_line = output
        .output
        .stdout()
        .split(|byte| *byte == b'\n')
        .filter_map(|line| std::str::from_utf8(line).ok())
        .map(str::trim_end)
        .find(|line| line.starts_with("Apply complete! Resources:"))
        .map(str::to_owned)
        .or_else(|| Some("Apply complete.".to_owned()));
    Ok(ApplyResult {
        status: ApplyStatus::Succeeded,
        summary_line,
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, io};

    use super::*;
    use crate::app::execution::{EventStream, ExecutionEventKind, ExecutionLogLine};
    use crate::infra::terraform::test_support::{ProcessOutput, RunningProcess};

    struct FakeRunner {
        response: RefCell<Option<(ProcessStatus, ProcessOutput)>>,
        arguments: RefCell<Vec<OsString>>,
    }

    struct FakeProcess {
        status: ProcessStatus,
        output: ProcessOutput,
    }

    impl ProcessRunner for FakeRunner {
        fn start(
            &self,
            _root: &Path,
            arguments: &[OsString],
        ) -> io::Result<Box<dyn RunningProcess>> {
            self.arguments
                .borrow_mut()
                .extend(arguments.iter().cloned());
            let (status, output) = self
                .response
                .borrow_mut()
                .take()
                .ok_or_else(|| io::Error::other("fake process was already started"))?;
            Ok(Box::new(FakeProcess { status, output }))
        }
    }

    impl RunningProcess for FakeProcess {
        fn try_wait(&mut self) -> io::Result<Option<ProcessStatus>> {
            Ok(Some(self.status))
        }

        fn request_interrupt(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn wait(&mut self) -> io::Result<ProcessStatus> {
            Ok(self.status)
        }

        fn collect_output(self: Box<Self>) -> io::Result<ProcessOutput> {
            Ok(self.output)
        }
    }

    #[test]
    fn successful_apply_preserves_human_output_and_summary() {
        let runner = FakeRunner {
            response: RefCell::new(Some((
                ProcessStatus::Exited(0),
                ProcessOutput::new(
                    b"Applying saved plan...\nApply complete! Resources: 1 added, 0 changed, 0 destroyed.\n"
                        .to_vec(),
                    b"warning: retained\n".to_vec(),
                ),
            ))),
            arguments: RefCell::new(Vec::new()),
        };
        let cancellation = CancellationToken::new();
        let mut events = Vec::new();

        let result = run_apply_with_arguments(
            Path::new("/project"),
            &[],
            &[OsString::from("-no-color")],
            Path::new("/project/review.tfplan"),
            &cancellation,
            &runner,
            &mut |event| events.push(event),
        )
        .expect("apply should finish");

        assert_eq!(result.status(), ApplyStatus::Succeeded);
        assert_eq!(
            result.summary_line(),
            Some("Apply complete! Resources: 1 added, 0 changed, 0 destroyed.")
        );
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            ExecutionEventKind::Log(ExecutionLogLine { text, .. })
                if text == "Applying saved plan..."
        )));
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            ExecutionEventKind::Log(ExecutionLogLine { stream: EventStream::Stderr, text })
                if text == "warning: retained"
        )));
        assert_eq!(
            runner.arguments.borrow().as_slice(),
            [
                OsString::from("apply"),
                OsString::from("-input=false"),
                OsString::from("-no-color"),
                OsString::from("/project/review.tfplan"),
            ]
        );
    }

    #[test]
    fn nonzero_apply_is_failed_and_cancellation_is_interrupted() {
        let failed_runner = FakeRunner {
            response: RefCell::new(Some((
                ProcessStatus::Exited(1),
                ProcessOutput::new(Vec::new(), b"Error: apply failed\n".to_vec()),
            ))),
            arguments: RefCell::new(Vec::new()),
        };
        let mut events = Vec::new();
        let failed = run_apply_with_arguments(
            Path::new("/project"),
            &[],
            &[OsString::from("-no-color")],
            Path::new("/project/review.tfplan"),
            &CancellationToken::new(),
            &failed_runner,
            &mut |event| events.push(event),
        )
        .expect("failed apply should return a result");
        assert_eq!(failed.status(), ApplyStatus::Failed);
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            ExecutionEventKind::Log(ExecutionLogLine { text, .. })
                if text == "Error: apply failed"
        )));

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let interrupted = run_apply_with_arguments(
            Path::new("/project"),
            &[],
            &[OsString::from("-no-color")],
            Path::new("/project/review.tfplan"),
            &cancelled,
            &failed_runner,
            &mut |_| {},
        )
        .expect("pre-cancelled apply should return a result");
        assert_eq!(interrupted.status(), ApplyStatus::Interrupted);
    }

    #[test]
    fn successful_apply_without_a_summary_still_succeeds_with_a_fallback() {
        let runner = FakeRunner {
            response: RefCell::new(Some((
                ProcessStatus::Exited(0),
                ProcessOutput::new(b"Applying saved plan...\n".to_vec(), Vec::new()),
            ))),
            arguments: RefCell::new(Vec::new()),
        };

        let result = run_apply_with_arguments(
            Path::new("/project"),
            &[],
            &[OsString::from("-no-color")],
            Path::new("/project/review.tfplan"),
            &CancellationToken::new(),
            &runner,
            &mut |_| {},
        )
        .expect("missing summary must not fail a successful apply");

        assert_eq!(result.status(), ApplyStatus::Succeeded);
        assert_eq!(result.summary_line(), Some("Apply complete."));
    }
}
