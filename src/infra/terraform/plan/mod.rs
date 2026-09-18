use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::app::execution::{ExecutionEvent, ExecutionPhase};

use super::{
    command::{
        CancellationToken, ProcessRunner, ProcessStatus, SystemProcessRunner, TerraformCommand,
        TerraformExecutionError, TerraformExecutionErrorKind, interrupted_error, non_zero_error,
        run_command_with_events,
    },
    show::{PlanExecution, read_plan},
};

pub(crate) fn run_plan(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<PlanExecution, TerraformExecutionError> {
    let mut ignore_event = |_event: ExecutionEvent| {};
    run_plan_with_events(root, cancellation, &mut ignore_event)
}

pub(crate) fn run_plan_with_events(
    root: &Path,
    cancellation: &CancellationToken,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) -> Result<PlanExecution, TerraformExecutionError> {
    run_plan_with_events_with_runner(root, cancellation, &SystemProcessRunner, event_sink)
}

pub(crate) fn run_plan_with_events_with_runner(
    root: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) -> Result<PlanExecution, TerraformExecutionError> {
    let mut ignore_phase = |_| {};
    run_plan_with_events_with_runner_and_phase(
        root,
        cancellation,
        runner,
        event_sink,
        &mut ignore_phase,
    )
}

pub(crate) fn run_plan_with_events_with_runner_and_phase(
    root: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    phase_sink: &mut dyn FnMut(ExecutionPhase),
) -> Result<PlanExecution, TerraformExecutionError> {
    let temporary_plan = TemporaryPlan::create().map_err(|error| {
        TerraformExecutionError::new(TerraformExecutionErrorKind::TemporaryPlan {
            message: error.to_string(),
        })
    })?;
    let result = execute_plan_with_events_and_phase(
        root,
        &temporary_plan.path,
        cancellation,
        runner,
        event_sink,
        phase_sink,
    );

    finish_plan(temporary_plan, result)
}

fn execute_plan_with_events(
    root: &Path,
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) -> Result<PlanExecution, TerraformExecutionError> {
    let mut ignore_phase = |_| {};
    execute_plan_with_events_and_phase(
        root,
        plan_path,
        cancellation,
        runner,
        event_sink,
        &mut ignore_phase,
    )
}

fn execute_plan_with_events_and_phase(
    root: &Path,
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    phase_sink: &mut dyn FnMut(ExecutionPhase),
) -> Result<PlanExecution, TerraformExecutionError> {
    let plan_arguments = plan_arguments(plan_path);
    let plan_output = run_command_with_events(
        root,
        TerraformCommand::Plan,
        &plan_arguments,
        cancellation,
        runner,
        Some(event_sink),
    )?;
    if plan_output.interrupted {
        return Err(interrupted_error(TerraformCommand::Plan, plan_output));
    }
    if !plan_output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(TerraformCommand::Plan, plan_output));
    }

    phase_sink(ExecutionPhase::Reading);
    read_plan(root, plan_path, cancellation, runner)
}

fn finish_plan(
    temporary_plan: TemporaryPlan,
    result: Result<PlanExecution, TerraformExecutionError>,
) -> Result<PlanExecution, TerraformExecutionError> {
    match temporary_plan.cleanup() {
        Ok(()) => result,
        Err(error) => match result {
            Ok(_) => Err(TerraformExecutionError::new(
                TerraformExecutionErrorKind::Cleanup {
                    message: error.to_string(),
                },
            )),
            Err(execution_error) => Err(execution_error.with_cleanup_error(&error)),
        },
    }
}

fn plan_arguments(plan_path: &Path) -> Vec<OsString> {
    let mut output = OsString::from("-out=");
    output.push(plan_path.as_os_str());
    vec![
        OsString::from("plan"),
        OsString::from("-input=false"),
        OsString::from("-json"),
        output,
    ]
}

struct TemporaryPlan {
    path: PathBuf,
}

