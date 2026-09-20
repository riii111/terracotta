#![allow(
    clippy::redundant_pub_crate,
    reason = "source model is shared with the crate-private infrastructure module"
)]

// Git attribution is dormant while the product reviews Terraform's full text.
#[cfg(test)]
#[allow(
    dead_code,
    reason = "dormant Git attribution remains compiled only for its unit tests"
)]
pub(crate) mod attribution;
pub(crate) mod copy;
pub(crate) mod execution;
#[cfg(test)]
#[allow(
    dead_code,
    unused_imports,
    reason = "dormant Git plan models remain compiled only for attribution tests"
)]
pub(crate) mod plan;
pub(crate) mod review;
pub(crate) mod session;
