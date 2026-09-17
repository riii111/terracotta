#![allow(
    clippy::redundant_pub_crate,
    reason = "Terraform infrastructure is shared only within the crate"
)]

pub(crate) mod clipboard;
#[cfg(feature = "test-support")]
mod clipboard_test_support;
pub(crate) mod git;
pub(crate) mod review;
pub(crate) mod terraform;

#[cfg(not(feature = "test-support"))]
pub(crate) use clipboard::SystemClipboard as ClipboardExecutor;
#[cfg(feature = "test-support")]
pub(crate) use clipboard_test_support::ClipboardExecutor;
