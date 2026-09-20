#![allow(
    clippy::redundant_pub_crate,
    reason = "Terraform infrastructure is shared only within the crate"
)]

mod cancellation;
pub(crate) mod clipboard;
// Git comparison is paused in the full-text experience and retained for unit tests.
#[cfg(test)]
#[allow(
    dead_code,
    reason = "dormant Git integration remains compiled only for unit tests"
)]
pub(crate) mod git;
#[cfg(test)]
#[allow(
    dead_code,
    reason = "dormant Git review remains compiled only for unit tests"
)]
pub(crate) mod review;
pub(crate) mod terraform;

pub(crate) use cancellation::CancellationToken;
pub(crate) use clipboard::SystemClipboard as ClipboardExecutor;
