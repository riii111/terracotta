use clap::{CommandFactory, Parser};

#[derive(Parser)]
#[command(version, about)]
struct Cli {}

fn main() -> std::io::Result<()> {
    Cli::parse();
    Cli::command().print_help()
}
