use std::{
    env,
    ffi::{OsStr, OsString},
    fmt::{Debug, Display, Formatter},
    io::{self, Read},
    ops::Range,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::app::execution::{
    EventStream, ExecutionEvent, ExecutionEventKind, ExecutionLogLine, ProcessExitStatus,
    ProcessTermination,
};
use crate::infra::CancellationToken;

use super::{events::TerraformEventParser, line_buffer::LineBuffer, show::PlanParseError};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerraformCommand {
    Plan,
    Show,
    Apply,
    WorkspaceShow,
}

impl Display for TerraformCommand {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Plan => "plan",
            Self::Show => "show",
            Self::Apply => "apply",
            Self::WorkspaceShow => "workspace show",
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
    pub(super) const fn is_success(self) -> bool {
        matches!(self, Self::Exited(0))
    }

    #[must_use]
    pub(crate) const fn is_plan_success(self) -> bool {
        matches!(self, Self::Exited(0 | 2))
    }

    #[must_use]
    pub(crate) const fn code(self) -> Option<i32> {
        match self {
            Self::Exited(code) => Some(code),
            Self::Signaled => None,
        }
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
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    ordered: Vec<ProcessOutputRange>,
}

#[derive(Clone, PartialEq, Eq)]
struct ProcessOutputRange {
    stream: EventStream,
    range: Range<usize>,
}

impl ProcessOutput {
    #[must_use]
    pub(super) const fn empty() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            ordered: Vec::new(),
        }
    }

    fn append(&mut self, chunk: &ProcessOutputChunk) {
        let range = match chunk.stream {
            EventStream::Stdout => {
                let start = self.stdout.len();
                self.stdout.extend_from_slice(&chunk.bytes);
                start..self.stdout.len()
            }
            EventStream::Stderr => {
                let start = self.stderr.len();
                self.stderr.extend_from_slice(&chunk.bytes);
                start..self.stderr.len()
            }
        };
        self.ordered.push(ProcessOutputRange {
            stream: chunk.stream,
            range,
        });
    }

    #[must_use]
    pub(super) fn stdout(&self) -> &[u8] {
        &self.stdout
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
        output: Box<ProcessOutput>,
    },
    Interrupted {
        command: TerraformCommand,
        output: Box<ProcessOutput>,
        interrupt_error: Option<String>,
    },
    InvalidPlan {
        source: PlanParseError,
    },
    InvalidWorkspace {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerraformExecutionError {
    kind: TerraformExecutionErrorKind,
    cleanup_error: Option<String>,
}

impl TerraformExecutionError {
    pub(super) const fn new(kind: TerraformExecutionErrorKind) -> Self {
        Self {
            kind,
            cleanup_error: None,
        }
    }
}

impl Display for TerraformExecutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
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
            TerraformExecutionErrorKind::InvalidWorkspace { message } => {
                write!(
                    formatter,
                    "terraform workspace output could not be parsed: {message}"
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

pub(super) struct ProcessResult {
    pub(super) status: Option<ProcessStatus>,
    pub(super) output: ProcessOutput,
    pub(super) interrupted: bool,
    interrupt_error: Option<String>,
}

impl ProcessResult {
    const fn interrupted(output: ProcessOutput, interrupt_error: Option<String>) -> Self {
        Self {
            status: None,
            output,
            interrupted: true,
            interrupt_error,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProcessOutputChunk {
    pub(super) stream: EventStream,
    pub(super) bytes: Vec<u8>,
}

pub(crate) trait ProcessRunner {
    fn start(&self, root: &Path, arguments: &[OsString]) -> io::Result<Box<dyn RunningProcess>>;
}

pub(crate) trait RunningProcess {
    fn poll_output(&mut self) -> io::Result<Vec<ProcessOutputChunk>> {
        Ok(Vec::new())
    }

    fn try_wait(&mut self) -> io::Result<Option<ProcessStatus>>;
    fn request_interrupt(&mut self) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<ProcessStatus>;
    fn collect_output(self: Box<Self>) -> io::Result<ProcessOutput>;
}

pub(crate) struct SystemProcessRunner;

#[derive(Default)]
struct ObservedOutput {
    stdout: usize,
    stderr: usize,
    chunks: usize,
}

enum EventParser {
    Json(TerraformEventParser),
    Text(TextLineParser),
}

impl EventParser {
    fn push(
        &mut self,
        stream: EventStream,
        bytes: &[u8],
        received_at: Instant,
    ) -> Vec<ExecutionEvent> {
        match self {
            Self::Json(parser) => parser.push(stream, bytes, received_at),
            Self::Text(parser) => parser.push(stream, bytes, received_at),
        }
    }

    fn finish(&mut self, stream: EventStream, received_at: Instant) -> Vec<ExecutionEvent> {
        match self {
            Self::Json(parser) => parser.finish(stream, received_at),
            Self::Text(parser) => parser.finish(stream, received_at),
        }
    }
}

#[derive(Default)]
struct TextLineParser {
    stdout: LineBuffer,
    stderr: LineBuffer,
}

impl TextLineParser {
    fn push(
        &mut self,
        stream: EventStream,
        bytes: &[u8],
        received_at: Instant,
    ) -> Vec<ExecutionEvent> {
        let mut events = Vec::new();
        self.buffer_mut(stream).push(bytes, |line| {
            events.push(log_event(stream, line, received_at));
        });
        events
    }

    fn finish(&mut self, stream: EventStream, received_at: Instant) -> Vec<ExecutionEvent> {
        let line = self.buffer_mut(stream).finish();
        if line.is_empty() {
            Vec::new()
        } else {
            vec![log_event(stream, &line, received_at)]
        }
    }

    const fn buffer_mut(&mut self, stream: EventStream) -> &mut LineBuffer {
        match stream {
            EventStream::Stdout => &mut self.stdout,
            EventStream::Stderr => &mut self.stderr,
        }
    }
}

fn log_event(stream: EventStream, line: &[u8], received_at: Instant) -> ExecutionEvent {
    ExecutionEvent {
        received_at,
        kind: ExecutionEventKind::Log(ExecutionLogLine {
            stream,
            text: String::from_utf8_lossy(line).into_owned(),
        }),
    }
}

pub(crate) fn resolve_executable() -> io::Result<std::path::PathBuf> {
    let current = std::env::current_exe()?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(if cfg!(windows) {
            "terraform.exe"
        } else {
            "terraform"
        });
        if !is_executable(&candidate) {
            continue;
        }
        let candidate = if candidate.is_absolute() {
            candidate
        } else {
            std::env::current_dir()?.join(candidate)
        };
        if same_executable(&candidate, &current)? {
            return Err(io::Error::other("terraform resolves to Terracotta itself"));
        }
        return Ok(candidate);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "terraform was not found in PATH",
    ))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        true
    }
}

fn same_executable(candidate: &Path, current: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let candidate = candidate.metadata()?;
        let current = current.metadata()?;
        Ok(candidate.dev() == current.dev() && candidate.ino() == current.ino())
    }
    #[cfg(windows)]
    {
        use std::{fs::File, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };

        fn identity(path: &Path) -> io::Result<(u32, u32, u32)> {
            let file = File::open(path)?;
            let mut information = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: the handle stays open and information points to writable storage.
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok((
                information.dwVolumeSerialNumber,
                information.nFileIndexHigh,
                information.nFileIndexLow,
            ))
        }
        Ok(identity(candidate)? == identity(current)?)
    }
}

pub(crate) fn delegate(
    executable: &Path,
    arguments: &[OsString],
) -> io::Result<std::process::ExitCode> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec())
    }
    #[cfg(windows)]
    {
        let mut process = SystemRunningProcess::new(command.spawn()?);
        let status = process.child.wait()?;
        drop(process);
        exit_delegated_process(status)
    }
}

