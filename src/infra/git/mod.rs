#![allow(
    clippy::redundant_pub_crate,
    reason = "Git comparison types are shared only within the crate"
)]

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use crate::app::{
    attribution::SourceLineChange,
    source_location::{SourceRange, SourceSide},
};
use crate::infra::terraform::hcl::HclSourceFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComparisonBasis {
    WorkingTreeVsHead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitDiffStatus {
    Complete,
    OutsideRepository { message: String },
    HeadUnavailable { message: String },
    Failed { operation: String, message: String },
}

impl GitDiffStatus {
    #[must_use]
    pub(crate) const fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    #[must_use]
    pub(crate) fn message(&self) -> Option<&str> {
        match self {
            Self::Complete => None,
            Self::OutsideRepository { message }
            | Self::HeadUnavailable { message }
            | Self::Failed { message, .. } => Some(message),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitDiff {
    root: PathBuf,
    repository_root: Option<PathBuf>,
    basis: ComparisonBasis,
    status: GitDiffStatus,
    before: Vec<HclSourceFile>,
    after: Vec<HclSourceFile>,
    changed_lines: Vec<SourceLineChange>,
}

impl GitDiff {
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub(crate) fn repository_root(&self) -> Option<&Path> {
        self.repository_root.as_deref()
    }

    #[must_use]
    pub(crate) const fn basis(&self) -> ComparisonBasis {
        self.basis
    }

    #[must_use]
    pub(crate) const fn status(&self) -> &GitDiffStatus {
        &self.status
    }

    #[must_use]
    pub(crate) fn before(&self) -> &[HclSourceFile] {
        &self.before
    }

    #[must_use]
    pub(crate) fn after(&self) -> &[HclSourceFile] {
        &self.after
    }

    #[must_use]
    pub(crate) fn changed_lines(&self) -> &[SourceLineChange] {
        &self.changed_lines
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileChangeKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChangedFile {
    path: PathBuf,
    kind: FileChangeKind,
}

#[derive(Debug)]
struct GitCommandError {
    operation: String,
    message: String,
}

impl GitCommandError {
    fn from_spawn(operation: &str, error: &io::Error) -> Self {
        Self {
            operation: operation.to_owned(),
            message: error.to_string(),
        }
    }

    fn from_output(operation: &str, output: &Output) -> Self {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Self {
            operation: operation.to_owned(),
            message: if message.is_empty() {
                format!("git exited with status {}", output.status)
            } else {
                message
            },
        }
    }
}

pub(crate) fn collect_diff(root: &Path) -> GitDiff {
    let root = match fs::canonicalize(root) {
        Ok(root) if root.is_dir() => root,
        Ok(root) => {
            return failed_diff(
                root,
                None,
                "read Terraform root",
                "the Terraform root is not a directory",
            );
        }
        Err(error) => {
            let message = error.to_string();
            return failed_diff(root.to_owned(), None, "read Terraform root", &message);
        }
    };

    let repository_root = match discover_repository(&root) {
        Ok(repository_root) => repository_root,
        Err(DiscoveryError::OutsideRepository(message)) => {
            return unavailable_diff(root, None, GitDiffStatus::OutsideRepository { message });
        }
        Err(DiscoveryError::Failed(error)) => {
            return failed_diff(root, None, &error.operation, &error.message);
        }
    };

    let root_relative = match root.strip_prefix(&repository_root) {
        Ok(relative) => relative,
        Err(error) => {
            let message = error.to_string();
            return failed_diff(
                root,
                Some(repository_root),
                "resolve Terraform root",
                &message,
            );
        }
    };
    let root_spec = if root_relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        root_relative.to_owned()
    };

    match resolve_head(&repository_root) {
        Ok(()) => collect_head_diff(root, repository_root, &root_spec),
        Err(HeadError::Unavailable(message)) => {
            collect_without_head(root, repository_root, &root_spec, message)
        }
        Err(HeadError::Failed(error)) => failed_diff(
            root,
            Some(repository_root),
            &error.operation,
            &error.message,
        ),
    }
}

fn collect_head_diff(root: PathBuf, repository_root: PathBuf, root_spec: &Path) -> GitDiff {
    let changed_files = match changed_files(&repository_root, &root, root_spec) {
        Ok(files) => files,
        Err(error) => {
            return failed_diff(
                root,
                Some(repository_root),
                &error.operation,
                &error.message,
            );
        }
    };
    let untracked_files = match untracked_files(&repository_root, &root, root_spec) {
        Ok(files) => files,
        Err(error) => {
            return failed_diff(
                root,
                Some(repository_root),
                &error.operation,
                &error.message,
            );
        }
    };
    let changed_files = merge_untracked(changed_files, untracked_files);
    let (before, after) = match load_sources(&repository_root, &changed_files) {
        Ok(sources) => sources,
        Err(error) => {
            return failed_diff(
                root,
                Some(repository_root),
                &error.operation,
                &error.message,
            );
        }
    };
    let changed_lines = match changed_lines(&repository_root, &root, root_spec) {
        Ok(changed_lines) => changed_lines,
        Err(error) => {
            return failed_diff(
                root,
                Some(repository_root),
                &error.operation,
                &error.message,
            );
        }
    };
    let mut changed_lines = changed_lines;
    add_missing_added_line_ranges(&mut changed_lines, &after, &changed_files);
    changed_lines.sort_by(|left, right| {
        left.path()
            .cmp(right.path())
            .then_with(|| source_side_order(left.side()).cmp(&source_side_order(right.side())))
            .then_with(|| left.range().start_line().cmp(&right.range().start_line()))
    });

    GitDiff {
        root,
        repository_root: Some(repository_root),
        basis: ComparisonBasis::WorkingTreeVsHead,
        status: GitDiffStatus::Complete,
        before,
        after,
        changed_lines,
    }
}

fn collect_without_head(
    root: PathBuf,
    repository_root: PathBuf,
    root_spec: &Path,
    message: String,
) -> GitDiff {
    let files = match files_without_head(&repository_root, &root, root_spec) {
        Ok(files) => files,
        Err(error) => {
            return failed_diff(
                root,
                Some(repository_root),
                &error.operation,
                &error.message,
            );
        }
    };
    let after = match load_after_sources(&repository_root, &files) {
        Ok(after) => after,
        Err(error) => {
            return failed_diff(
                root,
                Some(repository_root),
                &error.operation,
                &error.message,
            );
        }
    };
    let changed_lines = after
        .iter()
        .filter_map(|file| {
            let line_count = file.source().lines().count();
            (line_count > 0).then(|| {
                SourceLineChange::new(
                    file.path().to_owned(),
                    SourceSide::After,
                    SourceRange::new(1, line_count),
                )
            })
        })
        .collect();

    GitDiff {
        root,
        repository_root: Some(repository_root),
        basis: ComparisonBasis::WorkingTreeVsHead,
        status: GitDiffStatus::HeadUnavailable { message },
        before: Vec::new(),
        after,
        changed_lines,
    }
}

fn discover_repository(root: &Path) -> Result<PathBuf, DiscoveryError> {
    let output = run_git(
        root,
        "discover repository",
        ["rev-parse", "--show-toplevel"],
    )
    .map_err(DiscoveryError::Failed)?;
    if !output.status.success() {
        let error = GitCommandError::from_output("discover repository", &output);
        return Err(DiscoveryError::OutsideRepository(error.message));
    }

    let repository_root = String::from_utf8(output.stdout)
        .map_err(|error| {
            DiscoveryError::Failed(GitCommandError {
                operation: "discover repository".to_owned(),
                message: error.to_string(),
            })
        })?
        .trim()
        .to_owned();
    fs::canonicalize(repository_root).map_err(|error| {
        DiscoveryError::Failed(GitCommandError {
            operation: "resolve repository root".to_owned(),
            message: error.to_string(),
        })
    })
}

fn resolve_head(repository_root: &Path) -> Result<(), HeadError> {
    let output = run_git(
        repository_root,
        "resolve HEAD",
        ["rev-parse", "--verify", "HEAD^{commit}"],
    )
    .map_err(HeadError::Failed)?;
    if output.status.success() {
        Ok(())
    } else {
        let error = GitCommandError::from_output("resolve HEAD", &output);
        Err(HeadError::Unavailable(error.message))
    }
}

fn changed_files(
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

fn untracked_files(
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

fn files_without_head(
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
        .filter(|path| is_direct_config_path(repository_root, root, path))
        .map(|path| ChangedFile {
            path,
            kind: FileChangeKind::Added,
        })
        .collect())
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

fn nul_fields(output: &[u8], operation: &str) -> Result<Vec<String>, GitCommandError> {
    output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| {
            String::from_utf8(field.to_owned()).map_err(|error| {
                parse_error(operation, &format!("Git path is not valid UTF-8: {error}"))
            })
        })
        .collect()
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

fn merge_untracked(mut tracked: Vec<ChangedFile>, untracked: Vec<ChangedFile>) -> Vec<ChangedFile> {
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

fn load_sources(
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

fn load_after_sources(
    repository_root: &Path,
    files: &[ChangedFile],
) -> Result<Vec<HclSourceFile>, GitCommandError> {
    files
        .iter()
        .map(|file| read_working_tree_source(&repository_root.join(&file.path)))
        .collect()
}

fn read_head_source(repository_root: &Path, path: &Path) -> Result<HclSourceFile, GitCommandError> {
    let revision_path = format!("HEAD:{}", path.to_string_lossy());
    let output = checked_git(
        repository_root,
        "read HEAD source",
        ["show", revision_path.as_str()],
    )?;
    let source = String::from_utf8(output.stdout).map_err(|error| {
        parse_error(
            "read HEAD source",
            &format!("source is not valid UTF-8: {error}"),
        )
    })?;
    Ok(HclSourceFile::new(
        repository_root.join(path),
        source,
        SourceSide::Before,
    ))
}

fn read_working_tree_source(path: &Path) -> Result<HclSourceFile, GitCommandError> {
    let source = fs::read_to_string(path).map_err(|error| GitCommandError {
        operation: "read working tree source".to_owned(),
        message: format!("{}: {error}", path.display()),
    })?;
    Ok(HclSourceFile::new(path, source, SourceSide::After))
}

fn changed_lines(
    repository_root: &Path,
    root: &Path,
    root_spec: &Path,
) -> Result<Vec<SourceLineChange>, GitCommandError> {
    let output = checked_git(
        repository_root,
        "read Git diff hunks",
        [
            OsStr::new("-c"),
            OsStr::new("core.quotePath=false"),
            OsStr::new("diff"),
            OsStr::new("--no-ext-diff"),
            OsStr::new("--no-renames"),
            OsStr::new("--unified=0"),
            OsStr::new("--no-color"),
            OsStr::new("HEAD"),
            OsStr::new("--"),
            root_spec.as_os_str(),
        ],
    )?;
    parse_diff_hunks(&output.stdout, repository_root, root)
}

fn parse_diff_hunks(
    output: &[u8],
    repository_root: &Path,
    root: &Path,
) -> Result<Vec<SourceLineChange>, GitCommandError> {
    let text = String::from_utf8(output.to_owned()).map_err(|error| {
        parse_error(
            "parse Git diff hunks",
            &format!("diff is not valid UTF-8: {error}"),
        )
    })?;
    let mut old_path = None;
    let mut new_path = None;
    let mut in_file_header = false;
    let mut changes = Vec::new();

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            old_path = None;
            new_path = None;
            in_file_header = true;
            continue;
        }
        if in_file_header {
            if let Some(path) = line.strip_prefix("--- ") {
                old_path = parse_patch_path(path, b'a', repository_root)?;
                continue;
            }
            if let Some(path) = line.strip_prefix("+++ ") {
                new_path = parse_patch_path(path, b'b', repository_root)?;
                continue;
            }
        }
        let Some(header) = line.strip_prefix("@@ ") else {
            continue;
        };
        in_file_header = false;
        let (old_range, new_range) = parse_hunk_ranges(header)?;
        if let Some(path) = old_path.as_deref().or(new_path.as_deref()) {
            let absolute_path = path.to_owned();
            if !is_direct_config_path(repository_root, root, path) {
                continue;
            }
            add_line_change(
                &mut changes,
                absolute_path.clone(),
                SourceSide::Before,
                old_range,
            );
            add_line_change(&mut changes, absolute_path, SourceSide::After, new_range);
        }
    }
    Ok(changes)
}

fn parse_patch_path(
    path: &str,
    prefix: u8,
    repository_root: &Path,
) -> Result<Option<PathBuf>, GitCommandError> {
    let decoded = decode_git_path(path)?;
    if decoded == "/dev/null" {
        return Ok(None);
    }
    let prefix = format!("{}/", char::from(prefix));
    let Some(relative) = decoded.strip_prefix(&prefix) else {
        return Err(parse_error(
            "parse Git diff hunks",
            &format!("unexpected diff path: {decoded}"),
        ));
    };
    Ok(Some(repository_root.join(relative)))
}

fn decode_git_path(path: &str) -> Result<String, GitCommandError> {
    let path = path.split_once('\t').map_or(path, |(path, _)| path);
    let path = path.strip_suffix('\r').unwrap_or(path);
    if !path.starts_with('"') {
        return Ok(path.to_owned());
    }
    let bytes = path.as_bytes();
    if bytes.len() < 2 || bytes[bytes.len() - 1] != b'"' {
        return Err(parse_error(
            "parse Git diff hunks",
            "unterminated quoted path",
        ));
    }
    let mut decoded = Vec::new();
    let mut index = 1;
    while index < bytes.len() - 1 {
        if bytes[index] != b'\\' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        index += 1;
        let Some(&escaped) = bytes.get(index) else {
            return Err(parse_error(
                "parse Git diff hunks",
                "unterminated path escape",
            ));
        };
        match escaped {
            b'a' => decoded.push(7),
            b'b' => decoded.push(8),
            b't' => decoded.push(b'\t'),
            b'n' => decoded.push(b'\n'),
            b'v' => decoded.push(11),
            b'f' => decoded.push(12),
            b'r' => decoded.push(b'\r'),
            b'\\' | b'"' => decoded.push(escaped),
            b'0'..=b'7' => {
                let mut value = escaped - b'0';
                let mut digits = 1;
                while digits < 3
                    && bytes
                        .get(index + 1)
                        .is_some_and(|byte| (b'0'..=b'7').contains(byte))
                {
                    index += 1;
                    value = value * 8 + bytes[index] - b'0';
                    digits += 1;
                }
                decoded.push(value);
            }
            _ => {
                return Err(parse_error("parse Git diff hunks", "unknown path escape"));
            }
        }
        index += 1;
    }
    String::from_utf8(decoded).map_err(|error| {
        parse_error(
            "parse Git diff hunks",
            &format!("path is not valid UTF-8: {error}"),
        )
    })
}

type LineRange = (usize, usize);

fn parse_hunk_ranges(header: &str) -> Result<(LineRange, LineRange), GitCommandError> {
    let ranges = header
        .split_once(" @@")
        .map_or(header, |(ranges, _)| ranges);
    let mut fields = ranges.split_whitespace();
    let old = fields.next().and_then(|field| field.strip_prefix('-'));
    let new = fields.next().and_then(|field| field.strip_prefix('+'));
    match (
        old.and_then(parse_line_range),
        new.and_then(parse_line_range),
    ) {
        (Some(old), Some(new)) => Ok((old, new)),
        _ => Err(parse_error(
            "parse Git diff hunks",
            &format!("invalid hunk header: @@ {header}"),
        )),
    }
}

fn parse_line_range(range: &str) -> Option<(usize, usize)> {
    let (start, count) = range
        .split_once(',')
        .map_or((range, "1"), |(start, count)| (start, count));
    Some((start.parse().ok()?, count.parse().ok()?))
}

fn add_line_change(
    changes: &mut Vec<SourceLineChange>,
    path: PathBuf,
    side: SourceSide,
    (start, count): (usize, usize),
) {
    if count == 0 {
        return;
    }
    let Some(end) = start.checked_add(count - 1) else {
        return;
    };
    changes.push(SourceLineChange::new(
        path,
        side,
        SourceRange::new(start, end),
    ));
}

fn add_missing_added_line_ranges(
    changes: &mut Vec<SourceLineChange>,
    after: &[HclSourceFile],
    files: &[ChangedFile],
) {
    for file in files {
        if file.kind != FileChangeKind::Added {
            continue;
        }
        let Some(source) = after
            .iter()
            .find(|source| source.path().ends_with(&file.path))
        else {
            continue;
        };
        if changes
            .iter()
            .any(|change| change.side() == SourceSide::After && change.path() == source.path())
        {
            continue;
        }
        let line_count = source.source().lines().count();
        if line_count > 0 {
            changes.push(SourceLineChange::new(
                source.path().to_owned(),
                SourceSide::After,
                SourceRange::new(1, line_count),
            ));
        }
    }
}

const fn source_side_order(side: SourceSide) -> u8 {
    match side {
        SourceSide::Before => 0,
        SourceSide::After => 1,
    }
}

fn is_direct_config_path(repository_root: &Path, root: &Path, path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == OsStr::new("tf"))
        && repository_root.join(path).parent() == Some(root)
}

fn checked_git<I, S>(directory: &Path, operation: &str, args: I) -> Result<Output, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git(directory, operation, args)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitCommandError::from_output(operation, &output))
    }
}

fn run_git<I, S>(directory: &Path, operation: &str, args: I) -> Result<Output, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .map_err(|error| GitCommandError::from_spawn(operation, &error))
}

fn parse_error(operation: &str, message: &str) -> GitCommandError {
    GitCommandError {
        operation: operation.to_owned(),
        message: message.to_owned(),
    }
}

const fn unavailable_diff(
    root: PathBuf,
    repository_root: Option<PathBuf>,
    status: GitDiffStatus,
) -> GitDiff {
    GitDiff {
        root,
        repository_root,
        basis: ComparisonBasis::WorkingTreeVsHead,
        status,
        before: Vec::new(),
        after: Vec::new(),
        changed_lines: Vec::new(),
    }
}

fn failed_diff(
    root: PathBuf,
    repository_root: Option<PathBuf>,
    operation: &str,
    message: &str,
) -> GitDiff {
    unavailable_diff(
        root,
        repository_root,
        GitDiffStatus::Failed {
            operation: operation.to_owned(),
            message: message.to_owned(),
        },
    )
}

enum DiscoveryError {
    OutsideRepository(String),
    Failed(GitCommandError),
}

enum HeadError {
    Unavailable(String),
    Failed(GitCommandError),
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);

    struct TestRepository {
        path: PathBuf,
    }

    impl TestRepository {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is before Unix epoch")
                .as_nanos();
            let suffix = format!(
                "terracotta-git-{}-{}-{}",
                std::process::id(),
                suffix,
                NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(suffix);
            fs::create_dir(&path).expect("create temporary repository");
            git(&path, &["init", "--quiet"]);
            git(&path, &["config", "user.email", "test@example.com"]);
            git(&path, &["config", "user.name", "Terracotta Test"]);
            Self { path }
        }

        fn commit(&self, message: &str) {
            git(&self.path, &["add", "."]);
            git(&self.path, &["commit", "--quiet", "-m", message]);
        }
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).expect("remove temporary repository");
        }
    }

    fn git(repository: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .expect("run git in test repository");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write(repository: &TestRepository, relative: &str, source: &str) {
        let path = repository.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create source parent");
        }
        fs::write(path, source).expect("write source");
    }

    fn source_names(files: &[HclSourceFile]) -> Vec<&Path> {
        files.iter().map(HclSourceFile::path).collect()
    }

    #[test]
    fn combines_staged_and_unstaged_changes_against_head() {
        let repository = TestRepository::new();
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"before\"\n}\n",
        );
        repository.commit("initial");
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"after\"\n  extra = true\n}\n",
        );
        git(&repository.path, &["add", "main.tf"]);
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"after\"\n  extra = false\n}\n",
        );

        let result = collect_diff(&repository.path);

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert_eq!(result.basis(), ComparisonBasis::WorkingTreeVsHead);
        assert_eq!(
            result.before()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"before\"\n}\n"
        );
        assert_eq!(
            result.after()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"after\"\n  extra = false\n}\n"
        );
        assert_eq!(result.changed_lines().len(), 2);
        assert_eq!(result.changed_lines()[0].side(), SourceSide::Before);
        assert_eq!(result.changed_lines()[0].range(), SourceRange::new(2, 2));
        assert_eq!(result.changed_lines()[1].side(), SourceSide::After);
        assert_eq!(result.changed_lines()[1].range(), SourceRange::new(2, 3));
    }

    #[test]
    fn ignores_changes_that_are_restored_to_head() {
        let repository = TestRepository::new();
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");
        repository.commit("initial");
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");

        let result = collect_diff(&repository.path);

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert!(result.before().is_empty());
        assert!(result.after().is_empty());
        assert!(result.changed_lines().is_empty());
    }

    #[test]
    fn treats_rename_as_deleted_before_and_added_after() {
        let repository = TestRepository::new();
        write(
            &repository,
            "old name 日本.tf",
            "resource \"example\" \"old\" {}\n",
        );
        repository.commit("initial");
        fs::rename(
            repository.path.join("old name 日本.tf"),
            repository.path.join("new name 日本.tf"),
        )
        .expect("rename source");
        git(&repository.path, &["add", "-A"]);

        let result = collect_diff(&repository.path);
        let repository_root = result.root().to_owned();

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert_eq!(
            source_names(result.before()),
            vec![repository_root.join("old name 日本.tf").as_path()]
        );
        assert_eq!(
            source_names(result.after()),
            vec![repository_root.join("new name 日本.tf").as_path()]
        );
        assert_eq!(result.changed_lines().len(), 2);
        assert!(result.changed_lines().iter().any(|change| {
            change.path() == repository_root.join("old name 日本.tf")
                && change.side() == SourceSide::Before
        }));
        assert!(result.changed_lines().iter().any(|change| {
            change.path() == repository_root.join("new name 日本.tf")
                && change.side() == SourceSide::After
        }));
    }

    #[test]
    fn includes_non_ignored_untracked_tf_and_excludes_ignored_tf() {
        let repository = TestRepository::new();
        write(&repository, ".gitignore", "ignored.tf\n");
        write(
            &repository,
            "tracked.tf",
            "resource \"example\" \"one\" {}\n",
        );
        repository.commit("initial");
        write(
            &repository,
            "new file.tf",
            "resource \"example\" \"new\" {}\n",
        );
        write(
            &repository,
            "ignored.tf",
            "resource \"example\" \"ignored\" {}\n",
        );

        let result = collect_diff(&repository.path);
        let repository_root = result.root().to_owned();

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert_eq!(
            source_names(result.after()),
            vec![repository_root.join("new file.tf").as_path()]
        );
        assert_eq!(result.changed_lines().len(), 1);
        assert_eq!(result.changed_lines()[0].range(), SourceRange::new(1, 1));
    }

    #[test]
    fn only_collects_changes_directly_under_the_terraform_root() {
        let repository = TestRepository::new();
        write(
            &repository,
            "infra/prod/main.tf",
            "resource \"example\" \"one\" {}\n",
        );
        write(
            &repository,
            "outside.tf",
            "resource \"example\" \"outside\" {}\n",
        );
        repository.commit("initial");
        write(
            &repository,
            "infra/prod/main.tf",
            "resource \"example\" \"one\" {\n  value = true\n}\n",
        );
        write(
            &repository,
            "outside.tf",
            "resource \"example\" \"outside\" {\n  value = true\n}\n",
        );

        let result = collect_diff(&repository.path.join("infra/prod"));
        let root = result.root().to_owned();

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert_eq!(result.before().len(), 1);
        assert_eq!(result.after().len(), 1);
        assert!(
            result
                .changed_lines()
                .iter()
                .all(|change| { change.path() == root.join("main.tf") })
        );
    }

    #[test]
    fn distinguishes_outside_repository_and_missing_head() {
        let outside = std::env::temp_dir().join(format!(
            "terracotta-outside-{}",
            NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&outside).expect("create outside directory");
        let outside_result = collect_diff(&outside);
        fs::remove_dir(&outside).expect("remove outside directory");

        assert!(matches!(
            outside_result.status(),
            GitDiffStatus::OutsideRepository { .. }
        ));

        let repository = TestRepository::new();
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");
        let no_head_result = collect_diff(&repository.path);

        assert!(matches!(
            no_head_result.status(),
            GitDiffStatus::HeadUnavailable { .. }
        ));
        assert_eq!(no_head_result.after().len(), 1);
        assert_eq!(no_head_result.changed_lines().len(), 1);
    }

    #[test]
    fn reports_root_read_failure_separately() {
        let repository = TestRepository::new();
        let file = repository.path.join("not-a-root");
        fs::write(&file, "not a directory").expect("write invalid root");

        let result = collect_diff(&file);

        assert!(matches!(result.status(), GitDiffStatus::Failed { .. }));
        assert!(!result.status().is_complete());
        assert!(result.status().message().is_some());
    }
}
