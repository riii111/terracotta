use std::process::Command;

use rstest::rstest;

#[rstest]
#[case::help("--help")]
#[case::version("--version")]
fn help_and_version_work_without_a_terminal_or_terraform(#[case] arg: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_terracotta"))
        .env("PATH", "")
        .arg(arg)
        .output()
        .expect("CLI should start");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(
        !output.stdout.contains(&0x1b),
        "CLI must not initialize a TUI"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("terracotta"));
}

#[test]
fn plan_explains_non_tty_use_before_starting_terraform() {
    let output = Command::new(env!("CARGO_BIN_EXE_terracotta"))
        .env("PATH", "")
        .arg("plan")
        .output()
        .expect("CLI should start");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal"));
}

#[test]
fn plan_argument_errors_follow_clap_without_initializing_a_tui() {
    let output = Command::new(env!("CARGO_BIN_EXE_terracotta"))
        .env("PATH", "")
        .args(["plan", "--compare-ref"])
        .output()
        .expect("CLI should start");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    assert!(!output.stderr.contains(&0x1b));
}

#[cfg(all(unix, feature = "test-support"))]
mod pty_tests {
    use std::{
        env, fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    const PLAN_JSON: &str = r#"{
  "format_version": "1.0",
  "resource_changes": [{
    "address": "terraform_data.api",
    "mode": "managed",
    "change": {
      "actions": ["update"],
      "before": {"input": "old", "nested": {"stable": true}},
      "after": {"input": "new", "nested": {"stable": false}}
    }
  }]
}"#;

    const EMPTY_PLAN_JSON: &str = r#"{"format_version":"1.0"}"#;

    const FAKE_TERRAFORM: &str = r#"#!/bin/sh
set -eu

case "$1" in
  workspace)
    printf 'default\n'
    ;;
  plan)
    plan_path=''
    for argument in "$@"; do
      case "$argument" in
        -out=*) plan_path=${argument#-out=} ;;
      esac
    done
    printf '%s\n' "$plan_path" > "$TERRACOTTA_FAKE_PLAN_PATH"
    printf '%s\n' "$$" > "$TERRACOTTA_FAKE_PID_PATH"
    : > "$plan_path"
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = interrupt ]; then
      exec /bin/sleep 30
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = failure ]; then
      printf '%s\n' '{"type":"diagnostic","diagnostic":{"severity":"error","summary":"synthetic plan failure","detail":"fake Terraform failed"}}'
      exit 1
    fi
    printf '%s\n' '{"type":"planned_change","change":{"resource":{"addr":"terraform_data.api"}}}'
    ;;
  show)
    cat "$TERRACOTTA_FAKE_SHOW_JSON"
    ;;
  *)
    exit 2
    ;;
esac
"#;

    const PTY_DRIVER: &str = r#"
import os
import pty
import select
import signal
import fcntl
import struct
import sys
import termios
import time

_, binary, root, columns_arg, rows_arg, scenario = sys.argv[:6]
columns = int(columns_arg)
rows = int(rows_arg)
command = sys.argv[6:]
pid, fd = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.execv(
        "/bin/sh",
        [
            "sh",
            "-c",
            'stty rows "$1" cols "$2"; cd "$3"; shift 3; exec "$@"',
            "sh",
            rows_arg,
            columns_arg,
            root,
            binary,
            *command,
        ],
    )

output = bytearray()
observed = []


