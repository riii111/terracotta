use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Plan {
        #[arg(long, value_name = "REF")]
        compare_ref: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Plan { compare_ref }) => match env::current_dir() {
            Ok(root) => terracotta::run_plan(&root, compare_ref.as_deref()),
            Err(error) => {
                let _ = writeln!(
                    io::stderr(),
                    "failed to read the current directory: {error}"
                );
                ExitCode::from(1)
            }
        },
        None => {
            if Cli::command().print_help().is_err() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}
