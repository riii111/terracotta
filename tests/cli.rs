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

    const UI11_PLAN_JSON: &str = r#"{
  "format_version": "1.0",
  "resource_changes": [
    {
      "address": "terraform_data.api",
      "mode": "managed",
      "change": {
        "actions": ["update"],
        "before": {"input": "old"},
        "after": {"input": "new"}
      }
    },
    {
      "address": "terraform_data.new",
      "mode": "managed",
      "change": {
        "actions": ["create"],
        "before": null,
        "after": {"input": "created"}
      }
    },
    {
      "address": "terraform_data.old",
      "mode": "managed",
      "change": {
        "actions": ["delete"],
        "before": {"input": "old"},
        "after": null
      }
    }
  ]
}"#;

    const FAKE_TERRAFORM: &str = include_str!("support/cli/fake_terraform.sh");

    const FAKE_GIT: &str = include_str!("support/cli/fake_git.sh");

    const PTY_DRIVER: &str = include_str!("support/cli/pty_driver.py");

    struct Fixture {
        directory: PathBuf,
        root: PathBuf,
        bin: PathBuf,
        plan_path_record: PathBuf,
        pid_record: PathBuf,
        git_pid_record: PathBuf,
        plan_done_record: PathBuf,
        show_json: PathBuf,
    }

    impl Fixture {
        fn new(with_git: bool) -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let directory =
                env::temp_dir().join(format!("terracotta-cli-pty-{}-{id}", std::process::id()));
            let repository = directory.join("repository with spaces");
            let root = repository.join("environments/development/main");
            let production = repository.join("environments/production/main");
            let common = repository.join("common/main");
            let bin = directory.join("fake-bin");
            fs::create_dir_all(&root).expect("fixture root should be created");
            fs::create_dir_all(&production).expect("production fixture should be created");
            fs::create_dir_all(&common).expect("common fixture should be created");
            fs::create_dir(&bin).expect("fake bin should be created");
            fs::write(
                root.join("service.tf"),
                "resource \"terraform_data\" \"api\" {\n  input = \"old\"\n}\n",
            )
            .expect("fixture configuration should be written");
            fs::write(
                production.join("service.tf"),
                "resource \"terraform_data\" \"api\" {\n  input = \"production\"\n}\n",
            )
            .expect("production configuration should be written");
            fs::write(
                common.join("service.tf"),
                "resource \"terraform_data\" \"shared\" {\n  input = \"common\"\n}\n",
            )
            .expect("common configuration should be written");

            let plan_path_record = directory.join("plan-path");
            let pid_record = directory.join("terraform-pid");
            let git_pid_record = directory.join("git-pid");
            let plan_done_record = directory.join("plan-done");
            let show_json = directory.join("show.json");
            fs::write(&show_json, PLAN_JSON).expect("fake show JSON should be written");
            let terraform = bin.join("terraform");
            fs::write(&terraform, FAKE_TERRAFORM).expect("fake Terraform should be written");
            fs::set_permissions(&terraform, fs::Permissions::from_mode(0o755))
                .expect("fake Terraform should be executable");
            let fake_git = bin.join("git");
            fs::write(&fake_git, FAKE_GIT).expect("fake Git should be written");
            fs::set_permissions(&fake_git, fs::Permissions::from_mode(0o755))
                .expect("fake Git should be executable");

            if with_git {
                git(&repository, &["init", "--quiet", "--initial-branch=main"]);
                git(&repository, &["add", "."]);
                git(
                    &repository,
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
                    root.join("service.tf"),
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
                git_pid_record,
                plan_done_record,
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
                .env("TERRACOTTA_FAKE_GIT_PID_PATH", &self.git_pid_record)
                .env("TERRACOTTA_FAKE_PLAN_DONE_PATH", &self.plan_done_record)
                .env("TERRACOTTA_FAKE_SHOW_JSON", &self.show_json)
                .env(
                    "TERRACOTTA_REAL_GIT",
                    real_git_path()
                        .to_str()
                        .expect("real Git path should be UTF-8"),
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

        fn use_plan_json(&self, plan_json: &str) {
            fs::write(&self.show_json, plan_json).expect("plan JSON should be written");
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

        fn observed(&self, event: &str) {
            assert!(
                self.observed.split(',').any(|observed| observed == event),
                "PTY event {event} was not observed: {}",
                self.observed
            );
        }
    }

    struct BasicScenario {
        directory: PathBuf,
    }

    impl BasicScenario {
        fn setup() -> Self {
            let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
            let output = Command::new("python3")
                .current_dir(manifest_dir)
                .args(["fixtures/basic/scenario.py", "setup"])
                .output()
                .expect("basic scenario setup should start");
            assert!(
                output.status.success(),
                "basic scenario setup failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let directory = String::from_utf8(output.stdout)
                .expect("scenario path should be UTF-8")
                .trim()
                .parse()
                .expect("scenario path should be valid");
            Self { directory }
        }

        fn run(&self, binary: &Path) -> PtyResult {
            let mut process = Command::new("python3");
            remove_ambient_cli_environment(&mut process);
            process
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .arg("tests/support/cli/pty_driver.py")
                .arg(binary)
                .arg(&self.directory)
                .args(["100", "24", "basic_workflow", "plan"])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("TF_IN_AUTOMATION", "1")
                .env("TF_DATA_DIR", self.directory.join(".terraform"))
                .env("TF_CLI_CONFIG_FILE", "/dev/null")
                .env("CHECKPOINT_DISABLE", "1");
            let output = process.output().expect("basic scenario PTY should start");
            assert!(
                output.status.success(),
                "basic scenario PTY failed: {}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            PtyResult::parse(&String::from_utf8_lossy(&output.stdout))
        }

        fn clean(mut self) {
            clean_basic_scenario(&self.directory);
            self.directory.clear();
        }
    }

    impl Drop for BasicScenario {
        fn drop(&mut self) {
            if !self.directory.as_os_str().is_empty() {
                clean_basic_scenario(&self.directory);
            }
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
            "basic scenario cleanup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn remove_ambient_cli_environment(process: &mut Command) {
        for (key, _) in env::vars_os() {
            let name = key.to_string_lossy();
            if name.starts_with("GIT_") || name.starts_with("TF_") {
                process.env_remove(&key);
            }
        }
    }

    #[test]
    #[ignore = "requires Terraform CLI and the cloudless basic scenario"]
    fn pty_basic_scenario_covers_review_path_and_restores_terminal() {
        let scenario = BasicScenario::setup();
        let result = scenario.run(Path::new(env!("CARGO_BIN_EXE_terracotta")));

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        for event in ["list", "filter", "detail", "analysis_info", "back"] {
            result.observed(event);
        }
        scenario.clean();
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

    fn real_git_path() -> PathBuf {
        env::split_paths(&env::var_os("PATH").expect("PATH should be available"))
            .map(|directory| directory.join("git"))
            .find(|path| path.is_file())
            .expect("real Git should be available")
    }

    fn assert_child_reaped(path: &Path, label: &str) {
        let pid = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{label} pid should be recorded: {error}"))
            .trim()
            .parse::<u32>()
            .unwrap_or_else(|error| panic!("{label} pid should be numeric: {error}"));
        assert!(
            !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .expect("kill should start")
                .success(),
            "{label} process {pid} remains alive"
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
    fn pty_ui11_covers_filtered_navigation_and_change_sides() {
        let fixture = Fixture::new(true);
        fixture.use_plan_json(UI11_PLAN_JSON);
        let result = fixture.run(
            "ui11",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        for event in [
            "ui11_list",
            "ui11_filter",
            "ui11_create_detail",
            "ui11_delete_detail",
            "ui11_previous",
            "ui11_analysis_open",
            "ui11_analysis_closed",
            "ui11_detail_back",
        ] {
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
    fn pty_success_keeps_plan_diagnostic_available_in_review() {
        let fixture = Fixture::new(true);
        let result = fixture.run(
            "diagnostic_success",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 0);
        result.assert_restored();
        for event in ["diagnostic_notice", "diagnostics", "diagnostics_back"] {
            result.observed(event);
        }
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
    fn pty_ctrl_c_reaps_git_during_diff_and_returns_interrupt_status() {
        let fixture = Fixture::new(true);
        let result = fixture.run(
            "git_interrupt",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 130);
        result.assert_restored();
        result.observed("git_started");
        result.observed("cancelling");
        assert_child_reaped(&fixture.git_pid_record, "Git diff");
    }

    #[test]
    fn pty_ctrl_c_reaps_git_during_post_plan_configuration_and_cleans_plan() {
        let fixture = Fixture::new(true);
        let result = fixture.run(
            "config_interrupt",
            100,
            24,
            Path::new(env!("CARGO_BIN_EXE_terracotta")),
            &["plan"],
        );

        assert_eq!(result.exit_code, 130);
        result.assert_restored();
        result.observed("git_started");
        assert_child_reaped(&fixture.git_pid_record, "post-plan Git");
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
