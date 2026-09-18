#![allow(
    clippy::redundant_pub_crate,
    reason = "UI submodules are crate-internal implementation boundaries"
)]

pub(crate) mod features;
mod input;
mod primitives;
mod shell;
mod theme;

#[cfg(test)]
mod test_support;
