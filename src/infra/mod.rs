#![allow(
    clippy::redundant_pub_crate,
    reason = "Terraform infrastructure is shared only within the crate"
)]

pub(crate) mod terraform;
