use std::{ffi::OsStr, path::Path};

use crate::infra::CancellationToken;

use super::{GitInterrupted, command::checked_git};

pub(crate) fn current_branch(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<Option<String>, GitInterrupted> {
    let output = match checked_git(
        root,
        "read current Git branch",
        [OsStr::new("branch"), OsStr::new("--show-current")],
        cancellation,
    ) {
        Ok(output) => output,
        Err(error) if error.is_interrupted() => return Err(GitInterrupted),
        Err(_) => return Ok(None),
    };
    let branch = String::from_utf8(output.stdout)
        .ok()
        .map_or_else(String::new, |branch| branch.trim().to_owned());
    Ok((!branch.is_empty()).then_some(branch))
}
