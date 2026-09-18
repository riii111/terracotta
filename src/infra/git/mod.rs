mod branch;
mod command;
mod configuration;
mod diff;
mod merge_base;
mod rev_parse;

pub(crate) use command::GitCommandError;

pub(crate) use branch::current_branch;
pub(crate) use configuration::{
    ConfigurationComparison, ConfigurationComparisons, ConfigurationSnapshot,
    capture_working_tree_configuration_with_cancellation, compare_configurations_with_cancellation,
};
pub(crate) use diff::{
    ComparisonBasis, GitDiff, collect_diff_against_ref_with_cancellation,
    collect_diff_with_cancellation,
};
