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

pub mod ui;

use std::{path::Path, process::ExitCode};

/// Runs an interactive Terraform plan review for the supplied root.
#[must_use]
pub fn run_plan(root: &Path, compare_ref: Option<&str>) -> ExitCode {
    runtime::run_plan(root, compare_ref)
}
