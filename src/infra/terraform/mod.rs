#![allow(
    clippy::redundant_pub_crate,
    reason = "Terraform parsing is shared only within the crate"
)]

mod events;
pub(crate) mod execute;
pub(crate) mod hcl;
mod plan;
