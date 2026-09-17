#![allow(
    clippy::redundant_pub_crate,
    reason = "source model is shared with the crate-private infrastructure module"
)]

pub(crate) mod attribution;
pub(crate) mod plan;
pub(crate) mod source_location;
