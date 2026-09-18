#[allow(
    dead_code,
    reason = "later MVP screens still consume the source and attribute APIs"
)]
mod app;
#[allow(
    dead_code,
    reason = "later MVP screens still consume the infrastructure APIs"
)]
mod infra;
pub(crate) mod runtime;
#[cfg(feature = "test-support")]
pub(crate) mod test_support;

mod ui;

use std::{path::Path, process::ExitCode};

/// Runs an interactive Terraform plan review for the supplied root.
#[must_use]
pub fn run_plan(root: &Path, compare_ref: Option<&str>) -> ExitCode {
    runtime::run_plan(root, compare_ref)
}

/// Runs the development-only plan review with synthetic data.
///
/// # Errors
///
/// Returns an I/O error when terminal drawing or input handling fails.
pub fn run_synthetic() -> std::io::Result<()> {
    runtime::run_synthetic()
}

/// Runs the development-only execution screen with synthetic events.
///
/// # Errors
///
/// Returns an I/O error when terminal drawing or input handling fails.
pub fn run_synthetic_execution() -> std::io::Result<()> {
    runtime::run_synthetic_execution()
}