pub(crate) fn run_passthrough(
    executable: &Path,
    root: &Path,
    arguments: &[OsString],
) -> io::Result<ProcessStatus> {
    let mut command = Command::new(executable);
    command
        .current_dir(root)
        .args(arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    remove_cli_argument_environment(&mut command);
    #[cfg(unix)]
    {
        run_passthrough_unix(command)
    }
    #[cfg(windows)]
    {
        command.status().map(process_status)
    }
}

#[cfg(unix)]
fn run_passthrough_unix(mut command: Command) -> io::Result<ProcessStatus> {
    const extern "C" fn ignore_interrupt(_: libc::c_int) {}

    // SAFETY: the handler performs no work and is restored after the child exits.
    let previous = unsafe {
        libc::signal(
            libc::SIGINT,
            ignore_interrupt as *const () as libc::sighandler_t,
        )
    };
    if previous == libc::SIG_ERR {
        return Err(io::Error::last_os_error());
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            // SAFETY: restore the signal disposition captured immediately before spawning.
            unsafe { libc::signal(libc::SIGINT, previous) };
            return Err(error);
        }
    };
    let result = (|| {
        loop {
            if let Some(status) = child.try_wait()? {
                break Ok(process_status(status));
            }
            thread::sleep(PROCESS_POLL_INTERVAL);
        }
    })();
    // SAFETY: restore the signal disposition captured immediately before spawning the child.
    unsafe { libc::signal(libc::SIGINT, previous) };
    result
}

