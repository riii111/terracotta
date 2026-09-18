use std::{ffi::OsStr, path::Path};

use crate::infra::CancellationToken;

use super::command::checked_git;

pub(crate) fn current_branch(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<Option<String>, super::command::GitCommandError> {
    let output = match checked_git(
        root,
        "read current Git branch",
        [OsStr::new("branch"), OsStr::new("--show-current")],
        cancellation,
    ) {
        Ok(output) => output,
        Err(error) if error.is_interrupted() => return Err(error),
        Err(_) => return Ok(None),
    };
    let branch = String::from_utf8(output.stdout)
        .ok()
        .map_or_else(String::new, |branch| branch.trim().to_owned());
    Ok((!branch.is_empty()).then_some(branch))
}
