#![allow(
    clippy::redundant_pub_crate,
    reason = "Terraform parsing is shared only within the crate"
)]

pub(crate) mod hcl;