fn remove_cli_argument_environment(command: &mut Command) {
    for (name, _) in
        env::vars_os().filter(|(name, _)| name.to_string_lossy().starts_with("TF_CLI_ARGS"))
    {
        command.env_remove(name);
    }
}

#[cfg(windows)]
#[expect(
    clippy::exit,
    reason = "stable ExitCode only accepts u8; Windows delegation must preserve all 32 exit-status bits after reaping the child"
)]
fn exit_delegated_process(status: ExitStatus) -> ! {
    std::process::exit(status.code().unwrap_or(1));
}

pub(super) fn run_command(
    root: &Path,
    command: TerraformCommand,
    arguments: &[OsString],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<ProcessResult, TerraformExecutionError> {
    run_command_with_parser(root, command, arguments, cancellation, runner, None, false)
}

pub(super) fn run_command_with_events(
    root: &Path,
    command: TerraformCommand,
    arguments: &[OsString],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: Option<&mut dyn FnMut(ExecutionEvent)>,
) -> Result<ProcessResult, TerraformExecutionError> {
    run_command_with_parser(
        root,
        command,
        arguments,
        cancellation,
        runner,
        event_sink,
        false,
    )
}

pub(super) fn run_command_with_text_events(
    root: &Path,
    command: TerraformCommand,
    arguments: &[OsString],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: Option<&mut dyn FnMut(ExecutionEvent)>,
) -> Result<ProcessResult, TerraformExecutionError> {
    run_command_with_parser(
        root,
        command,
        arguments,
        cancellation,
        runner,
        event_sink,
        true,
    )
}

fn run_command_with_parser(
    root: &Path,
    command: TerraformCommand,
    arguments: &[OsString],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    mut event_sink: Option<&mut dyn FnMut(ExecutionEvent)>,
    text: bool,
) -> Result<ProcessResult, TerraformExecutionError> {
    let mut parser = event_sink.is_some().then(|| {
        if text {
            EventParser::Text(TextLineParser::default())
        } else {
            EventParser::Json(TerraformEventParser::new())
        }
    });
    let mut observed = ObservedOutput::default();
    if cancellation.is_cancelled() {
        if let Some(event_sink) = event_sink {
            emit_termination(event_sink, None, true);
        }
        return Ok(ProcessResult::interrupted(ProcessOutput::empty(), None));
    }

    let mut process = runner.start(root, arguments).map_err(|error| {
        TerraformExecutionError::new(TerraformExecutionErrorKind::Launch {
            command,
            message: error.to_string(),
        })
    })?;

    loop {
        let chunks = process.poll_output().map_err(|error| {
            TerraformExecutionError::new(TerraformExecutionErrorKind::Process {
                command,
                message: error.to_string(),
            })
        })?;
        if let (Some(parser), Some(event_sink)) = (parser.as_mut(), event_sink.as_deref_mut()) {
            emit_chunks(parser, &mut observed, chunks, event_sink);
        }

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
            if let (Some(parser), Some(event_sink)) = (parser.as_mut(), event_sink.as_deref_mut()) {
                emit_unobserved_output(parser, &mut observed, &output, event_sink);
                emit_parser_remainders(parser, event_sink);
                emit_termination(event_sink, Some(status), false);
            }
            return Ok(ProcessResult {
                status: Some(status),
                output,
                interrupted: false,
                interrupt_error: None,
            });
        }

        if cancellation.is_cancelled() {
            let interrupt_error = process
                .request_interrupt()
                .err()
                .map(|error| error.to_string());
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
            if let (Some(parser), Some(event_sink)) = (parser.as_mut(), event_sink.as_deref_mut()) {
                emit_unobserved_output(parser, &mut observed, &output, event_sink);
                emit_parser_remainders(parser, event_sink);
                emit_termination(event_sink, Some(status), true);
            }
            return Ok(ProcessResult {
                status: Some(status),
                output,
                interrupted: true,
                interrupt_error,
            });
        }

        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn emit_chunks(
    parser: &mut EventParser,
    observed: &mut ObservedOutput,
    chunks: Vec<ProcessOutputChunk>,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) {
    for chunk in chunks {
        let received_at = Instant::now();
        let length = chunk.bytes.len();
        let events = parser.push(chunk.stream, &chunk.bytes, received_at);
        match chunk.stream {
            EventStream::Stdout => observed.stdout += length,
            EventStream::Stderr => observed.stderr += length,
        }
        observed.chunks += 1;
        for event in events {
            event_sink(event);
        }
    }
}

fn emit_unobserved_output(
    parser: &mut EventParser,
    observed: &mut ObservedOutput,
    output: &ProcessOutput,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) {
    if !output.ordered.is_empty() {
        for record in output.ordered.iter().skip(observed.chunks) {
            let bytes = match record.stream {
                EventStream::Stdout => &output.stdout[record.range.clone()],
                EventStream::Stderr => &output.stderr[record.range.clone()],
            };
            for event in parser.push(record.stream, bytes, Instant::now()) {
                event_sink(event);
            }
        }
        observed.chunks = output.ordered.len();
        observed.stdout = output.stdout.len();
        observed.stderr = output.stderr.len();
        return;
    }
    emit_remaining_stream(
        parser,
        observed.stdout,
        EventStream::Stdout,
        &output.stdout,
        event_sink,
    );
    emit_remaining_stream(
        parser,
        observed.stderr,
        EventStream::Stderr,
        &output.stderr,
        event_sink,
    );
    observed.stdout = output.stdout.len();
    observed.stderr = output.stderr.len();
}

fn emit_remaining_stream(
    parser: &mut EventParser,
    observed: usize,
    stream: EventStream,
    output: &[u8],
    event_sink: &mut dyn FnMut(ExecutionEvent),
) {
    if let Some(remaining) = output.get(observed..) {
        let events = parser.push(stream, remaining, Instant::now());
        for event in events {
            event_sink(event);
        }
    }
}

fn emit_parser_remainders(parser: &mut EventParser, event_sink: &mut dyn FnMut(ExecutionEvent)) {
    for stream in [EventStream::Stdout, EventStream::Stderr] {
        for event in parser.finish(stream, Instant::now()) {
            event_sink(event);
        }
    }
}

fn emit_termination(
    event_sink: &mut dyn FnMut(ExecutionEvent),
    status: Option<ProcessStatus>,
    interrupted: bool,
) {
    let status = status.map_or(ProcessExitStatus::Signaled, |status| match status {
        ProcessStatus::Exited(code) => ProcessExitStatus::Exited(code),
        ProcessStatus::Signaled => ProcessExitStatus::Signaled,
    });
    event_sink(ExecutionEvent {
        received_at: Instant::now(),
        kind: ExecutionEventKind::Terminated(ProcessTermination {
            status,
            interrupted,
        }),
    });
}

pub(super) fn interrupted_error(
    command: TerraformCommand,
    process: ProcessResult,
) -> TerraformExecutionError {
    TerraformExecutionError::new(TerraformExecutionErrorKind::Interrupted {
        command,
        output: Box::new(process.output),
        interrupt_error: process.interrupt_error,
    })
}

pub(super) fn non_zero_error(
    command: TerraformCommand,
    process: ProcessResult,
) -> TerraformExecutionError {
    TerraformExecutionError::new(TerraformExecutionErrorKind::NonZero {
        command,
        status: process.status.unwrap_or(ProcessStatus::Signaled),
        output: Box::new(process.output),
    })
}

impl ProcessRunner for SystemProcessRunner {
    fn start(&self, root: &Path, arguments: &[OsString]) -> io::Result<Box<dyn RunningProcess>> {
        let mut command = Command::new(OsStr::new("terraform"));
        command
            .current_dir(root)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        remove_cli_argument_environment(&mut command);
        configure_process_group(&mut command);
        let child = command.spawn()?;
        Ok(Box::new(SystemRunningProcess::new(child)))
    }
}

struct SystemRunningProcess {
    child: Child,
    chunks: Receiver<io::Result<ProcessOutputChunk>>,
    readers: Vec<JoinHandle<io::Result<()>>>,
    output: ProcessOutput,
}

impl SystemRunningProcess {
    fn new(mut child: Child) -> Self {
        let (sender, chunks) = mpsc::channel();
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(spawn_reader(stdout, EventStream::Stdout, sender.clone()));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(spawn_reader(stderr, EventStream::Stderr, sender));
        }
        Self {
            child,
            chunks,
            readers,
            output: ProcessOutput::empty(),
        }
    }
}

impl RunningProcess for SystemRunningProcess {
    fn poll_output(&mut self) -> io::Result<Vec<ProcessOutputChunk>> {
        drain_output_chunks(&self.chunks, &mut self.output)
    }

    fn try_wait(&mut self) -> io::Result<Option<ProcessStatus>> {
        self.child
            .try_wait()
            .map(|status| status.map(process_status))
    }

    fn request_interrupt(&mut self) -> io::Result<()> {
        request_interrupt(&self.child)
    }

    fn wait(&mut self) -> io::Result<ProcessStatus> {
        self.child.wait().map(process_status)
    }

    fn collect_output(mut self: Box<Self>) -> io::Result<ProcessOutput> {
        let reader_result = join_readers(&mut self.readers);
        let chunk_result = drain_output_chunks(&self.chunks, &mut self.output);
        let output = std::mem::replace(&mut self.output, ProcessOutput::empty());
        reader_result.and(chunk_result).map(|_| output)
    }
}

impl Drop for SystemRunningProcess {
    fn drop(&mut self) {
        let running = self.child.try_wait().ok().flatten().is_none();
        if running {
            let _ = request_interrupt(&self.child);
            let _ = self.child.wait();
        }
        let _ = join_readers(&mut self.readers);
    }
}

#[cfg(unix)]
const fn configure_process_group(_command: &mut Command) {}

#[cfg(windows)]
fn configure_process_group(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
}

#[cfg(unix)]
fn request_interrupt(child: &Child) -> io::Result<()> {
    let pid = i32::try_from(child.id()).map_err(|_| io::Error::other("child PID is too large"))?;
    // SAFETY: kill is called with the live child PID and a valid signal constant.
    let result = unsafe { libc::kill(pid, libc::SIGINT) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn request_interrupt(child: &Child) -> io::Result<()> {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};

    // SAFETY: the child was created as its own process group and its PID is that group ID.
    let result = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
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

fn spawn_reader<R>(
    mut reader: R,
    stream: EventStream,
    sender: Sender<io::Result<ProcessOutputChunk>>,
) -> JoinHandle<io::Result<()>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0_u8; 8 * 1024];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(count) => count,
                Err(error) => {
                    let _ = sender.send(Err(io::Error::new(error.kind(), error.to_string())));
                    return Err(error);
                }
            };
            if count == 0 {
                return Ok(());
            }
            sender
                .send(Ok(ProcessOutputChunk {
                    stream,
                    bytes: buffer[..count].to_vec(),
                }))
                .map_err(|_| io::Error::other("Terraform output receiver was dropped"))?;
        }
    })
}

