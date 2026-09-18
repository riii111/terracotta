#![allow(
    clippy::redundant_pub_crate,
    reason = "Terraform infrastructure is shared only within the crate"
)]

pub(crate) mod clipboard;
pub(crate) mod git;
pub(crate) mod review;
pub(crate) mod terraform;

pub(crate) use clipboard::SystemClipboard as ClipboardExecutor;
