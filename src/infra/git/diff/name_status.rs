use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use super::super::command::{GitCommandError, checked_git, nul_fields, parse_error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FileChangeKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChangedFile {
    pub(super) path: PathBuf,
    pub(super) kind: FileChangeKind,
}

pub(super) fn changed_files(
    repository_root: &Path,
    root: &Path,
    root_spec: &Path,
) -> Result<Vec<ChangedFile>, GitCommandError> {
    let output = checked_git(
        repository_root,
        "read Git changes",
        [
            OsStr::new("diff"),
            OsStr::new("--name-status"),
            OsStr::new("--no-renames"),
            OsStr::new("-z"),
            OsStr::new("HEAD"),
            OsStr::new("--"),
            root_spec.as_os_str(),
        ],
    )?;
    parse_name_status(&output.stdout, repository_root, root)
}

pub(super) fn changed_files_between(
    repository_root: &Path,
    root: &Path,
    root_spec: &Path,
    before_revision: &str,
    after_revision: &str,
) -> Result<Vec<ChangedFile>, GitCommandError> {
    let output = checked_git(
        repository_root,
        "read Git commit changes",
        [
            OsStr::new("diff"),
            OsStr::new("--name-status"),
            OsStr::new("--no-renames"),
            OsStr::new("-z"),
            OsStr::new(before_revision),
            OsStr::new(after_revision),
            OsStr::new("--"),
            root_spec.as_os_str(),
        ],
    )?;
    parse_name_status(&output.stdout, repository_root, root)
}

pub(super) fn untracked_files(
    repository_root: &Path,
    root: &Path,
    root_spec: &Path,
) -> Result<Vec<ChangedFile>, GitCommandError> {
    let output = checked_git(
        repository_root,
        "read untracked Git files",
        [
            OsStr::new("ls-files"),
            OsStr::new("--others"),
            OsStr::new("--exclude-standard"),
            OsStr::new("-z"),
            OsStr::new("--"),
            root_spec.as_os_str(),
        ],
    )?;
    parse_untracked_files(&output.stdout, repository_root, root)
}

pub(super) fn files_without_head(
    repository_root: &Path,
    root: &Path,
    root_spec: &Path,
) -> Result<Vec<ChangedFile>, GitCommandError> {
    let output = checked_git(
        repository_root,
        "read files without HEAD",
        [
            OsStr::new("ls-files"),
            OsStr::new("--cached"),
            OsStr::new("--others"),
            OsStr::new("--exclude-standard"),
            OsStr::new("-z"),
            OsStr::new("--"),
            root_spec.as_os_str(),
        ],
    )?;
    let paths = parse_nul_paths(&output.stdout, "read files without HEAD")?;
    Ok(paths
        .into_iter()
        .filter(|path| {
            is_direct_config_path(repository_root, root, path)
                && repository_root.join(path).is_file()
        })
        .map(|path| ChangedFile {
            path,
            kind: FileChangeKind::Added,
        })
        .collect())
}

pub(super) fn merge_untracked(
    mut tracked: Vec<ChangedFile>,
    untracked: Vec<ChangedFile>,
) -> Vec<ChangedFile> {
    for file in untracked {
        if let Some(existing) = tracked.iter_mut().find(|item| item.path == file.path) {
            existing.kind = merge_file_change(existing.kind, file.kind);
        } else {
            tracked.push(file);
        }
    }
    tracked.sort_by(|left, right| left.path.cmp(&right.path));
    tracked
}

fn parse_name_status(
    output: &[u8],
    repository_root: &Path,
    root: &Path,
) -> Result<Vec<ChangedFile>, GitCommandError> {
    let fields = nul_fields(output, "parse Git changes")?;
    let mut files = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let status = fields[index].as_str();
        index += 1;
        let Some(path) = fields.get(index) else {
            return Err(parse_error(
                "parse Git changes",
                "missing path after status",
            ));
        };
        index += 1;

        if matches!(status.as_bytes().first(), Some(b'R' | b'C')) {
            let Some(new_path) = fields.get(index) else {
                return Err(parse_error(
                    "parse Git changes",
                    "missing destination path for rename",
                ));
            };
            index += 1;
            add_changed_file(
                &mut files,
                repository_root,
                root,
                path,
                FileChangeKind::Deleted,
            );
            add_changed_file(
                &mut files,
                repository_root,
                root,
                new_path,
                FileChangeKind::Added,
            );
            continue;
        }

        let kind = match status.as_bytes().first() {
            Some(b'A') => FileChangeKind::Added,
            Some(b'D') => FileChangeKind::Deleted,
            Some(_) => FileChangeKind::Modified,
            None => return Err(parse_error("parse Git changes", "empty status")),
        };
        add_changed_file(&mut files, repository_root, root, path, kind);
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn parse_untracked_files(
    output: &[u8],
    repository_root: &Path,
    root: &Path,
) -> Result<Vec<ChangedFile>, GitCommandError> {
    let paths = parse_nul_paths(output, "parse untracked Git files")?;
    let mut files = paths
        .into_iter()
        .filter_map(|path| {
            is_direct_config_path(repository_root, root, &path).then_some(ChangedFile {
                path,
                kind: FileChangeKind::Added,
            })
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn parse_nul_paths(output: &[u8], operation: &str) -> Result<Vec<PathBuf>, GitCommandError> {
    nul_fields(output, operation).map(|fields| fields.into_iter().map(PathBuf::from).collect())
}

fn add_changed_file(
    files: &mut Vec<ChangedFile>,
    repository_root: &Path,
    root: &Path,
    path: &str,
    kind: FileChangeKind,
) {
    if !is_direct_config_path(repository_root, root, Path::new(path)) {
        return;
    }
    if let Some(existing) = files.iter_mut().find(|file| file.path == Path::new(path)) {
        existing.kind = merge_file_change(existing.kind, kind);
    } else {
        files.push(ChangedFile {
            path: PathBuf::from(path),
            kind,
        });
    }
}

const fn merge_file_change(left: FileChangeKind, right: FileChangeKind) -> FileChangeKind {
    match (left, right) {
        (FileChangeKind::Added, FileChangeKind::Deleted)
        | (FileChangeKind::Deleted, FileChangeKind::Added) => FileChangeKind::Modified,
        (FileChangeKind::Added, _) | (_, FileChangeKind::Added) => FileChangeKind::Added,
        (FileChangeKind::Deleted, _) | (_, FileChangeKind::Deleted) => FileChangeKind::Deleted,
        _ => FileChangeKind::Modified,
    }
}

pub(super) fn is_direct_config_path(repository_root: &Path, root: &Path, path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == OsStr::new("tf"))
        && repository_root.join(path).parent() == Some(root)
}
