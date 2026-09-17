#![allow(
    clippy::redundant_pub_crate,
    reason = "source model is shared with the crate-private infrastructure module"
)]

pub(crate) mod attribute_diff;
pub(crate) mod attribution;
pub(crate) mod execution;
pub(crate) mod plan;
pub(crate) mod plan_list;
pub(crate) mod progress;
pub(crate) mod source_location;
