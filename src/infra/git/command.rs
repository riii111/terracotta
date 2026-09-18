use std::{
    ffi::OsStr,
    io::{self, Read},
    path::Path,
    process::{Child, Command, ExitStatus, Output, Stdio},
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::infra::CancellationToken;

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub(crate) struct GitCommandError {
    pub(super) operation: String,
    pub(super) message: String,
    interrupted: bool,
}

impl GitCommandError {
    fn from_spawn(operation: &str, error: &io::Error) -> Self {
        Self::normal(operation, error.to_string())
    }

    fn from_process(operation: &str, error: &io::Error) -> Self {
        Self::normal(
            operation,
            format!("failed while reading Git output: {error}"),
        )
    }

    pub(super) fn from_output(operation: &str, output: &Output) -> Self {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Self::normal(
            operation,
            if message.is_empty() {
                format!("git exited with status {}", output.status)
            } else {
                message
            },
        )
    }

    pub(super) fn interrupted(operation: &str) -> Self {
        Self {
            operation: operation.to_owned(),
            message: "Git command was interrupted".to_owned(),
            interrupted: true,
        }
    }

    fn normal(operation: &str, message: String) -> Self {
        Self {
            operation: operation.to_owned(),
            message,
            interrupted: false,
        }
    }

    pub(crate) const fn is_interrupted(&self) -> bool {
        self.interrupted
    }
}

pub(super) fn checked_git<I, S>(
    directory: &Path,
    operation: &str,
    args: I,
    cancellation: &CancellationToken,
) -> Result<Output, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git(directory, operation, args, cancellation)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitCommandError::from_output(operation, &output))
    }
}

pub(super) fn run_git<I, S>(
    directory: &Path,
    operation: &str,
    args: I,
    cancellation: &CancellationToken,
) -> Result<Output, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_git_with_env(directory, operation, args, &[], cancellation)
}

