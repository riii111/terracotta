use std::{fs, path::Path};

use crate::infra::CancellationToken;
use crate::{app::attribution::SourceSide, infra::terraform::hcl::HclSourceFile};

use super::{
    super::command::{GitCommandError, checked_git, parse_error},
    name_status::{ChangedFile, FileChangeKind},
};

pub(super) fn load_sources(
    repository_root: &Path,
    files: &[ChangedFile],
    cancellation: &CancellationToken,
) -> Result<(Vec<HclSourceFile>, Vec<HclSourceFile>), GitCommandError> {
    let mut before = Vec::new();
    let mut after = Vec::new();
    for file in files {
        if cancellation.is_cancelled() {
            return Err(GitCommandError::interrupted("read Git sources"));
        }
        let absolute_path = repository_root.join(&file.path);
        match file.kind {
            FileChangeKind::Added => {
                after.push(read_working_tree_source(&absolute_path)?);
            }
            FileChangeKind::Modified => {
                before.push(read_head_source(repository_root, &file.path, cancellation)?);
                after.push(read_working_tree_source(&absolute_path)?);
            }
            FileChangeKind::Deleted => {
                before.push(read_head_source(repository_root, &file.path, cancellation)?);
            }
        }
    }
    Ok((before, after))
}

pub(super) fn load_commit_sources(
    repository_root: &Path,
    files: &[ChangedFile],
    before_revision: &str,
    after_revision: &str,
    cancellation: &CancellationToken,
) -> Result<(Vec<HclSourceFile>, Vec<HclSourceFile>), GitCommandError> {
    let mut before = Vec::new();
    let mut after = Vec::new();
    for file in files {
        if cancellation.is_cancelled() {
            return Err(GitCommandError::interrupted("read Git commit sources"));
        }
        match file.kind {
            FileChangeKind::Added => after.push(read_revision_source(
                repository_root,
                after_revision,
                &file.path,
                SourceSide::After,
                cancellation,
            )?),
            FileChangeKind::Modified => {
                before.push(read_revision_source(
                    repository_root,
                    before_revision,
                    &file.path,
                    SourceSide::Before,
                    cancellation,
                )?);
                after.push(read_revision_source(
                    repository_root,
                    after_revision,
                    &file.path,
                    SourceSide::After,
                    cancellation,
                )?);
            }
            FileChangeKind::Deleted => before.push(read_revision_source(
                repository_root,
                before_revision,
                &file.path,
                SourceSide::Before,
                cancellation,
            )?),
        }
    }
    Ok((before, after))
}

pub(super) fn load_after_sources(
    repository_root: &Path,
    files: &[ChangedFile],
    cancellation: &CancellationToken,
) -> Result<Vec<HclSourceFile>, GitCommandError> {
    let mut sources = Vec::new();
    for file in files {
        if cancellation.is_cancelled() {
            return Err(GitCommandError::interrupted(
                "read Git working tree sources",
            ));
        }
        sources.push(read_working_tree_source(&repository_root.join(&file.path))?);
    }
    Ok(sources)
}

fn read_head_source(
    repository_root: &Path,
    path: &Path,
    cancellation: &CancellationToken,
) -> Result<HclSourceFile, GitCommandError> {
    read_revision_source(
        repository_root,
        "HEAD",
        path,
        SourceSide::Before,
        cancellation,
    )
}

fn read_revision_source(
    repository_root: &Path,
    revision: &str,
    path: &Path,
    side: SourceSide,
    cancellation: &CancellationToken,
) -> Result<HclSourceFile, GitCommandError> {
    let revision_path = format!("{revision}:{}", path.to_string_lossy());
    let output = checked_git(
        repository_root,
        "read Git commit source",
        ["show", revision_path.as_str()],
        cancellation,
    )?;
    let source = String::from_utf8(output.stdout).map_err(|error| {
        parse_error(
            "read Git commit source",
            &format!("source is not valid UTF-8: {error}"),
        )
    })?;
    Ok(HclSourceFile::new(repository_root.join(path), source, side))
}

fn read_working_tree_source(path: &Path) -> Result<HclSourceFile, GitCommandError> {
    let source = fs::read_to_string(path).map_err(|error| {
        parse_error(
            "read working tree source",
            &format!("{}: {error}", path.display()),
        )
    })?;
    Ok(HclSourceFile::new(path, source, SourceSide::After))
}
