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