fn drain_output_chunks(
    receiver: &Receiver<io::Result<ProcessOutputChunk>>,
    output: &mut ProcessOutput,
) -> io::Result<Vec<ProcessOutputChunk>> {
    let mut chunks = Vec::new();
    loop {
        match receiver.try_recv() {
            Ok(Ok(chunk)) => {
                output.append(&chunk);
                chunks.push(chunk);
            }
            Ok(Err(error)) => return Err(error),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(chunks),
        }
    }
}

fn join_readers(readers: &mut Vec<JoinHandle<io::Result<()>>>) -> io::Result<()> {
    let mut first_error = None;
    for reader in readers.drain(..) {
        match reader.join() {
            Ok(Err(error)) if first_error.is_none() => first_error = Some(error),
            Err(_) if first_error.is_none() => {
                first_error = Some(io::Error::other("Terraform output reader panicked"));
            }
            Ok(Ok(()) | Err(_)) | Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

// Shared by sibling Terraform tests because the represented fields stay private
// to this implementation module in production.
#[cfg(test)]
mod tests {
    use super::*;

    impl ProcessOutput {
        pub(crate) const fn new(stdout: Vec<u8>, stderr: Vec<u8>) -> Self {
            Self {
                stdout,
                stderr,
                ordered: Vec::new(),
            }
        }
    }

    impl TerraformExecutionError {
        pub(crate) fn with_cleanup_error(mut self, error: &io::Error) -> Self {
            self.cleanup_error = Some(error.to_string());
            self
        }

        pub(crate) const fn kind(&self) -> &TerraformExecutionErrorKind {
            &self.kind
        }

        pub(crate) fn cleanup_error(&self) -> Option<&str> {
            self.cleanup_error.as_deref()
        }
    }

    fn log_text(event: &ExecutionEvent) -> Option<(EventStream, &str)> {
        let ExecutionEventKind::Log(line) = &event.kind else {
            return None;
        };
        Some((line.stream, line.text.as_str()))
    }

    #[test]
    fn text_parser_preserves_interleaved_stream_order_and_split_utf8() {
        let now = Instant::now();
        let mut parser = TextLineParser::default();
        let message = "初期化しました\n".as_bytes();
        let split = "初".len() - 1;

        assert!(
            parser
                .push(EventStream::Stdout, &message[..split], now)
                .is_empty()
        );
        let stderr = parser.push(EventStream::Stderr, b"warning\n", now);
        let stdout = parser.push(EventStream::Stdout, &message[split..], now);

        assert_eq!(log_text(&stderr[0]), Some((EventStream::Stderr, "warning")));
        assert_eq!(
            log_text(&stdout[0]),
            Some((EventStream::Stdout, "初期化しました"))
        );
    }

    #[test]
    fn text_parser_preserves_chunked_long_lines_and_empty_lines() {
        let now = Instant::now();
        let mut parser = TextLineParser::default();
        let long_line = "x".repeat(100_000);
        let input = format!("{long_line}\r\n\n続き\n未完了").into_bytes();
        let mut events = Vec::new();

        for chunk in input.chunks(8 * 1024) {
            events.extend(parser.push(EventStream::Stdout, chunk, now));
        }
        events.extend(parser.finish(EventStream::Stdout, now));

        let lines = events.iter().filter_map(log_text).map(|(_, text)| text);
        assert_eq!(
            lines.collect::<Vec<_>>(),
            vec![long_line.as_str(), "", "続き", "未完了"]
        );
    }

    #[test]
    fn text_parser_flushes_a_final_line_without_newline() {
        let now = Instant::now();
        let mut parser = TextLineParser::default();

        assert!(
            parser
                .push(EventStream::Stdout, b"final line", now)
                .is_empty()
        );
        let remainder = parser.finish(EventStream::Stdout, now);

        assert_eq!(
            log_text(&remainder[0]),
            Some((EventStream::Stdout, "final line"))
        );
    }

    #[test]
    fn unobserved_output_replays_remaining_ranges_in_receive_order() {
        let message = "初期化\n".as_bytes();
        let split = "初".len() - 1;
        let mut chunks = vec![
            ProcessOutputChunk {
                stream: EventStream::Stdout,
                bytes: message[..split].to_vec(),
            },
            ProcessOutputChunk {
                stream: EventStream::Stderr,
                bytes: b"warning\n".to_vec(),
            },
            ProcessOutputChunk {
                stream: EventStream::Stdout,
                bytes: message[split..].to_vec(),
            },
            ProcessOutputChunk {
                stream: EventStream::Stdout,
                bytes: b"final line".to_vec(),
            },
        ];
        let mut output = ProcessOutput::empty();
        for chunk in &chunks {
            output.append(chunk);
        }

        let mut parser = EventParser::Text(TextLineParser::default());
        let mut observed = ObservedOutput::default();
        let mut events = Vec::new();
        emit_chunks(
            &mut parser,
            &mut observed,
            vec![chunks.remove(0)],
            &mut |event| events.push(event),
        );

        emit_unobserved_output(&mut parser, &mut observed, &output, &mut |event| {
            events.push(event);
        });
        emit_parser_remainders(&mut parser, &mut |event| events.push(event));

        assert_eq!(output.stdout(), [message, b"final line"].concat());
        assert_eq!(output.stderr, b"warning\n");
        assert_eq!(observed.stdout, output.stdout().len());
        assert_eq!(observed.stderr, output.stderr.len());
        assert_eq!(observed.chunks, 4);
        assert_eq!(
            events.iter().filter_map(log_text).collect::<Vec<_>>(),
            [
                (EventStream::Stderr, "warning"),
                (EventStream::Stdout, "初期化"),
                (EventStream::Stdout, "final line"),
            ]
        );
    }
}
