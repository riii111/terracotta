use std::io;

mod execution;
mod plan_list;

/// Runs the development-only plan list with synthetic plan and attribution data.
///
/// # Errors
///
/// Returns an I/O error when terminal drawing or input handling fails.
pub fn run_synthetic() -> io::Result<()> {
    plan_list::run_synthetic()
}

/// Runs the development-only execution screen with synthetic event data.
///
/// # Errors
///
/// Returns an I/O error when terminal drawing or input handling fails.
pub fn run_synthetic_execution() -> io::Result<()> {
    execution::run_synthetic_execution()
}

#[cfg(test)]
mod test_support;
