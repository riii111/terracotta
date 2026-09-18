use std::{
    ffi::{OsStr, OsString},
    fmt::{Debug, Display, Formatter},
    io::{self, Read},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::app::execution::{
    EventStream, ExecutionEvent, ExecutionEventKind, ProcessExitStatus, ProcessTermination,
};
use crate::infra::CancellationToken;

use super::{events::TerraformEventParser, show::PlanParseError};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerraformCommand {
    Plan,
    Show,
    WorkspaceShow,
}

impl Display for TerraformCommand {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Plan => "plan",
            Self::Show => "show",
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
    stderr: Vec<u8>,
}

impl ProcessOutput {
    #[must_use]
    pub(super) const fn empty() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    fn append(&mut self, chunk: &ProcessOutputChunk) {
        match chunk.stream {
            EventStream::Stdout => self.stdout.extend_from_slice(&chunk.bytes),
            EventStream::Stderr => self.stderr.extend_from_slice(&chunk.bytes),
        }
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
    InvalidWorkspace {
        message: String,
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
    pub(super) const fn new(kind: TerraformExecutionErrorKind) -> Self {
        Self {
            kind,
            cleanup_error: None,
        }
    }

    pub(super) fn with_cleanup_error(mut self, error: &io::Error) -> Self {
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
            TerraformExecutionErrorKind::InvalidWorkspace { message } => {
                write!(
                    formatter,
                    "terraform workspace output could not be parsed: {message}"
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

pub(super) struct ProcessResult {
    pub(super) status: Option<ProcessStatus>,
    pub(super) output: ProcessOutput,
    pub(super) interrupted: bool,
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
    fn kill(&mut self) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<ProcessStatus>;
    fn collect_output(self: Box<Self>) -> io::Result<ProcessOutput>;
}

pub(crate) struct SystemProcessRunner;

#[derive(Default)]
struct ObservedOutput {
    stdout: usize,
    stderr: usize,
}

pub(super) fn run_command(
    root: &Path,
    command: TerraformCommand,
    arguments: &[OsString],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<ProcessResult, TerraformExecutionError> {
    run_command_with_events(root, command, arguments, cancellation, runner, None)
}

pub(super) fn run_command_with_events(
    root: &Path,
    command: TerraformCommand,
    arguments: &[OsString],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    mut event_sink: Option<&mut dyn FnMut(ExecutionEvent)>,
) -> Result<ProcessResult, TerraformExecutionError> {
    let mut parser = event_sink.is_some().then(TerraformEventParser::new);
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
            if let (Some(parser), Some(event_sink)) = (parser.as_mut(), event_sink.as_deref_mut()) {
                emit_unobserved_output(parser, &mut observed, &output, event_sink);
                emit_parser_remainders(parser, event_sink);
                emit_termination(event_sink, Some(status), true);
            }
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

fn emit_chunks(
    parser: &mut TerraformEventParser,
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
        for event in events {
            event_sink(event);
        }
    }
}

fn emit_unobserved_output(
    parser: &mut TerraformEventParser,
    observed: &mut ObservedOutput,
    output: &ProcessOutput,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) {
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
    parser: &mut TerraformEventParser,
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

fn emit_parser_remainders(
    parser: &mut TerraformEventParser,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) {
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
        output: process.output,
        kill_error: process.kill_error,
    })
}

pub(super) fn non_zero_error(
    command: TerraformCommand,
    process: ProcessResult,
) -> TerraformExecutionError {
    TerraformExecutionError::new(TerraformExecutionErrorKind::NonZero {
        command,
        status: process.status.unwrap_or(ProcessStatus::Signaled),
        output: process.output,
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

    fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
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
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = join_readers(&mut self.readers);
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

#[cfg(test)]
mod tests {
    use super::*;

    impl ProcessOutput {
        pub(crate) const fn new(stdout: Vec<u8>, stderr: Vec<u8>) -> Self {
            Self { stdout, stderr }
        }

        pub(crate) fn stdout(&self) -> &[u8] {
            &self.stdout
        }

        pub(crate) fn stderr(&self) -> &[u8] {
            &self.stderr
        }
    }

    impl TerraformExecutionError {
        pub(crate) const fn kind(&self) -> &TerraformExecutionErrorKind {
            &self.kind
        }

        pub(crate) fn cleanup_error(&self) -> Option<&str> {
            self.cleanup_error.as_deref()
        }
    }
}
