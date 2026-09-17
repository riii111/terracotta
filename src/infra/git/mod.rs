#![allow(
    clippy::redundant_pub_crate,
    reason = "Git comparison types are shared only within the crate"
)]

use std::{ffi::OsStr, path::Path};

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

pub(crate) fn current_branch(root: &Path) -> Option<String> {
    let output = command::checked_git(
        root,
        "read current Git branch",
        [OsStr::new("branch"), OsStr::new("--show-current")],
    )
    .ok()?;
    let branch = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!branch.is_empty()).then_some(branch)
}