pub(super) fn run_git_with_env<I, S>(
    directory: &Path,
    operation: &str,
    args: I,
    environment: &[(&str, &str)],
    cancellation: &CancellationToken,
) -> Result<Output, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    if cancellation.is_cancelled() {
        return Err(GitCommandError::interrupted(operation));
    }

    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in environment {
        command.env(key, value);
    }

    let mut process = GitProcessGuard::spawn(command)
        .map_err(|error| GitCommandError::from_spawn(operation, &error))?;
    loop {
        process
            .drain_output()
            .map_err(|error| GitCommandError::from_process(operation, &error))?;
        let status = process
            .try_wait()
            .map_err(|error| GitCommandError::from_process(operation, &error))?;
        if let Some(status) = status {
            let output = process
                .collect_output(status)
                .map_err(|error| GitCommandError::from_process(operation, &error))?;
            if cancellation.is_cancelled() {
                return Err(GitCommandError::interrupted(operation));
            }
            return Ok(output);
        }

        if cancellation.is_cancelled() {
            if let Ok(status) = process.stop() {
                let _ = process.collect_output(status);
            }
            return Err(GitCommandError::interrupted(operation));
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

pub(super) fn parse_error(operation: &str, message: &str) -> GitCommandError {
    GitCommandError::normal(operation, message.to_owned())
}

pub(super) fn nul_fields(output: &[u8], operation: &str) -> Result<Vec<String>, GitCommandError> {
    output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| {
            String::from_utf8(field.to_owned()).map_err(|error| {
                parse_error(operation, &format!("Git path is not valid UTF-8: {error}"))
            })
        })
        .collect()
}

struct GitProcessGuard {
    child: Child,
    chunks: Receiver<io::Result<GitOutputChunk>>,
    readers: Vec<JoinHandle<io::Result<()>>>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl GitProcessGuard {
    fn spawn(mut command: Command) -> io::Result<Self> {
        let mut child = command.spawn()?;
        let (sender, chunks) = mpsc::channel();
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(spawn_reader(stdout, GitStream::Stdout, sender.clone()));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(spawn_reader(stderr, GitStream::Stderr, sender.clone()));
        }
        drop(sender);
        Ok(Self {
            child,
            chunks,
            readers,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    fn drain_output(&mut self) -> io::Result<()> {
        loop {
            match self.chunks.try_recv() {
                Ok(Ok(chunk)) => self.append(&chunk),
                Ok(Err(error)) => return Err(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(()),
            }
        }
    }

    fn append(&mut self, chunk: &GitOutputChunk) {
        match chunk.stream {
            GitStream::Stdout => self.stdout.extend_from_slice(&chunk.bytes),
            GitStream::Stderr => self.stderr.extend_from_slice(&chunk.bytes),
        }
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    fn stop(&mut self) -> io::Result<ExitStatus> {
        if self.child.try_wait()?.is_none() {
            let _ = self.child.kill();
        }
        self.child.wait()
    }

    fn collect_output(&mut self, status: ExitStatus) -> io::Result<Output> {
        let reader_result = join_readers(&mut self.readers);
        let chunk_result = self.drain_output();
        reader_result.and(chunk_result)?;
        Ok(Output {
            status,
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
        })
    }
}

impl Drop for GitProcessGuard {
    fn drop(&mut self) {
        let running = self
            .child
            .try_wait()
            .map_or(true, |status| status.is_none());
        if running {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = join_readers(&mut self.readers);
    }
}

#[derive(Clone, Copy)]
enum GitStream {
    Stdout,
    Stderr,
}

struct GitOutputChunk {
    stream: GitStream,
    bytes: Vec<u8>,
}

fn spawn_reader<R>(
    mut reader: R,
    stream: GitStream,
    sender: Sender<io::Result<GitOutputChunk>>,
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
                .send(Ok(GitOutputChunk {
                    stream,
                    bytes: buffer[..count].to_vec(),
                }))
                .map_err(|_| io::Error::other("Git output receiver was dropped"))?;
        }
    })
}

fn join_readers(readers: &mut Vec<JoinHandle<io::Result<()>>>) -> io::Result<()> {
    let mut first_error = None;
    for reader in readers.drain(..) {
        match reader.join() {
            Ok(Err(error)) if first_error.is_none() => first_error = Some(error),
            Err(_) if first_error.is_none() => {
                first_error = Some(io::Error::other("Git output reader panicked"));
            }
            Ok(Ok(()) | Err(_)) | Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos();
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "terracotta-git-command-{}-{suffix}-{sequence}",
                std::process::id(),
            ));
            fs::create_dir(&path).expect("test directory should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn install_fake_git(&self) {
            let path = self.path().join("git");
            fs::write(
                &path,
                r#"#!/bin/sh
if [ "$MODE" = "pre-cancel" ]; then
  printf '%s\n' "$$" > "$PID_FILE"
  exit 0
fi
if [ "$MODE" = "interrupt" ]; then
  printf '%s\n' "$$" > "$PID_FILE"
  while :; do :; done
fi
if [ "$MODE" = "interrupt-stream" ]; then
  printf '%s\n' "$$" > "$PID_FILE"
  i=0
  while [ "$i" -lt 20000 ]; do
    printf 'stdout-output-0123456789\n'
    printf 'stderr-output-0123456789\n' >&2
    i=$((i + 1))
  done
  : > "$READY_FILE"
  while :; do :; done
fi
if [ "$MODE" = "race" ]; then
  printf '%s\n' "$$" > "$PID_FILE"
  (
    while [ ! -f "$RELEASE_FILE" ]; do
      sleep 0.01
    done
    : > "$HOLDER_DONE_FILE"
  ) &
  printf 'race-output\n'
  : > "$READY_FILE"
  exit 0
fi
if [ "$MODE" = "stream" ]; then
  i=0
  while [ "$i" -lt 20000 ]; do
    printf 'stdout-output-0123456789\n'
    printf 'stderr-output-0123456789\n' >&2
    i=$((i + 1))
  done
  exit 7
fi
exit 0
"#,
            )
            .expect("fake Git should be written");
            let mut permissions = fs::metadata(&path)
                .expect("fake Git metadata should be available")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("fake Git should be executable");
        }

        fn path_environment(&self) -> String {
            let original = std::env::var("PATH").expect("PATH should be available");
            format!("{}:{original}", self.path().display())
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("test directory should be removed");
        }
    }

    #[test]
    fn does_not_spawn_git_after_cancellation() {
        let directory = TestDirectory::new();
        directory.install_fake_git();
        let pid_file = directory.path().join("pid");
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let error = run_git_with_env(
            directory.path(),
            "test Git command",
            std::iter::empty::<&str>(),
            &[
                ("PATH", directory.path_environment().as_str()),
                ("MODE", "pre-cancel"),
                (
                    "PID_FILE",
                    pid_file.to_str().expect("pid path should be UTF-8"),
                ),
            ],
            &cancellation,
        )
        .expect_err("cancelled Git should not start");

        assert!(error.is_interrupted());
        assert!(!pid_file.exists());
    }

    #[test]
    fn drains_both_pipes_before_returning_git_output() {
        let directory = TestDirectory::new();
        directory.install_fake_git();
        let cancellation = CancellationToken::new();

        let output = run_git_with_env(
            directory.path(),
            "stream Git output",
            std::iter::empty::<&str>(),
            &[
                ("PATH", directory.path_environment().as_str()),
                ("MODE", "stream"),
                ("PID_FILE", "unused"),
            ],
            &cancellation,
        )
        .expect("Git output should be collected");

        assert_eq!(output.status.code(), Some(7));
        assert!(output.stdout.len() > 64 * 1024);
        assert!(output.stderr.len() > 64 * 1024);
    }

    #[test]
    fn kills_and_reaps_git_when_cancelled_while_running() {
        let directory = TestDirectory::new();
        directory.install_fake_git();
        let pid_file = directory.path().join("pid");
        let cancellation = CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let path = directory.path().to_owned();
        let path_environment = directory.path_environment();
        let pid_path = pid_file
            .to_str()
            .expect("pid path should be UTF-8")
            .to_owned();
        let worker = thread::spawn(move || {
            run_git_with_env(
                &path,
                "interruptible Git command",
                std::iter::empty::<&str>(),
                &[
                    ("PATH", path_environment.as_str()),
                    ("MODE", "interrupt"),
                    ("PID_FILE", pid_path.as_str()),
                ],
                &worker_cancellation,
            )
        });
        for _ in 0..100 {
            if pid_file.is_file() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(pid_file.is_file(), "fake Git should have started");
        cancellation.cancel();

        let error = worker
            .join()
            .expect("Git worker should join")
            .expect_err("cancelled Git should return interruption");
        assert!(error.is_interrupted());
        let pid = fs::read_to_string(pid_file)
            .expect("Git pid should be recorded")
            .trim()
            .parse::<u32>()
            .expect("Git pid should be numeric");
        assert!(
            !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .expect("kill should start")
                .success()
        );
    }

    #[test]
    fn kills_and_reaps_git_after_both_pipes_fill_when_cancelled() {
        let directory = TestDirectory::new();
        directory.install_fake_git();
        let pid_file = directory.path().join("pid");
        let ready_file = directory.path().join("ready");
        let cancellation = CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let path = directory.path().to_owned();
        let path_environment = directory.path_environment();
        let pid_path = pid_file
            .to_str()
            .expect("pid path should be UTF-8")
            .to_owned();
        let ready_path = ready_file
            .to_str()
            .expect("ready path should be UTF-8")
            .to_owned();
        let worker = thread::spawn(move || {
            run_git_with_env(
                &path,
                "interruptible Git stream",
                std::iter::empty::<&str>(),
                &[
                    ("PATH", path_environment.as_str()),
                    ("MODE", "interrupt-stream"),
                    ("PID_FILE", pid_path.as_str()),
                    ("READY_FILE", ready_path.as_str()),
                ],
                &worker_cancellation,
            )
        });
        for _ in 0..100 {
            if ready_file.is_file() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            ready_file.is_file(),
            "fake Git should fill both pipes before waiting"
        );
        cancellation.cancel();

        let error = worker
            .join()
            .expect("Git worker should join")
            .expect_err("cancelled Git stream should return interruption");
        assert!(error.is_interrupted());
        let pid = fs::read_to_string(pid_file)
            .expect("Git pid should be recorded")
            .trim()
            .parse::<u32>()
            .expect("Git pid should be numeric");
        assert!(
            !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .expect("kill should start")
                .success(),
            "Git process {pid} remains alive"
        );
    }

    #[test]
    fn cancellation_wins_after_git_exit_before_output_readers_finish() {
        let directory = TestDirectory::new();
        directory.install_fake_git();
        let pid_file = directory.path().join("pid");
        let ready_file = directory.path().join("ready");
        let release_file = directory.path().join("release");
        let holder_done_file = directory.path().join("holder-done");
        let cancellation = CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let path = directory.path().to_owned();
        let path_environment = directory.path_environment();
        let pid_path = pid_file
            .to_str()
            .expect("pid path should be UTF-8")
            .to_owned();
        let ready_path = ready_file
            .to_str()
            .expect("ready path should be UTF-8")
            .to_owned();
        let release_path = release_file
            .to_str()
            .expect("release path should be UTF-8")
            .to_owned();
        let holder_done_path = holder_done_file
            .to_str()
            .expect("holder-done path should be UTF-8")
            .to_owned();
        let worker = thread::spawn(move || {
            run_git_with_env(
                &path,
                "racing Git command",
                std::iter::empty::<&str>(),
                &[
                    ("PATH", path_environment.as_str()),
                    ("MODE", "race"),
                    ("PID_FILE", pid_path.as_str()),
                    ("READY_FILE", ready_path.as_str()),
                    ("RELEASE_FILE", release_path.as_str()),
                    ("HOLDER_DONE_FILE", holder_done_path.as_str()),
                ],
                &worker_cancellation,
            )
        });
        for _ in 0..100 {
            if ready_file.is_file() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ready_file.is_file(), "fake Git should signal before exit");
        for _ in 0..100 {
            let alive = Command::new("kill")
                .args([
                    "-0",
                    fs::read_to_string(&pid_file)
                        .expect("Git pid should be recorded")
                        .trim(),
                ])
                .stderr(Stdio::null())
                .status()
                .expect("kill should start")
                .success();
            if !alive {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let pid = fs::read_to_string(&pid_file)
            .expect("Git pid should be recorded")
            .trim()
            .parse::<u32>()
            .expect("Git pid should be numeric");
        assert!(
            !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .expect("kill should start")
                .success(),
            "Git process {pid} should exit before cancellation"
        );
        cancellation.cancel();
        fs::write(&release_file, "release").expect("output holder should be released");

        let error = worker
            .join()
            .expect("Git worker should join")
            .expect_err("cancellation should win the normal-exit race");
        assert!(error.is_interrupted());
        assert!(
            holder_done_file.is_file(),
            "output reader holder should be reaped"
        );
    }
}
