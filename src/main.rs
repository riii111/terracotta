use std::{env, ffi::OsString, process::ExitCode};

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run Terraform, reviewing supported interactive plans.
    Terraform,
    /// Review a Terraform plan.
    Plan,
    /// Run Terraform apply.
    Apply,
}

fn main() -> ExitCode {
    let arguments: Vec<OsString> = env::args_os().collect();
    match arguments.get(1).and_then(|arg| arg.to_str()) {
        Some("terraform") => terracotta::run_terraform(&arguments[2..]),
        Some("plan" | "apply") => terracotta::run_terraform(&arguments[1..]),
        _ => {
            let _ = Cli::parse_from(arguments);
            if Cli::command().print_help().is_err() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}
