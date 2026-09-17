use std::{
    env,
    ffi::{OsStr, OsString},
    fmt::{Debug, Display, Formatter},
    fs::{self, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::app::plan::Plan;

use super::plan::{PlanParseError, parse_plan_json_bytes};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Default)]
pub(crate) struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    #[must_use]
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PlanExecution {
    json: Vec<u8>,
    plan: Plan,
}

impl PlanExecution {
    #[must_use]
    pub(crate) fn json(&self) -> &[u8] {
        &self.json
    }

    #[must_use]
    pub(crate) const fn plan(&self) -> &Plan {
        &self.plan
    }
}

impl Debug for PlanExecution {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlanExecution")
            .field("json", &"<redacted>")
            .field("plan", &self.plan)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerraformCommand {
    Plan,
    Show,
}

impl Display for TerraformCommand {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Plan => "plan",
            Self::Show => "show",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessStatus {
    Exited(i32),
    Signaled,
}

impl ProcessStatus {
    #[must_use]
    const fn is_success(self) -> bool {
        matches!(self, Self::Exited(0))
    }
}

impl Display for ProcessStatus {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exited(code) => write!(formatter, "exit status {code}"),
            Self::Signaled => formatter.write_str("terminated by signal"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProcessOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl ProcessOutput {
    #[must_use]
    const fn empty() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    #[must_use]
    pub(crate) fn stderr(&self) -> &[u8] {
        &self.stderr
    }
}

impl Debug for ProcessOutput {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessOutput")
            .field("stdout", &"<redacted>")
            .field("stderr", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerraformExecutionErrorKind {
    TemporaryPlan {
        message: String,
    },
    Launch {
        command: TerraformCommand,
        message: String,
    },
    Process {
        command: TerraformCommand,
        message: String,
    },
    NonZero {
        command: TerraformCommand,
        status: ProcessStatus,
        output: ProcessOutput,
    },
    Interrupted {
        command: TerraformCommand,
        output: ProcessOutput,
        kill_error: Option<String>,
    },
    InvalidPlan {
        source: PlanParseError,
    },
    Cleanup {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerraformExecutionError {
    kind: TerraformExecutionErrorKind,
    cleanup_error: Option<String>,
}

impl TerraformExecutionError {
    #[must_use]
    pub(crate) const fn kind(&self) -> &TerraformExecutionErrorKind {
        &self.kind
    }

    #[must_use]
    pub(crate) fn cleanup_error(&self) -> Option<&str> {
        self.cleanup_error.as_deref()
    }

    const fn new(kind: TerraformExecutionErrorKind) -> Self {
        Self {
            kind,
            cleanup_error: None,
        }
    }

    fn with_cleanup_error(mut self, error: &io::Error) -> Self {
        self.cleanup_error = Some(error.to_string());
        self
    }
}

impl Display for TerraformExecutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            TerraformExecutionErrorKind::TemporaryPlan { message } => {
                write!(
                    formatter,
                    "failed to create a temporary Terraform plan: {message}"
                )
            }
            TerraformExecutionErrorKind::Launch { command, message } => {
                write!(formatter, "failed to start terraform {command}: {message}")
            }
            TerraformExecutionErrorKind::Process { command, message } => {
                write!(
                    formatter,
                    "failed while running terraform {command}: {message}"
                )
            }
            TerraformExecutionErrorKind::NonZero {
                command, status, ..
            } => {
                write!(formatter, "terraform {command} failed with {status}")
            }
            TerraformExecutionErrorKind::Interrupted { command, .. } => {
                write!(formatter, "terraform {command} was interrupted")
            }
            TerraformExecutionErrorKind::InvalidPlan { source } => {
                write!(
                    formatter,
                    "terraform show output could not be parsed: {source}"
                )
            }
            TerraformExecutionErrorKind::Cleanup { message } => {
                write!(
                    formatter,
                    "failed to remove the temporary Terraform plan: {message}"
                )
            }
        }?;

        if let Some(error) = &self.cleanup_error {
            write!(formatter, "; plan cleanup also failed: {error}")?;
        }

        Ok(())
    }
}

impl std::error::Error for TerraformExecutionError {}

pub(crate) fn run_plan(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<PlanExecution, TerraformExecutionError> {
    let temporary_plan = TemporaryPlan::create().map_err(|error| {
        TerraformExecutionError::new(TerraformExecutionErrorKind::TemporaryPlan {
            message: error.to_string(),
        })
    })?;
    let result = execute_plan(
        root,
        &temporary_plan.path,
        cancellation,
        &SystemProcessRunner,
    );

    finish_plan(temporary_plan, result)
}

fn execute_plan(
    root: &Path,
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<PlanExecution, TerraformExecutionError> {
    let plan_arguments = plan_arguments(plan_path);
    let plan_output = run_command(
        root,
        TerraformCommand::Plan,
        &plan_arguments,
        cancellation,
        runner,
    )?;
    if plan_output.interrupted {
        return Err(interrupted_error(TerraformCommand::Plan, plan_output));
    }
    if !plan_output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(TerraformCommand::Plan, plan_output));
    }

    if cancellation.is_cancelled() {
        return Err(TerraformExecutionError::new(
            TerraformExecutionErrorKind::Interrupted {
                command: TerraformCommand::Show,
                output: ProcessOutput::empty(),
                kill_error: None,
            },
        ));
    }

    let show_arguments = show_arguments(plan_path);
    let show_output = run_command(
        root,
        TerraformCommand::Show,
        &show_arguments,
        cancellation,
        runner,
    )?;
    if show_output.interrupted {
        return Err(interrupted_error(TerraformCommand::Show, show_output));
    }
    if !show_output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(TerraformCommand::Show, show_output));
    }

    let json = show_output.output.stdout;
    let plan = parse_plan_json_bytes(&json).map_err(|source| {
        TerraformExecutionError::new(TerraformExecutionErrorKind::InvalidPlan { source })
    })?;

    Ok(PlanExecution { json, plan })
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

fn show_arguments(plan_path: &Path) -> Vec<OsString> {
    vec![
        OsString::from("show"),
        OsString::from("-json"),
        plan_path.as_os_str().to_owned(),
    ]
}

fn run_command(
    root: &Path,
    command: TerraformCommand,
    arguments: &[OsString],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<ProcessResult, TerraformExecutionError> {
    if cancellation.is_cancelled() {
        return Ok(ProcessResult::interrupted(ProcessOutput::empty(), None));
    }

    let mut process = runner.start(root, arguments).map_err(|error| {
        TerraformExecutionError::new(TerraformExecutionErrorKind::Launch {
            command,
            message: error.to_string(),
        })
    })?;

    loop {
        let status = process.try_wait().map_err(|error| {
            TerraformExecutionError::new(TerraformExecutionErrorKind::Process {
                command,
                message: error.to_string(),
            })
        })?;
        if let Some(status) = status {
            let output = process.collect_output().map_err(|error| {
                TerraformExecutionError::new(TerraformExecutionErrorKind::Process {
                    command,
                    message: error.to_string(),
                })
            })?;
            return Ok(ProcessResult {
                status: Some(status),
                output,
                interrupted: false,
                kill_error: None,
            });
        }

        if cancellation.is_cancelled() {
            let kill_error = process.kill().err().map(|error| error.to_string());
            let status = process.wait().map_err(|error| {
                TerraformExecutionError::new(TerraformExecutionErrorKind::Process {
                    command,
                    message: error.to_string(),
                })
            })?;
            let output = process.collect_output().map_err(|error| {
                TerraformExecutionError::new(TerraformExecutionErrorKind::Process {
                    command,
                    message: error.to_string(),
                })
            })?;
            return Ok(ProcessResult {
                status: Some(status),
                output,
                interrupted: true,
                kill_error,
            });
        }

        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn interrupted_error(command: TerraformCommand, process: ProcessResult) -> TerraformExecutionError {
    TerraformExecutionError::new(TerraformExecutionErrorKind::Interrupted {
        command,
        output: process.output,
        kill_error: process.kill_error,
    })
}

fn non_zero_error(command: TerraformCommand, process: ProcessResult) -> TerraformExecutionError {
    TerraformExecutionError::new(TerraformExecutionErrorKind::NonZero {
        command,
        status: process.status.unwrap_or(ProcessStatus::Signaled),
        output: process.output,
    })
}

struct ProcessResult {
    status: Option<ProcessStatus>,
    output: ProcessOutput,
    interrupted: bool,
    kill_error: Option<String>,
}

impl ProcessResult {
    const fn interrupted(output: ProcessOutput, kill_error: Option<String>) -> Self {
        Self {
            status: None,
            output,
            interrupted: true,
            kill_error,
        }
    }
}

trait ProcessRunner {
    fn start(&self, root: &Path, arguments: &[OsString]) -> io::Result<Box<dyn RunningProcess>>;
}

trait RunningProcess {
    fn try_wait(&mut self) -> io::Result<Option<ProcessStatus>>;
    fn kill(&mut self) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<ProcessStatus>;
    fn collect_output(self: Box<Self>) -> io::Result<ProcessOutput>;
}

struct SystemProcessRunner;

impl ProcessRunner for SystemProcessRunner {
    fn start(&self, root: &Path, arguments: &[OsString]) -> io::Result<Box<dyn RunningProcess>> {
        let mut command = Command::new(OsStr::new("terraform"));
        command
            .current_dir(root)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn()?;
        Ok(Box::new(SystemRunningProcess::new(child)))
    }
}

struct SystemRunningProcess {
    child: Child,
    stdout: Option<JoinHandle<io::Result<Vec<u8>>>>,
    stderr: Option<JoinHandle<io::Result<Vec<u8>>>>,
}

impl SystemRunningProcess {
    fn new(mut child: Child) -> Self {
        let stdout = child.stdout.take().map(spawn_reader);
        let stderr = child.stderr.take().map(spawn_reader);
        Self {
            child,
            stdout,
            stderr,
        }
    }
}

impl RunningProcess for SystemRunningProcess {
    fn try_wait(&mut self) -> io::Result<Option<ProcessStatus>> {
        self.child
            .try_wait()
            .map(|status| status.map(process_status))
    }

    fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    fn wait(&mut self) -> io::Result<ProcessStatus> {
        self.child.wait().map(process_status)
    }

    fn collect_output(mut self: Box<Self>) -> io::Result<ProcessOutput> {
        Ok(ProcessOutput {
            stdout: join_reader(self.stdout.take(), "stdout")?,
            stderr: join_reader(self.stderr.take(), "stderr")?,
        })
    }
}

impl Drop for SystemRunningProcess {
    fn drop(&mut self) {
        let running = self.child.try_wait().ok().flatten().is_none();
        if running {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.stdout.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr.take() {
            let _ = reader.join();
        }
    }
}

fn process_status(status: ExitStatus) -> ProcessStatus {
    if status.success() {
        ProcessStatus::Exited(0)
    } else {
        status
            .code()
            .map_or(ProcessStatus::Signaled, ProcessStatus::Exited)
    }
}

fn spawn_reader<R>(mut reader: R) -> JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn join_reader(
    reader: Option<JoinHandle<io::Result<Vec<u8>>>>,
    stream: &str,
) -> io::Result<Vec<u8>> {
    reader.map_or_else(
        || Ok(Vec::new()),
        |reader| {
            reader
                .join()
                .map_err(|_| io::Error::other(format!("terraform {stream} reader panicked")))?
        },
    )
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

    use super::*;

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
        fn try_wait(&mut self) -> io::Result<Option<ProcessStatus>> {
            match self.response.as_mut() {
                Some(FakeResponse::Exit { status, .. }) => Ok(Some(*status)),
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
                Some(FakeResponse::Exit { output, .. } | FakeResponse::Pending { output, .. }) => {
                    Ok(output)
                }
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
            output: ProcessOutput {
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
        }
    }

    fn show_process() -> FakeResponse {
        FakeResponse::Exit {
            status: ProcessStatus::Exited(0),
            output: ProcessOutput {
                stdout: PLAN_JSON.to_vec(),
                stderr: Vec::new(),
            },
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
            output: ProcessOutput {
                stdout: b"secret plan value".to_vec(),
                stderr: b"secret diagnostic".to_vec(),
            },
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
                output: ProcessOutput {
                    stdout: Vec::new(),
                    stderr: b"show failed".to_vec(),
                },
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
                output: ProcessOutput {
                    stdout: json!({"format_version": "2.0"}).to_string().into_bytes(),
                    stderr: Vec::new(),
                },
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
