use std::io::{self, IsTerminal};

use terracotta::run_synthetic;

fn main() -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "the terminal example requires an interactive terminal",
        ));
    }

    run_synthetic()
}
