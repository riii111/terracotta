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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("terracotta"));
    assert!(!stdout.contains("compare-ref"));
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
fn compare_ref_is_explicitly_unavailable_before_terminal_setup() {
    let output = Command::new(env!("CARGO_BIN_EXE_terracotta"))
        .env("PATH", "")
        .args(["plan", "--compare-ref", "main"])
        .output()
        .expect("CLI should start");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.contains(&0x1b));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Git comparison is paused"));
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
    use rstest::rstest;
    use std::{
        env, fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    const PLAN_JSON: &str = r#"{
  "format_version": "1.0",
  "applyable": true,
  "resource_changes": [{
    "address": "terraform_data.api",
    "change": {"actions": ["update"], "after": {"secret": "must-not-be-logged"}}
  }],
  "output_changes": {"endpoint": {"after": "must-not-be-logged"}}
}"#;

    const PLAN_TEXT: &str = r#"Terraform will perform the following actions:

  # terraform_data.api will be updated in-place
  ~ resource "terraform_data" "api" {
      ~ input = "old" -> "new"
        secret = (sensitive value)
    }

Plan: 0 to add, 1 to change, 0 to destroy.
"#;

    const FAKE_TERRAFORM: &str = include_str!("support/cli/fake_terraform.sh");
    const PTY_DRIVER: &str = include_str!("support/cli/pty_driver.py");

    struct Fixture {
        directory: PathBuf,
        root: PathBuf,
        bin: PathBuf,
        invocations: PathBuf,
        plan_path_record: PathBuf,
        pid_record: PathBuf,
        show_json: PathBuf,
        show_text: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let directory =
                env::temp_dir().join(format!("terracotta-cli-pty-{}-{id}", std::process::id()));
            let root = directory.join("plain directory with spaces");
            let bin = directory.join("fake-bin");
            fs::create_dir_all(&root).expect("fixture root should be created");
            fs::create_dir(&bin).expect("fake bin should be created");

            let invocations = directory.join("invocations");
            let plan_path_record = directory.join("plan-path");
            let pid_record = directory.join("terraform-pid");
            let show_json = directory.join("show.json");
            let show_text = directory.join("show.txt");
            fs::write(&show_json, PLAN_JSON).expect("fake show JSON should be written");
            fs::write(&show_text, PLAN_TEXT).expect("fake show text should be written");
            fs::write(&invocations, "").expect("invocation log should be created");
            let terraform = bin.join("terraform");
            fs::write(&terraform, FAKE_TERRAFORM).expect("fake Terraform should be written");
            fs::set_permissions(&terraform, fs::Permissions::from_mode(0o755))
                .expect("fake Terraform should be executable");

            Self {
                directory,
                root,
                bin,
                invocations,
                plan_path_record,
                pid_record,
                show_json,
                show_text,
            }
        }

        fn run(&self, scenario: &str, columns: u16, rows: u16) -> PtyResult {
            let original_path = env::var_os("PATH").expect("PATH should be available");
            let mut path_entries = vec![self.bin.clone()];
            path_entries.extend(env::split_paths(&original_path));
            let path = env::join_paths(path_entries).expect("test PATH should be valid");
            let mut process = Command::new("python3");
            process
                .arg("-c")
                .arg(PTY_DRIVER)
                .arg(env!("CARGO_BIN_EXE_terracotta"))
                .arg(&self.root)
                .arg(columns.to_string())
                .arg(rows.to_string())
                .arg(scenario)
                .arg("plan")
                .env("PATH", path)
                .env("TERRACOTTA_FAKE_MODE", scenario)
                .env("TERRACOTTA_FAKE_INVOCATIONS", &self.invocations)
                .env("TERRACOTTA_FAKE_PLAN_PATH", &self.plan_path_record)
                .env("TERRACOTTA_FAKE_PID_PATH", &self.pid_record)
                .env("TERRACOTTA_FAKE_SHOW_JSON", &self.show_json)
                .env("TERRACOTTA_FAKE_SHOW_TEXT", &self.show_text)
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

        fn invocation_arguments(&self) -> Vec<String> {
            fs::read_to_string(&self.invocations)
                .expect("invocations should be readable")
                .lines()
                .map(|line| {
                    let (directory, arguments) = line
                        .split_once('|')
                        .expect("invocation should contain directory and arguments");
                    assert_eq!(Path::new(directory), self.root);
                    arguments.to_owned()
                })
                .collect()
        }

        fn assert_saved_plan_removed(&self) {
            if !self.plan_path_record.exists() {
                return;
            }
            let path = fs::read_to_string(&self.plan_path_record)
                .expect("fake Terraform should record its plan path");
            assert!(
                !Path::new(path.trim()).exists(),
                "saved plan remains: {}",
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
                exit_code: field("exit=").parse().expect("exit should be numeric"),
                restored: field("restored=") == "true",
                cursor_restored: field("cursor_restored=") == "true",
                observed: field("observed=").to_owned(),
            }
        }

        fn assert_restored(&self) {
            assert!(self.restored, "alternate screen was not restored");
            assert!(self.cursor_restored, "cursor visibility was not restored");
        }

        fn observed(&self, event: &str) {
            assert!(
                self.observed.split(',').any(|observed| observed == event),
                "PTY event {event} was not observed: {}",
                self.observed
            );
        }
    }

    #[test]
    fn pty_runs_init_plan_and_both_show_modes_in_the_original_directory() {
        let fixture = Fixture::new();
        let result = fixture.run("full_text", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("plan_text");
        let arguments = fixture.invocation_arguments();
        assert_eq!(arguments[0], "init -input=false -no-color");
        assert_eq!(arguments[1], "workspace show");
        assert!(arguments[2].starts_with("plan -input=false -json -detailed-exitcode -out="));
        assert!(arguments[3].starts_with("show -no-color "));
        assert!(arguments[4].starts_with("show -json "));
        assert_eq!(arguments.len(), 5);
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_success_keeps_diagnostics_above_the_exact_plan_text() {
        let fixture = Fixture::new();
        let result = fixture.run("diagnostic_success", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("diagnostic_and_plan");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_filter_navigation_restores_the_full_review_and_terminal() {
        let fixture = Fixture::new();
        let result = fixture.run("filter_navigation", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("filter_input");
        result.observed("filter_matches");
        result.observed("filter_confirmed");
        result.observed("filter_cleared");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_quit_requires_enter_and_keeps_other_result_actions_available() {
        let fixture = Fixture::new();
        let result = fixture.run("quit_confirmation", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("quit_confirmation");
        result.observed("quit_repeat");
        result.observed("quit_cancelled");
        result.observed("quit_ctrl_c");
        result.observed("quit_ctrl_c_cancelled");
        result.observed("quit_copy");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_apply_success_uses_the_saved_plan_once_and_cleans_it_after_quit() {
        let fixture = Fixture::new();
        let result = fixture.run("apply_success", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("apply_confirmation");
        result.observed("apply_started");
        result.observed("apply_success");
        let arguments = fixture.invocation_arguments();
        assert_eq!(arguments.len(), 6);
        assert!(arguments[5].starts_with("apply -input=false -no-color "));
        assert_eq!(
            arguments[2].split("-out=").nth(1),
            arguments[5].split("-input=false -no-color ").nth(1)
        );
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_apply_failure_keeps_the_result_and_returns_failure() {
        let fixture = Fixture::new();
        let result = fixture.run("apply_failure", 100, 24);

        assert_eq!(result.exit_code, 1);
        result.assert_restored();
        result.observed("apply_confirmation");
        result.observed("apply_started");
        result.observed("apply_failure");
        assert_eq!(
            fixture
                .invocation_arguments()
                .iter()
                .filter(|arguments| arguments.starts_with("apply "))
                .count(),
            1
        );
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_apply_interrupt_waits_for_terraform_and_returns_130() {
        let fixture = Fixture::new();
        let result = fixture.run("apply_interrupt", 100, 24);

        assert_eq!(result.exit_code, 130);
        result.assert_restored();
        result.observed("apply_confirmation");
        result.observed("apply_started");
        result.observed("apply_interrupted");
        assert_eq!(
            fixture
                .invocation_arguments()
                .iter()
                .filter(|arguments| arguments.starts_with("apply "))
                .count(),
            1
        );
        fixture.assert_saved_plan_removed();
        assert_child_reaped(&fixture.pid_record);
    }

    #[rstest]
    #[case::no("apply_no")]
    #[case::escape("apply_escape")]
    fn pty_declining_apply_returns_to_the_same_review_without_running_apply(
        #[case] scenario: &str,
    ) {
        let fixture = Fixture::new();
        let result = fixture.run(scenario, 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("apply_confirmation");
        result.observed("plan_restored");
        assert!(
            fixture
                .invocation_arguments()
                .iter()
                .all(|arguments| !arguments.starts_with("apply "))
        );
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_init_failure_skips_workspace_plan_and_show() {
        let fixture = Fixture::new();
        let result = fixture.run("init_failure", 100, 24);

        assert_eq!(result.exit_code, 1);
        result.assert_restored();
        result.observed("failed");
        assert_eq!(
            fixture.invocation_arguments(),
            ["init -input=false -no-color"]
        );
    }

    #[test]
    fn pty_plan_failure_preserves_logs_and_cleans_the_saved_plan() {
        let fixture = Fixture::new();
        let result = fixture.run("failure", 100, 24);

        assert_eq!(result.exit_code, 1);
        result.assert_restored();
        result.observed("failed");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_ctrl_c_reaps_terraform_cleans_the_plan_and_returns_130() {
        let fixture = Fixture::new();
        let result = fixture.run("interrupt", 100, 24);

        assert_eq!(result.exit_code, 130);
        result.assert_restored();
        result.observed("terraform_started");
        result.observed("interrupt_requested");
        fixture.assert_saved_plan_removed();
        assert_child_reaped(&fixture.pid_record);
    }

    #[test]
    fn pty_narrow_terminal_recovers_after_resize() {
        let fixture = Fixture::new();
        let result = fixture.run("narrow", 20, 20);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("narrow");
        result.observed("resized");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_apply_confirmation_waits_for_resize_before_starting() {
        let fixture = Fixture::new();
        let result = fixture.run("apply_resize", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("apply_confirmation_narrow");
        result.observed("apply_confirmation_resized");
        result.observed("apply_started");
        result.observed("apply_result");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_panic_restores_terminal_and_cleans_any_created_plan() {
        let fixture = Fixture::new();
        let result = fixture.run("panic", 100, 24);

        assert_ne!(result.exit_code, 0);
        result.assert_restored();
        fixture.assert_saved_plan_removed();
    }

    #[test]
    #[ignore = "requires Terraform CLI and the cloudless basic scenario"]
    fn pty_basic_scenario_shows_the_full_plan_and_restores_terminal() {
        let scenario = BasicScenario::setup();
        let result = scenario.run();

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("plan_text");
        result.observed("apply_confirmation");
        result.observed("apply_result");
    }

    struct BasicScenario {
        directory: PathBuf,
    }

    impl BasicScenario {
        fn setup() -> Self {
            let output = Command::new("python3")
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .args(["fixtures/basic/scenario.py", "setup"])
                .output()
                .expect("basic scenario setup should start");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            Self {
                directory: PathBuf::from(
                    String::from_utf8(output.stdout)
                        .expect("scenario path should be UTF-8")
                        .trim(),
                ),
            }
        }

        fn run(&self) -> PtyResult {
            let output = Command::new("python3")
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .arg("tests/support/cli/pty_driver.py")
                .arg(env!("CARGO_BIN_EXE_terracotta"))
                .arg(&self.directory)
                .args(["100", "24", "basic_workflow", "plan"])
                .env("TF_IN_AUTOMATION", "1")
                .env("TF_DATA_DIR", self.directory.join(".terraform"))
                .env("TF_CLI_CONFIG_FILE", "/dev/null")
                .env("CHECKPOINT_DISABLE", "1")
                .output()
                .expect("basic scenario PTY should start");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            PtyResult::parse(&String::from_utf8_lossy(&output.stdout))
        }
    }

    impl Drop for BasicScenario {
        fn drop(&mut self) {
            clean_basic_scenario(&self.directory);
        }
    }

    fn clean_basic_scenario(directory: &Path) {
        let output = Command::new("python3")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["fixtures/basic/scenario.py", "clean"])
            .arg(directory)
            .output()
            .expect("basic scenario cleanup should start");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn assert_child_reaped(path: &Path) {
        let pid = fs::read_to_string(path)
            .expect("Terraform pid should be recorded")
            .trim()
            .parse::<u32>()
            .expect("Terraform pid should be numeric");
        assert!(
            !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .expect("kill should start")
                .success(),
            "Terraform process {pid} remains alive"
        );
    }
}
