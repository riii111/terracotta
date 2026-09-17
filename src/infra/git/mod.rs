mod branch;
mod command;
mod configuration;
mod diff;
mod merge_base;
mod rev_parse;

pub(crate) use branch::current_branch;
pub(crate) use configuration::{
    ConfigurationComparison, ConfigurationSnapshot, capture_working_tree_configuration,
    compare_commit_configurations, compare_configuration,
};
pub(crate) use diff::{ComparisonBasis, GitDiff, collect_diff, collect_diff_against_ref};
