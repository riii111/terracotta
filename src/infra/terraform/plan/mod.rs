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

use crate::app::execution::{
    Diagnostic, DiagnosticSeverity, DiagnosticSource, ExecutionEvent, ExecutionEventKind,
    ExecutionPhase,
};
use crate::app::review::PlanReview;
use crate::infra::CancellationToken;

use super::{
    command::{
        ProcessRunner, ProcessStatus, TerraformCommand, TerraformExecutionError,
        TerraformExecutionErrorKind, interrupted_error, non_zero_error, run_command_with_events,
        run_command_with_text_events,
    },
    show::read_review,
    workspace::read_workspace_with_runner,
};

pub(crate) struct PlannedReview {
    review: PlanReview,
    saved_plan: SavedPlan,
}

impl PlannedReview {
    pub(crate) fn into_parts(self) -> (PlanReview, SavedPlan) {
        (self.review, self.saved_plan)
    }
}

pub(crate) struct SavedPlan {
    path: Option<PathBuf>,
}

impl SavedPlan {
    fn create() -> io::Result<Self> {
        create_plan_path().map(|path| Self { path: Some(path) })
    }

    #[must_use]
    fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("saved plan path should exist until cleanup")
    }

    pub(crate) fn cleanup(mut self) -> io::Result<()> {
        self.remove()
    }

    fn remove(&mut self) -> io::Result<()> {
        let Some(path) = self.path.take() else {
            return Ok(());
        };
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Drop for SavedPlan {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

pub(crate) fn run_review(
    root: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    phase_sink: &mut dyn FnMut(ExecutionPhase),
) -> Result<PlannedReview, TerraformExecutionError> {
    let saved_plan = SavedPlan::create().map_err(|error| {
        TerraformExecutionError::new(TerraformExecutionErrorKind::TemporaryPlan {
            message: error.to_string(),
        })
    })?;
    let result = execute_review(
        root,
        saved_plan.path(),
        cancellation,
        runner,
        event_sink,
        phase_sink,
    );

    match result {
        Ok(review) => Ok(PlannedReview { review, saved_plan }),
        Err(error) => match saved_plan.cleanup() {
            Ok(()) => Err(error),
            Err(cleanup) => Err(error.with_cleanup_error(&cleanup)),
        },
    }
}

fn execute_review(
    root: &Path,
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    phase_sink: &mut dyn FnMut(ExecutionPhase),
) -> Result<PlanReview, TerraformExecutionError> {
    let mut diagnostics = Vec::new();
    let (workspace, document, metadata) = {
        phase_sink(ExecutionPhase::Initializing);
        let init_output = {
            let mut sink = |event| {
                collect_and_forward_review_event(event, &mut diagnostics, event_sink);
            };
            run_required_command(
                root,
                TerraformCommand::Init,
                &["init", "-input=false", "-no-color"],
                cancellation,
                runner,
                &mut sink,
                true,
            )?
        };
        diagnostics.extend(init_warning_diagnostics(&init_output.output));
        let workspace = read_workspace_with_runner(root, cancellation, runner)?;
        event_sink(ExecutionEvent {
            received_at: std::time::Instant::now(),
            kind: ExecutionEventKind::Workspace(workspace.clone()),
        });

        phase_sink(ExecutionPhase::Planning);
        let plan_arguments = review_plan_arguments(plan_path);
        let output = {
            let mut sink = |event| {
                collect_and_forward_review_event(event, &mut diagnostics, event_sink);
            };
            run_command_with_events(
                root,
                TerraformCommand::Plan,
                &plan_arguments,
                cancellation,
                runner,
                Some(&mut sink),
            )?
        };
        if output.interrupted {
            return Err(interrupted_error(TerraformCommand::Plan, output));
        }
        if !output.status.is_some_and(ProcessStatus::is_plan_success) {
            return Err(non_zero_error(TerraformCommand::Plan, output));
        }
        let plan_changed = output.status.and_then(ProcessStatus::code) == Some(2);

        phase_sink(ExecutionPhase::Reading);
        let (document, metadata) =
            read_review(root, plan_path, plan_changed, cancellation, runner)?;
        (workspace, document, metadata)
    };
    Ok(PlanReview::new(
        root.to_owned(),
        workspace,
        document,
        metadata,
        diagnostics,
    ))
}

fn collect_and_forward_review_event(
    event: ExecutionEvent,
    diagnostics: &mut Vec<Diagnostic>,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) {
    if let ExecutionEventKind::Diagnostic(diagnostic) = &event.kind
        && matches!(
            diagnostic.severity,
            DiagnosticSeverity::Warning | DiagnosticSeverity::Error | DiagnosticSeverity::Unknown
        )
    {
        diagnostics.push(diagnostic.clone());
    }
    event_sink(event);
}

#[allow(
    clippy::too_many_arguments,
    reason = "command execution keeps root, cancellation, runner, and event ownership explicit"
)]
fn run_required_command(
    root: &Path,
    command: TerraformCommand,
    arguments: &[&str],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    human_output: bool,
) -> Result<super::command::ProcessResult, TerraformExecutionError> {
    let arguments = arguments.iter().map(OsString::from).collect::<Vec<_>>();
    let output = if human_output {
        run_command_with_text_events(
            root,
            command,
            &arguments,
            cancellation,
            runner,
            Some(event_sink),
        )?
    } else {
        run_command_with_events(
            root,
            command,
            &arguments,
            cancellation,
            runner,
            Some(event_sink),
        )?
    };
    if output.interrupted {
        return Err(interrupted_error(command, output));
    }
    if !output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(command, output));
    }
    Ok(output)
}

