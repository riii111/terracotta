#![allow(
    clippy::redundant_pub_crate,
    reason = "Git comparison types are shared only within the crate"
)]

mod command;
mod configuration;
mod diff;
mod merge_base;
mod rev_parse;

pub(crate) use configuration::{
    ConfigurationComparison, ConfigurationSnapshot, capture_working_tree_configuration,
    compare_commit_configurations, compare_configuration,
};
pub(crate) use diff::{ComparisonBasis, GitDiff, collect_diff, collect_diff_against_ref};

#[allow(unused_imports, reason = "preserve the crate-internal Git facade")]
pub(crate) use configuration::capture_revision_configuration;
#[allow(unused_imports, reason = "preserve the crate-internal Git facade")]
pub(crate) use diff::GitDiffStatus;
