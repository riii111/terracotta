use std::{ffi::OsStr, path::Path};

use super::command::{GitCommandError, parse_error, run_git};

pub(super) enum MergeBaseError {
    NoCommonAncestor,
    Ambiguous(Vec<String>),
    Failed(GitCommandError),
}

pub(super) fn resolve_merge_base(
    repository_root: &Path,
    compare_commit: &str,
    head_commit: &str,
) -> Result<String, MergeBaseError> {
    let output = run_git(
        repository_root,
        "resolve merge-base",
        [
            OsStr::new("merge-base"),
            OsStr::new("--all"),
            OsStr::new(compare_commit),
            OsStr::new(head_commit),
        ],
    )
    .map_err(MergeBaseError::Failed)?;
    if !output.status.success() {
        let error = GitCommandError::from_output("resolve merge-base", &output);
        if output.status.code() == Some(1) && output.stderr.iter().all(u8::is_ascii_whitespace) {
            return Err(MergeBaseError::NoCommonAncestor);
        }
        return Err(MergeBaseError::Failed(error));
    }

    let bases = String::from_utf8(output.stdout)
        .map_err(|error| {
            MergeBaseError::Failed(parse_error("resolve merge-base", &error.to_string()))
        })?
        .lines()
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    match bases.as_slice() {
        [] => Err(MergeBaseError::NoCommonAncestor),
        [merge_base] => Ok(merge_base.clone()),
        _ => Err(MergeBaseError::Ambiguous(bases)),
    }
}
