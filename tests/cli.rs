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
    "change": {
      "actions": ["update"],
      "before": {"secret": "old-secret"},
      "before_sensitive": {"secret": true},
      "after": {"secret": "must-not-be-logged"},
      "after_sensitive": {"secret": true}
    }
  }],
  "output_changes": {"endpoint": {"change": {"after": "must-not-be-logged", "after_sensitive": true}}}
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
        signal_log: PathBuf,
        show_json: PathBuf,
        show_text: PathBuf,
        env_log: PathBuf,
        tool_log: PathBuf,
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
            let signal_log = directory.join("signals");
            let show_json = directory.join("show.json");
            let show_text = directory.join("show.txt");
            let env_log = directory.join("environment");
            let tool_log = directory.join("tools");
            fs::write(&show_json, PLAN_JSON).expect("fake show JSON should be written");
            fs::write(&show_text, PLAN_TEXT).expect("fake show text should be written");
            fs::write(&invocations, "").expect("invocation log should be created");
            fs::write(&env_log, "").expect("environment log should be created");
            fs::write(&tool_log, "").expect("tool log should be created");
            fs::write(&signal_log, "").expect("signal log should be created");
            let terraform = bin.join("terraform");
            fs::write(&terraform, FAKE_TERRAFORM).expect("fake Terraform should be written");
            fs::set_permissions(&terraform, fs::Permissions::from_mode(0o755))
                .expect("fake Terraform should be executable");
            let tofu = bin.join("tofu");
            fs::write(&tofu, FAKE_TERRAFORM).expect("fake OpenTofu should be written");
            fs::set_permissions(&tofu, fs::Permissions::from_mode(0o755))
                .expect("fake OpenTofu should be executable");

            Self {
                directory,
                root,
                bin,
                invocations,
                plan_path_record,
                pid_record,
                signal_log,
                show_json,
                show_text,
                env_log,
                tool_log,
            }
        }

        fn run(&self, scenario: &str, columns: u16, rows: u16) -> PtyResult {
            self.run_with_command(scenario, columns, rows, "plan")
        }

        fn run_with_command(
            &self,
            scenario: &str,
            columns: u16,
            rows: u16,
            command: &str,
        ) -> PtyResult {
            self.run_with_arguments(scenario, columns, rows, command, &[])
        }

        fn run_with_arguments(
            &self,
            scenario: &str,
            columns: u16,
            rows: u16,
            command: &str,
            arguments: &[&str],
        ) -> PtyResult {
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
                .arg(command)
                .args(arguments)
                .env("PATH", path)
                .env("TERRACOTTA_FAKE_MODE", scenario)
                .env("TERRACOTTA_FAKE_INVOCATIONS", &self.invocations)
                .env("TERRACOTTA_FAKE_PLAN_PATH", &self.plan_path_record)
                .env("TERRACOTTA_FAKE_PID_PATH", &self.pid_record)
                .env("TERRACOTTA_FAKE_SIGNAL_LOG", &self.signal_log)
                .env("TERRACOTTA_FAKE_SHOW_JSON", &self.show_json)
                .env("TERRACOTTA_FAKE_SHOW_TEXT", &self.show_text)
                .env("TERRACOTTA_FAKE_ENV_LOG", &self.env_log)
                .env("TERRACOTTA_FAKE_TOOL_LOG", &self.tool_log)
                .env_remove("TF_IN_AUTOMATION")
                .env_remove("CI")
                .env_remove("TF_CLI_ARGS")
                .env_remove("TF_CLI_ARGS_plan")
                .env("TF_CLI_CONFIG_FILE", "/dev/null")
                .env("CHECKPOINT_DISABLE", "1");
            if scenario == "panic" {
                process.env("TERRACOTTA_TEST_PANIC_AFTER_DRAW", "1");
            }
            if scenario == "cli_args" {
                process.env("TF_CLI_ARGS", "-no-color");
                process.env("TF_CLI_ARGS_plan", "-refresh=false");
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

        fn forwarded_cli_arguments(&self) -> Vec<String> {
            fs::read_to_string(&self.env_log)
                .expect("fake Terraform environment log should be readable")
                .lines()
                .map(str::to_owned)
                .collect()
        }

        fn signal_count(&self) -> usize {
            fs::read_to_string(&self.signal_log)
                .expect("signal log should be readable")
                .lines()
                .count()
        }

        fn invoked_tools(&self) -> Vec<String> {
            fs::read_to_string(&self.tool_log)
                .expect("fake tool log should be readable")
                .lines()
                .map(str::to_owned)
                .collect()
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

        fn assert_no_tui(&self) {
            assert!(!self.restored, "plan failure unexpectedly entered the TUI");
            assert!(
                !self.cursor_restored,
                "plan failure unexpectedly changed the cursor"
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

    #[test]
    fn pty_runs_plan_and_both_show_modes_in_the_original_directory() {
        let fixture = Fixture::new();
        let result = fixture.run("full_text", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("plan_text");
        let arguments = fixture.invocation_arguments();
        assert!(arguments[0].starts_with("plan -detailed-exitcode -out="));
        assert_eq!(arguments[1], "version -json");
        assert_eq!(arguments[2], "workspace show");
        assert!(arguments[3].starts_with("show -no-color "));
        assert!(arguments[4].starts_with("show -json "));
        assert_eq!(arguments.len(), 5);
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_opentofu_uses_the_shared_review_path_and_selected_executable() {
        let fixture = Fixture::new();
        let result = fixture.run_with_arguments("full_text", 100, 24, "tofu", &["plan"]);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("plan_text");
        assert_eq!(fixture.invoked_tools(), vec!["tofu".to_owned(); 5]);
        assert_eq!(fixture.invocation_arguments()[1], "version -json");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_success_keeps_plan_status_above_plan_text() {
        let fixture = Fixture::new();
        let result = fixture.run("diagnostic_success", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("plan_status_and_text");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_apply_no_changes_exits_without_confirmation_or_apply() {
        let fixture = Fixture::new();
        let result = fixture.run_with_command("no_changes", 100, 24, "apply");

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("no_changes");
        assert!(
            fixture
                .invocation_arguments()
                .iter()
                .all(|arguments| !arguments.starts_with("apply "))
        );
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_detailed_exit_code_is_returned_after_review() {
        let fixture = Fixture::new();
        let result =
            fixture.run_with_arguments("detailed", 100, 24, "plan", &["-detailed-exitcode"]);

        assert_eq!(result.exit_code, 2);
        result.assert_restored();
        result.observed("plan_text");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_apply_maps_plan_only_options_away_from_apply() {
        let fixture = Fixture::new();
        let result = fixture.run_with_arguments(
            "apply_mapping",
            100,
            24,
            "apply",
            &["-var", "name=value", "-parallelism", "4"],
        );

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("apply_success");
        let arguments = fixture.invocation_arguments();
        assert!(arguments[0].contains("-var name=value"));
        assert!(arguments[0].contains("-parallelism 4"));
        assert!(arguments[6].contains("-parallelism 4"));
        assert!(!arguments[6].contains("-var"));
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_user_owned_output_path_is_not_removed() {
        let fixture = Fixture::new();
        let result =
            fixture.run_with_arguments("user_output", 100, 24, "plan", &["-out=review.tfplan"]);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        let output = fixture.root.join("review.tfplan");
        assert!(output.exists());
        assert!(fixture.invocation_arguments()[0].contains(&output.display().to_string()));
        fs::remove_file(output).expect("user-owned output should be cleaned by the test");
    }

    #[test]
    fn pty_managed_children_do_not_receive_cli_argument_environment_again() {
        let fixture = Fixture::new();
        let result = fixture.run("cli_args", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("plan_text");
        assert!(
            fixture.invocation_arguments()[0]
                .starts_with("plan -no-color -refresh=false -detailed-exitcode -out=")
        );
        assert!(fixture.forwarded_cli_arguments().iter().all(|line| {
            matches!(
                line.as_str(),
                "TF_CLI_ARGS=" | "TF_CLI_ARGS_plan=" | "TF_CLI_ARGS_apply="
            )
        }));
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_filter_navigation_restores_the_full_review_and_terminal() {
        let fixture = Fixture::new();
        let result = fixture.run("filter_navigation", 100, 24);

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("plan_position");
        result.observed("filter_input");
        result.observed("filter_matches");
        result.observed("filter_confirmed");
        result.observed("filter_help");
        result.observed("filter_context");
        result.observed("filter_copy");
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
        let result = fixture.run_with_command("apply_success", 100, 24, "apply");

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("plan_help");
        result.observed("apply_confirmation");
        result.observed("apply_help");
        result.observed("apply_context");
        result.observed("apply_started");
        result.observed("apply_success");
        let arguments = fixture.invocation_arguments();
        assert_eq!(arguments.len(), 7);
        assert!(arguments[5].starts_with("workspace show"));
        assert!(arguments[6].starts_with("apply -json -input=false "));
        assert_eq!(
            arguments[0].split("-out=").nth(1),
            arguments[6].split("-json -input=false ").nth(1)
        );
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_apply_progress_can_open_close_and_reopen_the_log_viewer() {
        let fixture = Fixture::new();
        let result = fixture.run_with_command("apply_log_view", 100, 24, "apply");

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        result.observed("apply_started");
        result.observed("apply_logs_open");
        result.observed("apply_logs_closed");
        result.observed("apply_logs_reopened");
        result.observed("apply_success");
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_apply_failure_keeps_the_result_and_returns_failure() {
        let fixture = Fixture::new();
        let result = fixture.run_with_command("apply_failure", 100, 24, "apply");

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
        let result = fixture.run_with_command("apply_interrupt", 100, 24, "apply");

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
        let result = fixture.run_with_command(scenario, 100, 24, "apply");

        assert_eq!(result.exit_code, 1);
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
    fn pty_plan_failure_skips_workspace_and_show() {
        let fixture = Fixture::new();
        let result = fixture.run("failure", 100, 24);

        assert_eq!(result.exit_code, 1);
        result.assert_no_tui();
        result.observed("failed");
        assert_eq!(fixture.invocation_arguments().len(), 1);
        assert!(fixture.invocation_arguments()[0].starts_with("plan "));
        fixture.assert_saved_plan_removed();
    }

    #[test]
    fn pty_ctrl_c_reaps_terraform_cleans_the_plan_and_returns_130() {
        let fixture = Fixture::new();
        let result = fixture.run("interrupt", 100, 24);

        assert_eq!(result.exit_code, 130);
        result.assert_no_tui();
        result.observed("terraform_started");
        result.observed("interrupt_requested");
        fixture.assert_saved_plan_removed();
        assert_eq!(fixture.signal_count(), 1);
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
        let result = fixture.run_with_command("apply_resize", 100, 24, "apply");

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
                .env_remove("TF_IN_AUTOMATION")
                .env_remove("CI")
                .env_remove("TF_CLI_ARGS")
                .env_remove("TF_CLI_ARGS_plan")
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