impl TemporaryPlan {
    fn create() -> io::Result<Self> {
        let directory = env::temp_dir();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let process_id = std::process::id();

        for attempt in 0..100 {
            let path = directory.join(format!(
                "terracotta-{process_id}-{timestamp}-{attempt}.tfplan"
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            match options.open(&path) {
                Ok(_) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }

        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique Terraform plan path",
        ))
    }

    fn cleanup(self) -> io::Result<()> {
        match fs::remove_file(self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
    };

    use serde_json::json;

    use crate::app::execution::{Diagnostic, DiagnosticSource, ResourceEvent, ResourceEventKind};

    use super::super::command::{ProcessOutput, ProcessOutputChunk, RunningProcess};
    use super::*;
    use crate::app::execution::{
        EventStream, ExecutionEventKind, ProcessExitStatus, ProcessTermination,
    };
    use std::process::Command;

    fn execute_plan(
        root: &Path,
        plan_path: &Path,
        cancellation: &CancellationToken,
        runner: &dyn ProcessRunner,
    ) -> Result<PlanExecution, TerraformExecutionError> {
        let mut ignore_event = |_event: ExecutionEvent| {};
        execute_plan_with_events(root, plan_path, cancellation, runner, &mut ignore_event)
    }

    const PLAN_JSON: &[u8] = br#"{"format_version":"1.0"}"#;

    #[derive(Debug)]
    struct Invocation {
        root: PathBuf,
        arguments: Vec<OsString>,
    }

    struct FakeRunner {
        responses: RefCell<VecDeque<FakeResponse>>,
        invocations: RefCell<Vec<Invocation>>,
    }

    enum FakeResponse {
        LaunchError(&'static str),
        Exit {
            status: ProcessStatus,
            output: ProcessOutput,
        },
        Streaming {
            status: ProcessStatus,
            output: ProcessOutput,
            chunks: VecDeque<ProcessOutputChunk>,
        },
        Pending {
            cancellation: CancellationToken,
            output: ProcessOutput,
            killed: Rc<Cell<bool>>,
        },
        MakePlanPathDirectory,
    }

    struct FakeProcess {
        response: Option<FakeResponse>,
        plan_path: Option<PathBuf>,
    }

    impl FakeRunner {
        fn new(responses: impl IntoIterator<Item = FakeResponse>) -> Self {
            Self {
                responses: RefCell::new(responses.into_iter().collect()),
                invocations: RefCell::new(Vec::new()),
            }
        }
    }

    impl ProcessRunner for FakeRunner {
        fn start(
            &self,
            root: &Path,
            arguments: &[OsString],
        ) -> io::Result<Box<dyn RunningProcess>> {
            self.invocations.borrow_mut().push(Invocation {
                root: root.to_owned(),
                arguments: arguments.to_vec(),
            });
            let response = self
                .responses
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| io::Error::other("fake response was not configured"))?;
            if let FakeResponse::LaunchError(message) = response {
                return Err(io::Error::other(message));
            }

            let plan_path = arguments.iter().find_map(|argument| {
                argument
                    .to_str()
                    .and_then(|argument| argument.strip_prefix("-out="))
                    .map(PathBuf::from)
            });
            Ok(Box::new(FakeProcess {
                response: Some(response),
                plan_path,
            }))
        }
    }

    impl RunningProcess for FakeProcess {
        fn poll_output(&mut self) -> io::Result<Vec<ProcessOutputChunk>> {
            match self.response.as_mut() {
                Some(FakeResponse::Streaming { chunks, .. }) => Ok(chunks.drain(..).collect()),
                _ => Ok(Vec::new()),
            }
        }

        fn try_wait(&mut self) -> io::Result<Option<ProcessStatus>> {
            match self.response.as_mut() {
                Some(FakeResponse::Exit { status, .. }) => Ok(Some(*status)),
                Some(FakeResponse::Streaming { status, chunks, .. }) => {
                    if chunks.is_empty() {
                        Ok(Some(*status))
                    } else {
                        Ok(None)
                    }
                }
                Some(FakeResponse::Pending { cancellation, .. }) => {
                    cancellation.cancel();
                    Ok(None)
                }
                Some(FakeResponse::MakePlanPathDirectory) => {
                    let path = self
                        .plan_path
                        .take()
                        .ok_or_else(|| io::Error::other("plan path was not passed"))?;
                    fs::remove_file(&path)?;
                    fs::create_dir(path)?;
                    Ok(Some(ProcessStatus::Exited(0)))
                }
                None => Err(io::Error::other("fake process was already consumed")),
                Some(FakeResponse::LaunchError(_)) => {
                    Err(io::Error::other("launch error cannot become a process"))
                }
            }
        }

        fn kill(&mut self) -> io::Result<()> {
            if let Some(FakeResponse::Pending { killed, .. }) = self.response.as_ref() {
                killed.set(true);
            }
            Ok(())
        }

        fn wait(&mut self) -> io::Result<ProcessStatus> {
            Ok(ProcessStatus::Signaled)
        }

        fn collect_output(self: Box<Self>) -> io::Result<ProcessOutput> {
            match self.response {
                Some(
                    FakeResponse::Exit { output, .. }
                    | FakeResponse::Streaming { output, .. }
                    | FakeResponse::Pending { output, .. },
                ) => Ok(output),
                Some(FakeResponse::MakePlanPathDirectory) => Ok(ProcessOutput::empty()),
                Some(FakeResponse::LaunchError(_)) | None => {
                    Err(io::Error::other("fake process output was unavailable"))
                }
            }
        }
    }

    fn successful_process() -> FakeResponse {
        FakeResponse::Exit {
            status: ProcessStatus::Exited(0),
            output: ProcessOutput::new(Vec::new(), Vec::new()),
        }
    }

    fn show_process() -> FakeResponse {
        FakeResponse::Exit {
            status: ProcessStatus::Exited(0),
            output: ProcessOutput::new(PLAN_JSON.to_vec(), Vec::new()),
        }
    }

    fn streaming_process(
        status: ProcessStatus,
        stdout_chunks: impl IntoIterator<Item = Vec<u8>>,
        stderr_chunks: impl IntoIterator<Item = Vec<u8>>,
    ) -> FakeResponse {
        let stdout_chunks = stdout_chunks.into_iter().map(|bytes| ProcessOutputChunk {
            stream: EventStream::Stdout,
            bytes,
        });
        let stderr_chunks = stderr_chunks.into_iter().map(|bytes| ProcessOutputChunk {
            stream: EventStream::Stderr,
            bytes,
        });
        let chunks: VecDeque<_> = stdout_chunks.chain(stderr_chunks).collect();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        for chunk in &chunks {
            match chunk.stream {
                EventStream::Stdout => stdout.extend_from_slice(&chunk.bytes),
                EventStream::Stderr => stderr.extend_from_slice(&chunk.bytes),
            }
        }
        FakeResponse::Streaming {
            status,
            output: ProcessOutput::new(stdout, stderr),
            chunks,
        }
    }

    fn temporary_plan_with_space() -> (TemporaryPlan, PathBuf) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = (0..100)
            .find_map(|attempt| {
                let directory = env::temp_dir().join(format!(
                    "terracotta test plan {} {timestamp} {attempt}",
                    std::process::id()
                ));
                match fs::create_dir(&directory) {
                    Ok(()) => Some(directory),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
                    Err(error) => panic!("test directory should be created: {error}"),
                }
            })
            .expect("test directory should be unique");
        let path = directory.join("saved plan.tfplan");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("test plan should be created");
        (TemporaryPlan { path }, directory)
    }

    fn run_fake(
        runner: &FakeRunner,
        temporary_plan: TemporaryPlan,
        cancellation: &CancellationToken,
    ) -> Result<PlanExecution, TerraformExecutionError> {
        let result = execute_plan(
            Path::new("/root with spaces"),
            &temporary_plan.path,
            cancellation,
            runner,
        );
        finish_plan(temporary_plan, result)
    }

    fn argument_strings(arguments: &[OsString]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn runs_plan_then_show_in_explicit_root_with_argument_boundaries() {
        let runner = FakeRunner::new([successful_process(), show_process()]);
        let cancellation = CancellationToken::new();
        let (temporary_plan, directory) = temporary_plan_with_space();
        let plan_path = temporary_plan.path.clone();

        let result = run_fake(&runner, temporary_plan, &cancellation)
            .expect("Terraform plan should be returned");

        assert_eq!(result.json(), PLAN_JSON);
        assert!(result.plan().changes.is_empty());
        let invocations = runner.invocations.borrow();
        assert_eq!(invocations.len(), 2);
        assert_eq!(invocations[0].root, Path::new("/root with spaces"));
        assert_eq!(
            argument_strings(&invocations[0].arguments),
            vec![
                "plan".to_owned(),
                "-input=false".to_owned(),
                "-json".to_owned(),
                format!("-out={}", plan_path.display()),
            ]
        );
        assert_eq!(
            argument_strings(&invocations[1].arguments),
            vec![
                "show".to_owned(),
                "-json".to_owned(),
                plan_path.to_string_lossy().into_owned(),
            ]
        );
        assert!(!plan_path.exists(), "temporary plan should be removed");
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn delivers_plan_events_before_process_termination_and_keeps_show_silent() {
        let refresh_start =
            br#"{"type":"refresh_start","hook":{"resource":{"addr":"aws_vpc.main"}}}
"#
            .to_vec();
        let refresh_complete =
            br#"{"type":"refresh_complete","hook":{"resource":{"addr":"aws_vpc.main"}}}
"#
            .to_vec();
        let runner = FakeRunner::new([
            streaming_process(
                ProcessStatus::Exited(0),
                [refresh_start, refresh_complete],
                [b"provider warning".to_vec()],
            ),
            show_process(),
        ]);
        let cancellation = CancellationToken::new();
        let (temporary_plan, directory) = temporary_plan_with_space();
        let mut events = Vec::new();

        let result = execute_plan_with_events(
            Path::new("/root"),
            &temporary_plan.path,
            &cancellation,
            &runner,
            &mut |event| events.push(event),
        );
        let result = finish_plan(temporary_plan, result).expect("plan should be returned");

        assert!(result.plan().changes.is_empty());
        assert!(matches!(
            events.first().map(|event| &event.kind),
            Some(ExecutionEventKind::Resource(ResourceEvent {
                address,
                kind: ResourceEventKind::RefreshStart,
            })) if address == "aws_vpc.main"
        ));
        assert!(matches!(
            events.get(1).map(|event| &event.kind),
            Some(ExecutionEventKind::Resource(ResourceEvent {
                kind: ResourceEventKind::RefreshComplete,
                ..
            }))
        ));
        assert!(matches!(
            events.get(2).map(|event| &event.kind),
            Some(ExecutionEventKind::Diagnostic(Diagnostic {
                source: DiagnosticSource::NonJson {
                    stream: EventStream::Stderr
                },
                ..
            }))
        ));
        assert!(matches!(
            events.last().map(|event| &event.kind),
            Some(ExecutionEventKind::Terminated(ProcessTermination {
                status: ProcessExitStatus::Exited(0),
                interrupted: false,
            }))
        ));
        assert_eq!(runner.invocations.borrow().len(), 2);
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn distinguishes_plan_launch_failure_and_skips_show() {
        let runner = FakeRunner::new([FakeResponse::LaunchError("not found")]);
        let cancellation = CancellationToken::new();
        let (temporary_plan, directory) = temporary_plan_with_space();

        let error =
            run_fake(&runner, temporary_plan, &cancellation).expect_err("plan launch should fail");

        assert!(matches!(
            error.kind(),
            TerraformExecutionErrorKind::Launch {
                command: TerraformCommand::Plan,
                ..
            }
        ));
        assert_eq!(runner.invocations.borrow().len(), 1);
        assert!(error.cleanup_error().is_none());
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn distinguishes_plan_nonzero_exit_and_keeps_output_out_of_error_text() {
        let runner = FakeRunner::new([FakeResponse::Exit {
            status: ProcessStatus::Exited(1),
            output: ProcessOutput::new(
                b"secret plan value".to_vec(),
                b"secret diagnostic".to_vec(),
            ),
        }]);
        let cancellation = CancellationToken::new();
        let (temporary_plan, directory) = temporary_plan_with_space();

        let error = run_fake(&runner, temporary_plan, &cancellation)
            .expect_err("non-zero plan should fail");

        let TerraformExecutionErrorKind::NonZero { output, .. } = error.kind() else {
            panic!("expected a non-zero process error");
        };
        assert_eq!(output.stdout(), b"secret plan value");
        assert_eq!(output.stderr(), b"secret diagnostic");
        assert!(!error.to_string().contains("secret"));
        assert!(!format!("{error:?}").contains("secret"));
        assert_eq!(runner.invocations.borrow().len(), 1);
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn distinguishes_show_launch_failure_after_a_successful_plan() {
        let runner = FakeRunner::new([
            successful_process(),
            FakeResponse::LaunchError("show unavailable"),
        ]);
        let cancellation = CancellationToken::new();
        let (temporary_plan, directory) = temporary_plan_with_space();

        let error =
            run_fake(&runner, temporary_plan, &cancellation).expect_err("show launch should fail");

        assert!(matches!(
            error.kind(),
            TerraformExecutionErrorKind::Launch {
                command: TerraformCommand::Show,
                ..
            }
        ));
        assert_eq!(runner.invocations.borrow().len(), 2);
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn distinguishes_show_nonzero_exit() {
        let runner = FakeRunner::new([
            successful_process(),
            FakeResponse::Exit {
                status: ProcessStatus::Exited(1),
                output: ProcessOutput::new(Vec::new(), b"show failed".to_vec()),
            },
        ]);
        let cancellation = CancellationToken::new();
        let (temporary_plan, directory) = temporary_plan_with_space();

        let error = run_fake(&runner, temporary_plan, &cancellation)
            .expect_err("non-zero show should fail");

        assert!(matches!(
            error.kind(),
            TerraformExecutionErrorKind::NonZero {
                command: TerraformCommand::Show,
                ..
            }
        ));
        assert_eq!(runner.invocations.borrow().len(), 2);
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn interrupts_and_reaps_plan_before_cleanup_without_starting_show() {
        let cancellation = CancellationToken::new();
        let killed = Rc::new(Cell::new(false));
        let runner = FakeRunner::new([FakeResponse::Pending {
            cancellation: cancellation.clone(),
            output: ProcessOutput::empty(),
            killed: killed.clone(),
        }]);
        let (temporary_plan, directory) = temporary_plan_with_space();

        let error = run_fake(&runner, temporary_plan, &cancellation)
            .expect_err("cancelled plan should fail");

        assert!(matches!(
            error.kind(),
            TerraformExecutionErrorKind::Interrupted {
                command: TerraformCommand::Plan,
                ..
            }
        ));
        assert!(killed.get(), "interrupted process should be killed");
        assert_eq!(runner.invocations.borrow().len(), 1);
        assert!(error.cleanup_error().is_none());
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn interrupts_and_reaps_show_before_cleanup() {
        let cancellation = CancellationToken::new();
        let killed = Rc::new(Cell::new(false));
        let runner = FakeRunner::new([
            successful_process(),
            FakeResponse::Pending {
                cancellation: cancellation.clone(),
                output: ProcessOutput::empty(),
                killed: killed.clone(),
            },
        ]);
        let (temporary_plan, directory) = temporary_plan_with_space();

        let error = run_fake(&runner, temporary_plan, &cancellation)
            .expect_err("cancelled show should fail");

        assert!(matches!(
            error.kind(),
            TerraformExecutionErrorKind::Interrupted {
                command: TerraformCommand::Show,
                ..
            }
        ));
        assert!(killed.get(), "interrupted process should be killed");
        assert_eq!(runner.invocations.borrow().len(), 2);
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn reports_cleanup_failure_separately() {
        let runner = FakeRunner::new([FakeResponse::MakePlanPathDirectory, show_process()]);
        let cancellation = CancellationToken::new();
        let (temporary_plan, directory) = temporary_plan_with_space();
        let plan_path = temporary_plan.path.clone();

        let error = run_fake(&runner, temporary_plan, &cancellation)
            .expect_err("cleanup of a directory should fail");

        assert!(matches!(
            error.kind(),
            TerraformExecutionErrorKind::Cleanup { .. }
        ));
        assert!(plan_path.is_dir());
        fs::remove_dir(plan_path).expect("test plan directory should be removed");
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn keeps_parser_failures_distinct_from_process_failures() {
        let runner = FakeRunner::new([
            successful_process(),
            FakeResponse::Exit {
                status: ProcessStatus::Exited(0),
                output: ProcessOutput::new(
                    json!({"format_version": "2.0"}).to_string().into_bytes(),
                    Vec::new(),
                ),
            },
        ]);
        let cancellation = CancellationToken::new();
        let (temporary_plan, directory) = temporary_plan_with_space();

        let error = run_fake(&runner, temporary_plan, &cancellation)
            .expect_err("unsupported plan format should fail");

        assert!(matches!(
            error.kind(),
            TerraformExecutionErrorKind::InvalidPlan { .. }
        ));
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    #[ignore = "requires Terraform CLI"]
    fn gets_saved_plan_from_the_basic_terraform_scenario() {
        let setup = Command::new("python3")
            .args(["fixtures/basic/scenario.py", "setup"])
            .output()
            .expect("scenario setup should start");
        assert!(
            setup.status.success(),
            "scenario setup failed: {}",
            String::from_utf8_lossy(&setup.stderr)
        );
        let directory = PathBuf::from(
            String::from_utf8(setup.stdout)
                .expect("scenario path should be UTF-8")
                .trim(),
        );

        let cancellation = CancellationToken::new();
        let result = run_plan(&directory, &cancellation);
        let cleanup = Command::new("python3")
            .args(["fixtures/basic/scenario.py", "clean"])
            .arg(&directory)
            .output()
            .expect("scenario cleanup should start");
        assert!(
            cleanup.status.success(),
            "scenario cleanup failed: {}",
            String::from_utf8_lossy(&cleanup.stderr)
        );

        let execution = result.expect("Terraform plan should be obtained");
        assert_eq!(execution.plan().summary.creates, 1);
        assert_eq!(execution.plan().summary.updates, 2);
        assert_eq!(execution.plan().summary.replaces, 1);
        assert_eq!(execution.plan().summary.deletes, 1);
    }
}