class Screen:
    def __init__(self, columns, rows):
        self.columns = columns
        self.rows = rows
        self.cells = [[" "] * columns for _ in range(rows)]
        self.row = 0
        self.column = 0
        self.saved = (0, 0)
        self.pending = bytearray()

    def resize(self, columns, rows):
        cells = [[" "] * columns for _ in range(rows)]
        for row in range(min(self.rows, rows)):
            for column in range(min(self.columns, columns)):
                cells[row][column] = self.cells[row][column]
        self.columns = columns
        self.rows = rows
        self.cells = cells
        self.row = min(self.row, rows - 1)
        self.column = min(self.column, columns - 1)

    def feed(self, data):
        self.pending.extend(data)
        while self.pending:
            if self.pending[0] == 0x1b:
                if len(self.pending) < 2:
                    return
                if self.pending[1] == ord("["):
                    final = next(
                        (index for index, value in enumerate(self.pending[2:], 2)
                         if 0x40 <= value <= 0x7e),
                        None,
                    )
                    if final is None:
                        return
                    sequence = bytes(self.pending[2:final])
                    command = chr(self.pending[final])
                    del self.pending[:final + 1]
                    self.csi(sequence.decode("ascii", "ignore"), command)
                    continue
                del self.pending[:2]
                continue

            value = self.pending[0]
            if value == 0x0d:
                del self.pending[:1]
                self.column = 0
                continue
            if value == 0x0a:
                del self.pending[:1]
                self.row = min(self.row + 1, self.rows - 1)
                continue
            if value == 0x08:
                del self.pending[:1]
                self.column = max(self.column - 1, 0)
                continue
            if value < 0x20 or value == 0x7f:
                del self.pending[:1]
                continue

            character = None
            for length in range(1, min(4, len(self.pending)) + 1):
                try:
                    character = bytes(self.pending[:length]).decode("utf-8")
                    break
                except UnicodeDecodeError as error:
                    if error.reason == "unexpected end of data" and length == len(self.pending):
                        return
            if character is None:
                character = "�"
                length = 1
            del self.pending[:length]
            self.put(character)

    def put(self, character):
        if self.row >= self.rows:
            return
        if self.column >= self.columns:
            self.column = 0
            self.row = min(self.row + 1, self.rows - 1)
        self.cells[self.row][self.column] = character
        self.column = min(self.column + 1, self.columns)

    def csi(self, parameters, command):
        private = parameters.startswith("?")
        if private:
            parameters = parameters[1:]
        values = []
        for value in parameters.split(";") if parameters else []:
            try:
                values.append(int(value) if value else 1)
            except ValueError:
                values.append(1)

        if command in ("H", "f"):
            self.row = max((values[0] if values else 1) - 1, 0)
            self.column = max((values[1] if len(values) > 1 else 1) - 1, 0)
        elif command == "G":
            self.column = max((values[0] if values else 1) - 1, 0)
        elif command == "d":
            self.row = max((values[0] if values else 1) - 1, 0)
        elif command == "A":
            self.row = max(self.row - (values[0] if values else 1), 0)
        elif command == "B":
            self.row = min(self.row + (values[0] if values else 1), self.rows - 1)
        elif command == "C":
            self.column = min(self.column + (values[0] if values else 1), self.columns)
        elif command == "D":
            self.column = max(self.column - (values[0] if values else 1), 0)
        elif command == "J":
            if not values or values[0] == 2:
                self.cells = [[" "] * self.columns for _ in range(self.rows)]
        elif command == "K":
            mode = values[0] if values else 0
            start = 0 if mode == 2 else self.column if mode == 0 else 0
            end = self.columns if mode in (0, 2) else self.column + 1
            for column in range(start, min(end, self.columns)):
                self.cells[self.row][column] = " "
        elif command == "s":
            self.saved = (self.row, self.column)
        elif command == "u":
            self.row, self.column = self.saved

    def text(self):
        return "\n".join("".join(row) for row in self.cells)


screen = Screen(columns, rows)


def child_status():
    waited, status = os.waitpid(pid, os.WNOHANG)
    if waited == 0:
        return None
    return os.waitstatus_to_exitcode(status)


def read_available():
    ready, _, _ = select.select([fd], [], [], 0.1)
    if not ready:
        return
    try:
        chunk = os.read(fd, 8192)
        output.extend(chunk)
        screen.feed(chunk)
    except OSError:
        pass


