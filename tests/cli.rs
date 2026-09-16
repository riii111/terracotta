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
