#![allow(
    clippy::redundant_pub_crate,
    reason = "UI submodules are crate-internal implementation boundaries"
)]

pub(crate) mod features;
mod input;
#[cfg(test)]
mod preview;
mod primitives;
mod shell;
#[cfg(test)]
#[allow(
    dead_code,
    reason = "shared UI fixtures are consumed by feature-specific snapshot tests"
)]
mod test_support;
mod theme;
