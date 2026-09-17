use std::{fs, path::Path};

use crate::{app::source_location::SourceSide, infra::terraform::hcl::HclSourceFile};

use super::{
    super::command::{GitCommandError, checked_git, parse_error},
    name_status::{ChangedFile, FileChangeKind},
};

pub(super) fn load_sources(
    repository_root: &Path,
    files: &[ChangedFile],
) -> Result<(Vec<HclSourceFile>, Vec<HclSourceFile>), GitCommandError> {
    let mut before = Vec::new();
    let mut after = Vec::new();
    for file in files {
        let absolute_path = repository_root.join(&file.path);
        match file.kind {
            FileChangeKind::Added => {
                after.push(read_working_tree_source(&absolute_path)?);
            }
            FileChangeKind::Modified => {
                before.push(read_head_source(repository_root, &file.path)?);
                after.push(read_working_tree_source(&absolute_path)?);
            }
            FileChangeKind::Deleted => {
                before.push(read_head_source(repository_root, &file.path)?);
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
) -> Result<(Vec<HclSourceFile>, Vec<HclSourceFile>), GitCommandError> {
    let mut before = Vec::new();
    let mut after = Vec::new();
    for file in files {
        match file.kind {
            FileChangeKind::Added => after.push(read_revision_source(
                repository_root,
                after_revision,
                &file.path,
                SourceSide::After,
            )?),
            FileChangeKind::Modified => {
                before.push(read_revision_source(
                    repository_root,
                    before_revision,
                    &file.path,
                    SourceSide::Before,
                )?);
                after.push(read_revision_source(
                    repository_root,
                    after_revision,
                    &file.path,
                    SourceSide::After,
                )?);
            }
            FileChangeKind::Deleted => before.push(read_revision_source(
                repository_root,
                before_revision,
                &file.path,
                SourceSide::Before,
            )?),
        }
    }
    Ok((before, after))
}

pub(super) fn load_after_sources(
    repository_root: &Path,
    files: &[ChangedFile],
) -> Result<Vec<HclSourceFile>, GitCommandError> {
    files
        .iter()
        .map(|file| read_working_tree_source(&repository_root.join(&file.path)))
        .collect()
}

fn read_head_source(repository_root: &Path, path: &Path) -> Result<HclSourceFile, GitCommandError> {
    read_revision_source(repository_root, "HEAD", path, SourceSide::Before)
}

fn read_revision_source(
    repository_root: &Path,
    revision: &str,
    path: &Path,
    side: SourceSide,
) -> Result<HclSourceFile, GitCommandError> {
    let revision_path = format!("{revision}:{}", path.to_string_lossy());
    let output = checked_git(
        repository_root,
        "read Git commit source",
        ["show", revision_path.as_str()],
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
    let source = fs::read_to_string(path).map_err(|error| GitCommandError {
        operation: "read working tree source".to_owned(),
        message: format!("{}: {error}", path.display()),
    })?;
    Ok(HclSourceFile::new(path, source, SourceSide::After))
}
