use std::{ffi::OsStr, path::Path};

use super::command::checked_git;

pub(crate) fn current_branch(root: &Path) -> Option<String> {
    let output = checked_git(
        root,
        "read current Git branch",
        [OsStr::new("branch"), OsStr::new("--show-current")],
    )
    .ok()?;
    let branch = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!branch.is_empty()).then_some(branch)
}