def wait_screen(predicate, name, description, timeout=20):
    before = screen.text()
    deadline = time.time() + timeout
    while time.time() < deadline:
        current = screen.text()
        if current != before and predicate(current):
            observed.append(name)
            return
        read_available()
        current = screen.text()
        if current != before and predicate(current):
            observed.append(name)
            return
        if child_status() is not None:
            break
    raise RuntimeError(
        f"missing {name}: {description!r}; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


def wait_new(marker, name, timeout=20):
    wait_screen(lambda current: marker in current, name, marker, timeout)


def wait_parts(markers, name, timeout=20):
    wait_screen(lambda current: all(marker in current for marker in markers), name, markers, timeout)


def wait_any(markers, name, timeout=20):
    wait_screen(lambda current: any(marker in current for marker in markers), name, markers, timeout)


def wait_file(path, name, timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if os.path.isfile(path) and os.path.getsize(path) > 0:
            observed.append(name)
            return
        read_available()
    raise RuntimeError(f"missing {name}: {path!r}; head={bytes(output)[:4000]!r}")


def wait_exit(timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        status = child_status()
        if status is not None:
            return drain_after_exit(status)
        read_available()
    raise RuntimeError(
        f"child did not exit; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


def send_key(key):
    os.write(fd, key)
    time.sleep(0.1)


def drain_after_exit(status):
    while True:
        ready, _, _ = select.select([fd], [], [], 0.2)
        if not ready:
            return status
        try:
            chunk = os.read(fd, 8192)
        except OSError:
            return status
        if not chunk:
            return status
        output.extend(chunk)
        screen.feed(chunk)


def resize(columns, rows):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
    screen.resize(columns, rows)
    try:
        os.killpg(pid, signal.SIGWINCH)
    except OSError:
        os.kill(pid, signal.SIGWINCH)


def kill_child():
    try:
        os.killpg(pid, signal.SIGKILL)
    except OSError:
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass
    try:
        os.waitpid(pid, 0)
    except ChildProcessError:
        pass


try:
    if scenario == "workflow":
        wait_new("Needs review: 0 / 1", "list")
        send_key(b"f")
        wait_parts(["Showing", "0/1"], "filter_empty")
        send_key(b"f")
        wait_new("Filter: All", "filter_all")
        send_key(b"/")
        send_key(b"/")
        wait_new("Type to search", "search_input")
        send_key(b"z")
        wait_new("No matching resources", "search_empty")
        send_key(b"\x7f")
        send_key(b"\x7f")
        send_key(b"\r")
        wait_new("terraform_data.api", "search_cleared")
        send_key(b"\r")
        wait_new("Resource 1/1", "detail")
        send_key(b"j")
        send_key(b"\r")
        wait_new("collaps", "expanded")
        send_key(b"y")
        wait_any(["Copied", "Copy failed"], "copy")
        send_key(b"\x1b")
        wait_new("Filter:", "back")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "failure":
        wait_new("Failed", "failed")
        send_key(b"y")
        wait_any(["Copied diagnostic", "Copy failed: clipboard unavailable."], "failure_copy")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "git_failure":
        wait_new("incomplete", "git_failure")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "interrupt":
        wait_file(os.environ["TERRACOTTA_FAKE_PID_PATH"], "terraform_started")
        send_key(b"\x03")
        wait_new("Cancelling...", "cancelling")
        exit_code = wait_exit()
    elif scenario == "narrow":
        wait_new("Terminal too small", "narrow")
        resize(100, 24)
        wait_new("Needs review: 0 / 1", "resized")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "empty":
        wait_parts(["Needs review: 0 / 0", "No res"], "empty")
        send_key(b"y")
        send_key(b"\r")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "compare_ref":
        wait_parts(["merge-base(main)", "Needs review: 1 / 1"], "compare_ref")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "panic":
        exit_code = wait_exit()
    else:
        raise RuntimeError(f"unknown scenario: {scenario}")
    print(f"exit={exit_code}")
    print(f"restored={str(b'\x1b[?1049l' in output).lower()}")
    print(f"cursor_restored={str(b'\x1b[?25h' in output).lower()}")
    print("observed=" + ",".join(observed))
except BaseException as error:
    kill_child()
    print(f"driver_error={error!r}")
    print(bytes(output)[-1200:].decode("utf-8", "replace"))
    raise
"#;

    struct Fixture {
        directory: PathBuf,
        root: PathBuf,
        bin: PathBuf,
        plan_path_record: PathBuf,
        pid_record: PathBuf,
        show_json: PathBuf,
        clipboard_path: PathBuf,
    }

    impl Fixture {
        fn new(with_git: bool) -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let directory =
                env::temp_dir().join(format!("terracotta-cli-pty-{}-{id}", std::process::id()));
            let root = directory.join("root with spaces");
            let bin = directory.join("fake-bin");
            fs::create_dir_all(&root).expect("fixture root should be created");
            fs::create_dir(&bin).expect("fake bin should be created");
            fs::write(
                root.join("main.tf"),
                "resource \"terraform_data\" \"api\" {\n  input = \"old\"\n}\n",
            )
            .expect("fixture configuration should be written");

            let plan_path_record = directory.join("plan-path");
            let pid_record = directory.join("terraform-pid");
            let show_json = directory.join("show.json");
            let clipboard_path = directory.join("clipboard");
            fs::write(&show_json, PLAN_JSON).expect("fake show JSON should be written");
            let terraform = bin.join("terraform");
            fs::write(&terraform, FAKE_TERRAFORM).expect("fake Terraform should be written");
            fs::set_permissions(&terraform, fs::Permissions::from_mode(0o755))
                .expect("fake Terraform should be executable");

            if with_git {
                git(&root, &["init", "--quiet", "--initial-branch=main"]);
                git(&root, &["add", "main.tf"]);
                git(
                    &root,
                    &[
                        "-c",
                        "user.name=Terracotta PTY",
                        "-c",
                        "user.email=pty@example.invalid",
                        "commit",
                        "--quiet",
                        "-m",
                        "test: establish fixture",
                    ],
                );
                fs::write(
                    root.join("main.tf"),
                    "resource \"terraform_data\" \"api\" {\n  input = \"new\"\n}\n",
                )
                .expect("fixture change should be written");
            }

            Self {
                directory,
                root,
                bin,
                plan_path_record,
                pid_record,
                show_json,
                clipboard_path,
            }
        }

        fn run(
            &self,
            scenario: &str,
            columns: u16,
            rows: u16,
            binary: &Path,
            command: &[&str],
        ) -> PtyResult {
            let original_path = env::var_os("PATH").expect("PATH should be available");
            let mut path_entries = vec![self.bin.clone()];
            path_entries.extend(env::split_paths(&original_path));
            let path = env::join_paths(path_entries).expect("test PATH should be valid");
            let mut process = Command::new("python3");
            process
                .arg("-c")
                .arg(PTY_DRIVER)
                .arg(binary)
                .arg(&self.root)
                .arg(columns.to_string())
                .arg(rows.to_string())
                .arg(scenario)
                .args(command)
                .env("PATH", path)
                .env("TERRACOTTA_FAKE_MODE", scenario)
                .env("TERRACOTTA_FAKE_PLAN_PATH", &self.plan_path_record)
                .env("TERRACOTTA_FAKE_PID_PATH", &self.pid_record)
                .env("TERRACOTTA_FAKE_SHOW_JSON", &self.show_json)
                .env(
                    "TERRACOTTA_TEST_CLIPBOARD",
                    if scenario == "failure" {
                        std::ffi::OsString::from("unavailable")
                    } else {
                        self.clipboard_path.clone().into_os_string()
                    },
                )
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("TF_IN_AUTOMATION", "1")
                .env("TF_CLI_CONFIG_FILE", "/dev/null")
                .env("CHECKPOINT_DISABLE", "1");
            if scenario == "panic" {
                process.env("TERRACOTTA_TEST_PANIC_AFTER_DRAW", "1");
            }
            let output = process.output().expect("PTY driver should start");
            assert!(
                output.status.success(),
                "PTY driver failed: {}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            PtyResult::parse(&String::from_utf8_lossy(&output.stdout))
        }

        fn use_empty_plan(&self) {
            fs::write(&self.show_json, EMPTY_PLAN_JSON).expect("empty plan should be written");
        }

        fn assert_temporary_plan_removed(&self) {
            let path = fs::read_to_string(&self.plan_path_record)
                .expect("fake Terraform should record its plan path");
            assert!(
                !Path::new(path.trim()).exists(),
                "temporary plan remains: {}",
                path.trim()
            );
        }

        fn assert_clipboard_recorded(&self) {
            let copied = fs::read_to_string(&self.clipboard_path)
                .expect("fake clipboard should record copied text");
            assert!(copied.contains("Resource ~ terraform_data.api"));
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.directory).expect("PTY fixture should be removed");
        }
    }

    struct PtyResult {
        exit_code: i32,
        restored: bool,
        cursor_restored: bool,
        observed: String,
    }

    impl PtyResult {
        fn parse(output: &str) -> Self {
            let field = |name: &str| {
                output
                    .lines()
                    .find_map(|line| line.strip_prefix(name))
                    .unwrap_or_else(|| panic!("PTY driver omitted {name}: {output}"))
            };
            Self {
                exit_code: field("exit=")
                    .parse()
                    .expect("PTY child exit should be numeric"),
                restored: field("restored=") == "true",
                cursor_restored: field("cursor_restored=") == "true",
                observed: field("observed=").to_owned(),
            }
        }

        fn assert_restored(&self) {
            assert!(
                self.restored,
                "alternate screen was not restored (exit={}, observed={})",
                self.exit_code, self.observed
            );
            assert!(
                self.cursor_restored,
                "cursor visibility was not restored (exit={}, observed={})",
                self.exit_code, self.observed
            );
        }

        fn observed(&self, event: &str) {
            assert!(
                self.observed.split(',').any(|observed| observed == event),
                "PTY event {event} was not observed: {}",
                self.observed
            );
        }
    }

    fn git(root: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .output()
            .expect("git should start");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn pty_workflow_connects_plan_list_detail_expansion_copy_and_quit() {
        let fixture = Fixture::new(true);
        let result = fixture.run(
            "workflow",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        for event in ["list", "filter_empty", "detail", "expanded", "copy", "back"] {
            result.observed(event);
        }
        fixture.assert_temporary_plan_removed();
        fixture.assert_clipboard_recorded();
    }

    #[test]
    fn pty_terraform_failure_copies_diagnostic_and_returns_failure() {
        let fixture = Fixture::new(true);
        let result = fixture.run(
            "failure",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 1);
        result.assert_restored();
        result.observed("failed");
        result.observed("failure_copy");
        fixture.assert_temporary_plan_removed();
    }

    #[test]
    fn pty_git_failure_keeps_plan_review_available_and_marks_analysis_incomplete() {
        let fixture = Fixture::new(false);
        let result = fixture.run(
            "git_failure",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("git_failure");
        fixture.assert_temporary_plan_removed();
    }

    #[test]
    fn pty_ctrl_c_reaps_terraform_and_returns_interrupt_status() {
        let fixture = Fixture::new(true);
        let result = fixture.run(
            "interrupt",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 130);
        result.assert_restored();
        result.observed("terraform_started");
        result.observed("cancelling");
        fixture.assert_temporary_plan_removed();
        let pid = fs::read_to_string(&fixture.pid_record)
            .expect("fake Terraform should record its pid")
            .trim()
            .parse::<u32>()
            .expect("fake Terraform pid should be numeric");
        assert!(
            !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .expect("kill should start")
                .success(),
            "Terraform process {pid} remains alive"
        );
    }

    #[test]
    fn pty_empty_plan_allows_quit_without_resource_actions() {
        let fixture = Fixture::new(true);
        fixture.use_empty_plan();
        let result = fixture.run(
            "empty",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("empty");
        fixture.assert_temporary_plan_removed();
    }

    #[test]
    fn pty_narrow_terminal_can_quit_after_resize_guidance() {
        let fixture = Fixture::new(true);
        let result = fixture.run(
            "narrow",
            47,
            20,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("narrow");
        fixture.assert_temporary_plan_removed();
    }

    #[test]
    fn pty_compare_ref_reaches_review_with_its_comparison_label() {
        let fixture = Fixture::new(true);
        let result = fixture.run(
            "compare_ref",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan", "--compare-ref", "main"],
        );

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("compare_ref");
        fixture.assert_temporary_plan_removed();
    }

    #[test]
    fn pty_panic_restores_terminal_before_the_test_process_continues() {
        if env::var_os("TERRACOTTA_PTY_PANIC_CHILD").is_some() {
            let panic_result = std::panic::catch_unwind(|| {
                ratatui::run(|_| panic!("synthetic terminal panic"));
            });
            assert!(panic_result.is_err());
            return;
        }

        let fixture = Fixture::new(true);
        let result = fixture.run(
            "panic",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_ne!(result.exit_code, 0);
        result.assert_restored();
    }
}