fn init_warning_diagnostics(output: &super::command::ProcessOutput) -> Vec<Diagnostic> {
    [output.stdout(), output.stderr()]
        .into_iter()
        .flat_map(warnings_from_human_output)
        .collect()
}

fn warnings_from_human_output(output: &[u8]) -> Vec<Diagnostic> {
    let text = String::from_utf8_lossy(output);
    let lines = text.lines().collect::<Vec<_>>();
    let mut diagnostics = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let (line, boxed) = human_output_line(lines[index]);
        let Some(summary) = line.strip_prefix("Warning:").map(str::trim) else {
            index += 1;
            continue;
        };
        let mut detail_lines = Vec::new();
        index += 1;
        if boxed {
            while index < lines.len() && !lines[index].trim_start().starts_with('╵') {
                let (line, _) = human_output_line(lines[index]);
                detail_lines.push(line);
                index += 1;
            }
        }
        let detail_start = detail_lines
            .iter()
            .position(|line| !line.is_empty())
            .unwrap_or(detail_lines.len());
        let detail_end = detail_lines
            .iter()
            .rposition(|line| !line.is_empty())
            .map_or(detail_start, |index| index + 1);
        let detail = detail_lines[detail_start..detail_end].join("\n");
        diagnostics.push(Diagnostic {
            severity: DiagnosticSeverity::Warning,
            summary: summary.to_owned(),
            detail: (!detail.is_empty()).then_some(detail),
            position: None,
            source: DiagnosticSource::Terraform,
        });
    }
    diagnostics
}

fn human_output_line(line: &str) -> (&str, bool) {
    let line = line.trim_start();
    line.strip_prefix('│')
        .map_or_else(|| (line.trim(), false), |line| (line.trim(), true))
}

fn review_plan_arguments(plan_path: &Path) -> Vec<OsString> {
    let mut output = OsString::from("-out=");
    output.push(plan_path.as_os_str());
    vec![
        OsString::from("plan"),
        OsString::from("-input=false"),
        OsString::from("-json"),
        OsString::from("-detailed-exitcode"),
        output,
    ]
}

