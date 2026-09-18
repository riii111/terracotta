#![allow(
    clippy::redundant_pub_crate,
    reason = "UI submodules are crate-internal implementation boundaries"
)]

use crate::runtime;

pub(crate) mod features;
mod input;
mod primitives;
mod shell;
mod theme;

#[cfg(test)]
mod test_support;

/// Runs the development-only plan list with synthetic plan and attribution data.
///
/// # Errors
///
/// Returns an I/O error when terminal drawing or input handling fails.
pub fn run_synthetic() -> std::io::Result<()> {
    runtime::run_synthetic()
}

/// Runs the development-only execution screen with synthetic event data.
///
/// # Errors
///
/// Returns an I/O error when terminal drawing or input handling fails.
pub fn run_synthetic_execution() -> std::io::Result<()> {
    runtime::run_synthetic_execution()
}
