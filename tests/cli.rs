use std::process::Command;

#[test]
fn help_and_version_work_without_a_terminal_or_terraform() {
    for arg in ["--help", "--version"] {
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

#[cfg(unix)]
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
import sys
import time

_, binary, root, columns, rows, scenario = sys.argv[:6]
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
            rows,
            columns,
            root,
            binary,
            *command,
        ],
    )

output = bytearray()
cursor = 0
observed = []


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
        output.extend(os.read(fd, 8192))
    except OSError:
        pass


def wait_new(marker, name, timeout=20):
    global cursor
    start = cursor
    deadline = time.time() + timeout
    while time.time() < deadline:
        if marker in bytes(output[start:]).decode("utf-8", "replace"):
            observed.append(name)
            cursor = len(output)
            return
        read_available()
        if child_status() is not None:
            break
    raise RuntimeError(
        f"missing {name}: {marker!r}; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


def wait_parts(markers, name, timeout=20):
    global cursor
    start = cursor
    deadline = time.time() + timeout
    while time.time() < deadline:
        text = bytes(output[start:]).decode("utf-8", "replace")
        if all(marker in text for marker in markers):
            observed.append(name)
            cursor = len(output)
            return
        read_available()
        if child_status() is not None:
            break
    raise RuntimeError(
        f"missing {name}; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


def wait_any(markers, name, timeout=20):
    global cursor
    start = cursor
    deadline = time.time() + timeout
    while time.time() < deadline:
        text = bytes(output[start:]).decode("utf-8", "replace")
        if any(marker in text for marker in markers):
            observed.append(name)
            cursor = len(output)
            return
        read_available()
        if child_status() is not None:
            break
    raise RuntimeError(
        f"missing {name}; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


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
            while True:
                ready, _, _ = select.select([fd], [], [], 0.1)
                if not ready:
                    break
                try:
                    chunk = os.read(fd, 8192)
                except OSError:
                    return status
                if not chunk:
                    return status
                output.extend(chunk)
            return status
        read_available()
    raise RuntimeError(
        f"child did not exit; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


def send_key(key):
    os.write(fd, key)
    time.sleep(0.1)


def send_until_exit(key):
    deadline = time.time() + 10
    while time.time() < deadline:
        status = child_status()
        if status is not None:
            return status
        os.write(fd, key)
        read_available()
        time.sleep(0.1)
    raise RuntimeError(
        f"child did not exit after repeated input; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


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
        exit_code = send_until_exit(b"q")
    elif scenario == "empty":
        wait_parts(["Needs review: 0 / 0", "No res"], "empty")
        send_key(b"y")
        send_key(b"\r")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "compare_ref":
        wait_new("merge-base(main)", "compare_ref")
        wait_new("Needs review: 1 / 1", "compare_ref_list")
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
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("TF_IN_AUTOMATION", "1")
                .env("TF_CLI_CONFIG_FILE", "/dev/null")
                .env("CHECKPOINT_DISABLE", "1");
            if scenario == "panic" {
                process.env("TERRACOTTA_PTY_PANIC_CHILD", "1");
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

        fn assert_alternate_screen_restored(&self) {
            assert!(
                self.restored,
                "alternate screen was not restored (exit={}, observed={})",
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
        let binary = env::current_exe().expect("integration test binary path should exist");
        let result = fixture.run(
            "panic",
            100,
            24,
            &binary,
            &[
                "--exact",
                "pty_tests::pty_panic_restores_terminal_before_the_test_process_continues",
                "--nocapture",
            ],
        );

        assert_eq!(result.exit_code, 0);
        result.assert_alternate_screen_restored();
    }
}