fn create_plan_path() -> io::Result<PathBuf> {
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
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique Terraform plan path",
    ))
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::app::plan::Plan;

    use super::{
        CancellationToken, ExecutionEvent, ExecutionPhase, OpenOptions, OsString, Path, PathBuf,
        ProcessRunner, ProcessStatus, SystemTime, TerraformCommand, TerraformExecutionError,
        TerraformExecutionErrorKind, UNIX_EPOCH, env, fs, interrupted_error, io, non_zero_error,
        run_command_with_events,
    };

    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    pub(crate) fn run_plan(
        root: &Path,
        cancellation: &CancellationToken,
        runner: &dyn ProcessRunner,
        event_sink: &mut dyn FnMut(ExecutionEvent),
        phase_sink: &mut dyn FnMut(ExecutionPhase),
    ) -> Result<Plan, TerraformExecutionError> {
        let temporary_plan = TemporaryPlan::create().map_err(|error| {
            TerraformExecutionError::new(TerraformExecutionErrorKind::TemporaryPlan {
                message: error.to_string(),
            })
        })?;
        let result = execute_plan(
            root,
            &temporary_plan.path,
            cancellation,
            runner,
            event_sink,
            phase_sink,
        );

        finish_plan(temporary_plan, result)
    }

    pub(crate) fn execute_plan(
        root: &Path,
        plan_path: &Path,
        cancellation: &CancellationToken,
        runner: &dyn ProcessRunner,
        event_sink: &mut dyn FnMut(ExecutionEvent),
        phase_sink: &mut dyn FnMut(ExecutionPhase),
    ) -> Result<Plan, TerraformExecutionError> {
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
        super::super::show::test_support::read_plan(root, plan_path, cancellation, runner)
    }

    pub(crate) fn finish_plan(
        temporary_plan: TemporaryPlan,
        result: Result<Plan, TerraformExecutionError>,
    ) -> Result<Plan, TerraformExecutionError> {
        match temporary_plan.cleanup() {
            Ok(()) => result,
            Err(error) => match result {
                Ok(_) => Err(TerraformExecutionError::new(
                    TerraformExecutionErrorKind::TemporaryPlan {
                        message: format!("failed to remove temporary plan: {error}"),
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

    pub(crate) struct TemporaryPlan {
        pub(crate) path: PathBuf,
    }

    impl TemporaryPlan {
        pub(crate) fn create() -> io::Result<Self> {
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

        pub(crate) fn cleanup(self) -> io::Result<()> {
            match fs::remove_file(self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
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

    use crate::app::execution::{ResourceEvent, ResourceEventKind};

    use super::super::command::{ProcessOutput, ProcessOutputChunk, RunningProcess};
    use super::test_support::{TemporaryPlan, execute_plan, finish_plan, run_plan};
    use super::*;
    use crate::app::execution::{EventStream, ProcessExitStatus, ProcessTermination};
    use crate::app::plan::Plan;
    use std::process::Command;

    fn execute_plan_without_events(
        root: &Path,
        plan_path: &Path,
        cancellation: &CancellationToken,
        runner: &dyn ProcessRunner,
    ) -> Result<Plan, TerraformExecutionError> {
        execute_plan(
            root,
            plan_path,
            cancellation,
            runner,
            &mut |_| {},
            &mut |_| {},
        )
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
            interrupt_count: Rc<Cell<usize>>,
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

        fn request_interrupt(&mut self) -> io::Result<()> {
            if let Some(FakeResponse::Pending {
                interrupt_count, ..
            }) = self.response.as_ref()
            {
                interrupt_count.set(interrupt_count.get() + 1);
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

    fn output_process(status: ProcessStatus, stdout: &[u8], stderr: &[u8]) -> FakeResponse {
        FakeResponse::Exit {
            status,
            output: ProcessOutput::new(stdout.to_vec(), stderr.to_vec()),
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
    ) -> Result<Plan, TerraformExecutionError> {
        let result = execute_plan_without_events(
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

    fn assert_init_warning(review: &PlanReview) {
        assert_eq!(review.diagnostics().len(), 1);
        assert_eq!(
            (
                review.diagnostics()[0].summary.as_str(),
                review.diagnostics()[0].detail.as_deref()
            ),
            (
                "Provider development overrides are in effect",
                Some("Local providers are active.")
            )
        );
    }

    #[test]
    fn review_workflow_runs_init_plan_and_text_then_json_show_in_the_same_root() {
        let plan_json = json!({
            "format_version": "1.0",
            "applyable": true,
            "resource_changes": [{
                "address": "terraform_data.api",
                "change": {"actions": ["update"]}
            }]
        })
        .to_string();
        let runner = FakeRunner::new([
            output_process(
                ProcessStatus::Exited(0),
                b"init out\n",
                "╷\n│ Warning: Provider development overrides are in effect\n│\n│ Local providers are active.\n╵\n".as_bytes(),
            ),
            output_process(ProcessStatus::Exited(0), b"default\n", b""),
            output_process(ProcessStatus::Exited(2), b"", b""),
            output_process(ProcessStatus::Exited(0), b"standard plan\n", b""),
            output_process(ProcessStatus::Exited(0), plan_json.as_bytes(), b""),
        ]);
        let cancellation = CancellationToken::new();
        let mut events = Vec::new();
        let mut phases = Vec::new();

        let planned = run_review(
            Path::new("/root with spaces"),
            &cancellation,
            &runner,
            &mut |event| events.push(event),
            &mut |phase| phases.push(phase),
        )
        .expect("review should complete");
        let (review, saved_plan) = planned.into_parts();
        let saved_path = saved_plan.path().to_owned();

        assert_eq!(review.document().text(), "standard plan\n");
        assert_eq!(review.metadata().changes(), 1);
        assert_init_warning(&review);
        assert!(
            saved_path.exists(),
            "saved plan should outlive plan parsing"
        );
        assert_eq!(
            phases,
            [
                ExecutionPhase::Initializing,
                ExecutionPhase::Planning,
                ExecutionPhase::Reading
            ]
        );
        assert!(events.iter().any(|event| {
            matches!(
                &event.kind,
                ExecutionEventKind::Log(line)
                    if line.stream == EventStream::Stdout && line.text == "init out"
            )
        }));
        assert!(!events.iter().any(|event| {
            matches!(
                &event.kind,
                ExecutionEventKind::Diagnostic(diagnostic)
                    if diagnostic.summary == "Provider development overrides are in effect"
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                &event.kind,
                ExecutionEventKind::Log(line)
                    if line.stream == EventStream::Stderr
                        && line.text == "│ Warning: Provider development overrides are in effect"
            )
        }));

        let invocations = runner.invocations.borrow();
        assert_eq!(invocations.len(), 5);
        assert!(
            invocations
                .iter()
                .all(|invocation| invocation.root == Path::new("/root with spaces"))
        );
        assert_eq!(
            argument_strings(&invocations[0].arguments),
            ["init", "-input=false", "-no-color"]
        );
        assert_eq!(
            argument_strings(&invocations[1].arguments),
            ["workspace", "show"]
        );
        assert_eq!(
            &argument_strings(&invocations[2].arguments)[..4],
            ["plan", "-input=false", "-json", "-detailed-exitcode"]
        );
        assert_eq!(
            argument_strings(&invocations[3].arguments)[..2],
            ["show", "-no-color"]
        );
        assert_eq!(
            argument_strings(&invocations[4].arguments)[..2],
            ["show", "-json"]
        );
        drop(invocations);

        saved_plan.cleanup().expect("saved plan should be removed");
        assert!(!saved_path.exists());
    }

    #[test]
    fn review_workflow_stops_after_init_failure() {
        let runner = FakeRunner::new([output_process(
            ProcessStatus::Exited(1),
            b"",
            b"invalid configuration\n",
        )]);
        let cancellation = CancellationToken::new();

        let result = run_review(
            Path::new("/root"),
            &cancellation,
            &runner,
            &mut |_| {},
            &mut |_| {},
        );
        let Err(error) = result else {
            panic!("init failure should stop the workflow");
        };

        assert!(matches!(
            error.kind(),
            TerraformExecutionErrorKind::NonZero {
                command: TerraformCommand::Init,
                ..
            }
        ));
        assert_eq!(runner.invocations.borrow().len(), 1);
    }

    #[test]
    fn review_workflow_keeps_primary_and_cleanup_failures() {
        let runner = FakeRunner::new([
            successful_process(),
            output_process(ProcessStatus::Exited(0), b"default\n", b""),
            FakeResponse::MakePlanPathDirectory,
            FakeResponse::LaunchError("show unavailable"),
        ]);
        let cancellation = CancellationToken::new();

        let result = run_review(
            Path::new("/root"),
            &cancellation,
            &runner,
            &mut |_| {},
            &mut |_| {},
        );
        let Err(error) = result else {
            panic!("show and cleanup failures should be reported");
        };

        assert!(matches!(
            error.kind(),
            TerraformExecutionErrorKind::Launch {
                command: TerraformCommand::Show,
                ..
            }
        ));
        assert!(error.cleanup_error().is_some());
        let plan_path = runner.invocations.borrow()[2]
            .arguments
            .iter()
            .find_map(|argument| {
                argument
                    .to_str()
                    .and_then(|argument| argument.strip_prefix("-out="))
                    .map(PathBuf::from)
            })
            .expect("plan invocation should contain the saved path");
        assert!(plan_path.is_dir());
        fs::remove_dir(plan_path).expect("synthetic plan directory should be removed");
    }

    #[test]
    fn runs_plan_then_show_in_explicit_root_with_argument_boundaries() {
        let runner = FakeRunner::new([successful_process(), show_process()]);
        let cancellation = CancellationToken::new();
        let (temporary_plan, directory) = temporary_plan_with_space();
        let plan_path = temporary_plan.path.clone();

        let result = run_fake(&runner, temporary_plan, &cancellation)
            .expect("Terraform plan should be returned");

        assert!(result.changes.is_empty());
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

        let result = execute_plan(
            Path::new("/root"),
            &temporary_plan.path,
            &cancellation,
            &runner,
            &mut |event| events.push(event),
            &mut |_| {},
        );
        let result = finish_plan(temporary_plan, result).expect("plan should be returned");

        assert!(result.changes.is_empty());
        assert!(matches!(
            events.first().map(|event| &event.kind),
            Some(ExecutionEventKind::Resource(ResourceEvent {
                address,
                kind: ResourceEventKind::RefreshStart,
                ..
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
        let interrupt_count = Rc::new(Cell::new(0));
        let runner = FakeRunner::new([FakeResponse::Pending {
            cancellation: cancellation.clone(),
            output: ProcessOutput::empty(),
            interrupt_count: interrupt_count.clone(),
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
        assert_eq!(
            interrupt_count.get(),
            1,
            "interrupt should be requested once"
        );
        assert_eq!(runner.invocations.borrow().len(), 1);
        assert!(error.cleanup_error().is_none());
        fs::remove_dir(directory).expect("test directory should be empty");
    }

    #[test]
    fn interrupts_and_reaps_show_before_cleanup() {
        let cancellation = CancellationToken::new();
        let interrupt_count = Rc::new(Cell::new(0));
        let runner = FakeRunner::new([
            successful_process(),
            FakeResponse::Pending {
                cancellation: cancellation.clone(),
                output: ProcessOutput::empty(),
                interrupt_count: interrupt_count.clone(),
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
        assert_eq!(
            interrupt_count.get(),
            1,
            "interrupt should be requested once"
        );
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
            TerraformExecutionErrorKind::TemporaryPlan { .. }
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
        let mut ignore_event = |_| {};
        let mut ignore_phase = |_| {};
        let result = run_plan(
            &directory,
            &cancellation,
            &super::super::command::SystemProcessRunner,
            &mut ignore_event,
            &mut ignore_phase,
        );
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

        let plan = result.expect("Terraform plan should be obtained");
        assert_eq!(plan.summary.creates, 1);
        assert_eq!(plan.summary.updates, 2);
        assert_eq!(plan.summary.replaces, 1);
        assert_eq!(plan.summary.deletes, 1);
    }
}
