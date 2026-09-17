#![allow(
    clippy::redundant_pub_crate,
    reason = "Git comparison types are shared only within the crate"
)]

use std::{
    ffi::{OsStr, OsString},
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
    HeadVsMergeBase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitDiffStatus {
    Complete,
    OutsideRepository {
        message: String,
    },
    HeadUnavailable {
        message: String,
    },
    CompareRefUnavailable {
        reference: String,
        message: String,
    },
    AmbiguousCompareRef {
        reference: String,
        message: String,
    },
    NoCommonAncestor {
        reference: String,
        message: String,
    },
    AmbiguousMergeBase {
        reference: String,
        merge_bases: Vec<String>,
        message: String,
    },
    Failed {
        operation: String,
        message: String,
    },
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
            | Self::CompareRefUnavailable { message, .. }
            | Self::AmbiguousCompareRef { message, .. }
            | Self::NoCommonAncestor { message, .. }
            | Self::AmbiguousMergeBase { message, .. }
            | Self::Failed { message, .. } => Some(message),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComparisonMetadata {
    basis: ComparisonBasis,
    compare_ref: Option<String>,
    resolved_commit: Option<String>,
    head_commit: Option<String>,
    merge_base: Option<String>,
}

impl ComparisonMetadata {
    const fn working_tree() -> Self {
        Self {
            basis: ComparisonBasis::WorkingTreeVsHead,
            compare_ref: None,
            resolved_commit: None,
            head_commit: None,
            merge_base: None,
        }
    }

    fn for_compare_ref(compare_ref: &str) -> Self {
        Self {
            basis: ComparisonBasis::HeadVsMergeBase,
            compare_ref: Some(compare_ref.to_owned()),
            resolved_commit: None,
            head_commit: None,
            merge_base: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitDiff {
    root: PathBuf,
    repository_root: Option<PathBuf>,
    comparison: ComparisonMetadata,
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
        self.comparison.basis
    }

    #[must_use]
    pub(crate) fn compare_ref(&self) -> Option<&str> {
        self.comparison.compare_ref.as_deref()
    }

    #[must_use]
    pub(crate) fn resolved_commit(&self) -> Option<&str> {
        self.comparison.resolved_commit.as_deref()
    }

    #[must_use]
    pub(crate) fn head_commit(&self) -> Option<&str> {
        self.comparison.head_commit.as_deref()
    }

    #[must_use]
    pub(crate) fn merge_base(&self) -> Option<&str> {
        self.comparison.merge_base.as_deref()
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

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ConfigurationSnapshot {
    files: Vec<ConfigurationFile>,
    issues: Vec<String>,
}

impl std::fmt::Debug for ConfigurationSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigurationSnapshot")
            .field(
                "files",
                &self.files.iter().map(|file| &file.path).collect::<Vec<_>>(),
            )
            .field("issues", &self.issues)
            .finish()
    }
}

impl ConfigurationSnapshot {
    #[must_use]
    pub(crate) fn changed_paths(&self, other: &Self) -> Vec<PathBuf> {
        let mut paths = self
            .files
            .iter()
            .map(|file| file.path.clone())
            .chain(other.files.iter().map(|file| file.path.clone()))
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        paths
            .into_iter()
            .filter(|path| self.file_contents(path) != other.file_contents(path))
            .collect()
    }

    #[must_use]
    pub(crate) fn issues(&self) -> &[String] {
        &self.issues
    }

    #[must_use]
    pub(crate) fn differing_source_paths(
        &self,
        before: &[HclSourceFile],
        after: &[HclSourceFile],
    ) -> Vec<PathBuf> {
        let mut paths = before
            .iter()
            .map(|source| source.path().to_owned())
            .chain(after.iter().map(|source| source.path().to_owned()))
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        paths
            .into_iter()
            .filter(|path| {
                let expected = after
                    .iter()
                    .find(|source| source.path() == path)
                    .map(|source| source.source().as_bytes());
                self.file_contents(path) != expected
            })
            .collect()
    }

    fn file_contents(&self, path: &Path) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.contents.as_slice())
    }
}

#[derive(Clone, PartialEq, Eq)]
struct ConfigurationFile {
    path: PathBuf,
    contents: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigurationComparison {
    changed_paths: Vec<PathBuf>,
    issues: Vec<String>,
}

impl ConfigurationComparison {
    #[must_use]
    pub(crate) fn changed_paths(&self) -> &[PathBuf] {
        &self.changed_paths
    }

    #[must_use]
    pub(crate) fn issues(&self) -> &[String] {
        &self.issues
    }
}

pub(crate) fn capture_working_tree_configuration(root: &Path) -> ConfigurationSnapshot {
    let mut snapshot = ConfigurationSnapshot {
        files: Vec::new(),
        issues: Vec::new(),
    };
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            snapshot.issues.push(format!(
                "failed to read Terraform configuration directory {}: {error}",
                root.display()
            ));
            return snapshot;
        }
    };

    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_configuration_file(path))
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        match fs::read(&path) {
            Ok(contents) => snapshot.files.push(ConfigurationFile { path, contents }),
            Err(error) => snapshot.issues.push(format!(
                "failed to read Terraform configuration {}: {error}",
                path.display()
            )),
        }
    }
    snapshot
}

pub(crate) fn capture_revision_configuration(
    repository_root: &Path,
    root: &Path,
    revision: &str,
) -> ConfigurationSnapshot {
    let mut snapshot = ConfigurationSnapshot {
        files: Vec::new(),
        issues: Vec::new(),
    };
    let root_relative = match root.strip_prefix(repository_root) {
        Ok(relative) => relative,
        Err(error) => {
            snapshot.issues.push(format!(
                "failed to resolve Terraform root in Git repository: {error}"
            ));
            return snapshot;
        }
    };
    let root_spec = if root_relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        root_relative.to_owned()
    };
    let root_spec = root_spec.as_os_str();
    let output = match checked_git(
        repository_root,
        "list Git Terraform configuration",
        [
            OsStr::new("ls-tree"),
            OsStr::new("-r"),
            OsStr::new("-z"),
            OsStr::new("--name-only"),
            OsStr::new(revision),
            OsStr::new("--"),
            root_spec,
        ],
    ) {
        Ok(output) => output,
        Err(error) => {
            snapshot
                .issues
                .push(format!("{}: {}", error.operation, error.message));
            return snapshot;
        }
    };
    let paths = match nul_fields(&output.stdout, "parse Git Terraform configuration") {
        Ok(paths) => paths,
        Err(error) => {
            snapshot
                .issues
                .push(format!("{}: {}", error.operation, error.message));
            return snapshot;
        }
    };
    for relative in paths {
        let relative = PathBuf::from(relative);
        if !is_configuration_file(&relative)
            || !is_direct_repository_path(repository_root, root, &relative)
        {
            continue;
        }
        let revision_path = format!("{revision}:{}", relative.to_string_lossy());
        match checked_git(
            repository_root,
            "read Git Terraform configuration",
            [OsStr::new("show"), OsStr::new(revision_path.as_str())],
        ) {
            Ok(output) => snapshot.files.push(ConfigurationFile {
                path: repository_root.join(&relative),
                contents: output.stdout,
            }),
            Err(error) => snapshot.issues.push(format!(
                "{} {}: {}",
                error.operation,
                relative.display(),
                error.message
            )),
        }
    }
    snapshot
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    snapshot
}

pub(crate) fn compare_configuration(
    diff: &GitDiff,
    working_tree: &ConfigurationSnapshot,
) -> ConfigurationComparison {
    let Some(repository_root) = diff.repository_root() else {
        return ConfigurationComparison {
            changed_paths: Vec::new(),
            issues: vec!["Git repository root is unavailable".to_owned()],
        };
    };
    let revision = diff.head_commit();
    let Some(revision) = revision else {
        return ConfigurationComparison {
            changed_paths: Vec::new(),
            issues: vec!["Git comparison commit is unavailable".to_owned()],
        };
    };
    let baseline = capture_revision_configuration(repository_root, diff.root(), revision);
    let mut issues = baseline.issues.clone();
    issues.extend(working_tree.issues.iter().cloned());
    ConfigurationComparison {
        changed_paths: baseline.changed_paths(working_tree),
        issues,
    }
}

pub(crate) fn compare_commit_configurations(diff: &GitDiff) -> ConfigurationComparison {
    let Some(repository_root) = diff.repository_root() else {
        return ConfigurationComparison {
            changed_paths: Vec::new(),
            issues: vec!["Git repository root is unavailable".to_owned()],
        };
    };
    let (Some(before), Some(after)) = (diff.merge_base(), diff.head_commit()) else {
        return ConfigurationComparison {
            changed_paths: Vec::new(),
            issues: vec!["Git comparison commits are unavailable".to_owned()],
        };
    };
    let before = capture_revision_configuration(repository_root, diff.root(), before);
    let after = capture_revision_configuration(repository_root, diff.root(), after);
    let mut issues = before.issues.clone();
    issues.extend(after.issues.iter().cloned());
    ConfigurationComparison {
        changed_paths: before.changed_paths(&after),
        issues,
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
        Ok(head_commit) => collect_head_diff(root, repository_root, &root_spec, head_commit),
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

pub(crate) fn collect_diff_against_ref(root: &Path, compare_ref: &str) -> GitDiff {
    let comparison = ComparisonMetadata::for_compare_ref(compare_ref);
    let root = match fs::canonicalize(root) {
        Ok(root) if root.is_dir() => root,
        Ok(root) => {
            return failed_diff_with_comparison(
                root,
                None,
                "read Terraform root",
                "the Terraform root is not a directory",
                comparison,
            );
        }
        Err(error) => {
            let message = error.to_string();
            return failed_diff_with_comparison(
                root.to_owned(),
                None,
                "read Terraform root",
                &message,
                comparison,
            );
        }
    };

    let repository_root = match discover_repository(&root) {
        Ok(repository_root) => repository_root,
        Err(DiscoveryError::OutsideRepository(message)) => {
            return unavailable_diff_with_comparison(
                root,
                None,
                GitDiffStatus::OutsideRepository { message },
                comparison,
            );
        }
        Err(DiscoveryError::Failed(error)) => {
            return failed_diff_with_comparison(
                root,
                None,
                &error.operation,
                &error.message,
                comparison,
            );
        }
    };

    let root_relative = match root.strip_prefix(&repository_root) {
        Ok(relative) => relative,
        Err(error) => {
            let message = error.to_string();
            return failed_diff_with_comparison(
                root,
                Some(repository_root),
                "resolve Terraform root",
                &message,
                comparison,
            );
        }
    };
    let root_spec = if root_relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        root_relative.to_owned()
    };

    let resolution = match resolve_comparison(&repository_root, compare_ref, comparison) {
        Ok(resolution) => resolution,
        Err(error) => return (*error).into_diff(root, Some(repository_root)),
    };

    collect_commit_diff(
        root,
        repository_root,
        &root_spec,
        &resolution.before_revision,
        &resolution.after_revision,
        resolution.comparison,
    )
}

fn collect_commit_diff(
    root: PathBuf,
    repository_root: PathBuf,
    root_spec: &Path,
    before_revision: &str,
    after_revision: &str,
    comparison: ComparisonMetadata,
) -> GitDiff {
    let changed_files = match changed_files_between(
        &repository_root,
        &root,
        root_spec,
        before_revision,
        after_revision,
    ) {
        Ok(files) => files,
        Err(error) => {
            return failed_diff_with_comparison(
                root,
                Some(repository_root),
                &error.operation,
                &error.message,
                comparison,
            );
        }
    };
    let (before, after) = match load_commit_sources(
        &repository_root,
        &changed_files,
        before_revision,
        after_revision,
    ) {
        Ok(sources) => sources,
        Err(error) => {
            return failed_diff_with_comparison(
                root,
                Some(repository_root),
                &error.operation,
                &error.message,
                comparison,
            );
        }
    };
    let mut changed_lines = match changed_lines_between(
        &repository_root,
        &root,
        root_spec,
        before_revision,
        after_revision,
    ) {
        Ok(changed_lines) => changed_lines,
        Err(error) => {
            return failed_diff_with_comparison(
                root,
                Some(repository_root),
                &error.operation,
                &error.message,
                comparison,
            );
        }
    };
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
        comparison,
        status: GitDiffStatus::Complete,
        before,
        after,
        changed_lines,
    }
}

fn collect_head_diff(
    root: PathBuf,
    repository_root: PathBuf,
    root_spec: &Path,
    head_commit: String,
) -> GitDiff {
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
        comparison: ComparisonMetadata {
            basis: ComparisonBasis::WorkingTreeVsHead,
            compare_ref: None,
            resolved_commit: None,
            head_commit: Some(head_commit),
            merge_base: None,
        },
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
        comparison: ComparisonMetadata::working_tree(),
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

fn resolve_head(repository_root: &Path) -> Result<String, HeadError> {
    let output = run_git(
        repository_root,
        "resolve HEAD",
        ["rev-parse", "--verify", "HEAD^{commit}"],
    )
    .map_err(HeadError::Failed)?;
    if output.status.success() {
        single_commit(&output, "resolve HEAD").map_err(HeadError::Failed)
    } else {
        let error = GitCommandError::from_output("resolve HEAD", &output);
        Err(HeadError::Unavailable(error.message))
    }
}

struct ResolvedComparison {
    comparison: ComparisonMetadata,
    before_revision: String,
    after_revision: String,
}

struct ComparisonResolutionError {
    comparison: ComparisonMetadata,
    failure: ComparisonResolutionFailure,
}

enum ComparisonResolutionFailure {
    HeadUnavailable(String),
    HeadFailed(GitCommandError),
    CompareRefUnavailable(String),
    AmbiguousCompareRef(String),
    CompareRefFailed(GitCommandError),
    NoCommonAncestor,
    AmbiguousMergeBase(Vec<String>),
    MergeBaseFailed(GitCommandError),
}

impl ComparisonResolutionError {
    fn into_diff(self, root: PathBuf, repository_root: Option<PathBuf>) -> GitDiff {
        let Some(reference) = self.comparison.compare_ref.clone() else {
            return failed_diff_with_comparison(
                root,
                repository_root,
                "resolve comparison",
                "comparison ref is missing",
                self.comparison,
            );
        };
        let status = match self.failure {
            ComparisonResolutionFailure::HeadUnavailable(message) => {
                GitDiffStatus::HeadUnavailable { message }
            }
            ComparisonResolutionFailure::HeadFailed(error)
            | ComparisonResolutionFailure::CompareRefFailed(error)
            | ComparisonResolutionFailure::MergeBaseFailed(error) => GitDiffStatus::Failed {
                operation: error.operation,
                message: error.message,
            },
            ComparisonResolutionFailure::CompareRefUnavailable(message) => {
                GitDiffStatus::CompareRefUnavailable { reference, message }
            }
            ComparisonResolutionFailure::AmbiguousCompareRef(message) => {
                GitDiffStatus::AmbiguousCompareRef { reference, message }
            }
            ComparisonResolutionFailure::NoCommonAncestor => GitDiffStatus::NoCommonAncestor {
                reference,
                message: "the comparison ref and HEAD have no common ancestor".to_owned(),
            },
            ComparisonResolutionFailure::AmbiguousMergeBase(merge_bases) => {
                let message = format!(
                    "the comparison basis has multiple merge-bases: {}",
                    merge_bases.join(", ")
                );
                GitDiffStatus::AmbiguousMergeBase {
                    reference,
                    merge_bases,
                    message,
                }
            }
        };
        unavailable_diff_with_comparison(root, repository_root, status, self.comparison)
    }
}

fn resolve_comparison(
    repository_root: &Path,
    compare_ref: &str,
    mut comparison: ComparisonMetadata,
) -> Result<ResolvedComparison, Box<ComparisonResolutionError>> {
    let head_commit = resolve_head(repository_root).map_err(|error| {
        Box::new(ComparisonResolutionError {
            comparison: comparison.clone(),
            failure: match error {
                HeadError::Unavailable(message) => {
                    ComparisonResolutionFailure::HeadUnavailable(message)
                }
                HeadError::Failed(error) => ComparisonResolutionFailure::HeadFailed(error),
            },
        })
    })?;
    comparison.head_commit = Some(head_commit.clone());

    let resolved_commit = resolve_compare_ref(repository_root, compare_ref).map_err(|error| {
        Box::new(ComparisonResolutionError {
            comparison: comparison.clone(),
            failure: match error {
                CompareRefError::Unavailable(message) => {
                    ComparisonResolutionFailure::CompareRefUnavailable(message)
                }
                CompareRefError::Ambiguous(message) => {
                    ComparisonResolutionFailure::AmbiguousCompareRef(message)
                }
                CompareRefError::Failed(error) => {
                    ComparisonResolutionFailure::CompareRefFailed(error)
                }
            },
        })
    })?;
    comparison.resolved_commit = Some(resolved_commit.clone());

    let merge_base =
        resolve_merge_base(repository_root, &resolved_commit, &head_commit).map_err(|error| {
            Box::new(ComparisonResolutionError {
                comparison: comparison.clone(),
                failure: match error {
                    MergeBaseError::NoCommonAncestor => {
                        ComparisonResolutionFailure::NoCommonAncestor
                    }
                    MergeBaseError::Ambiguous(merge_bases) => {
                        ComparisonResolutionFailure::AmbiguousMergeBase(merge_bases)
                    }
                    MergeBaseError::Failed(error) => {
                        ComparisonResolutionFailure::MergeBaseFailed(error)
                    }
                },
            })
        })?;
    comparison.merge_base = Some(merge_base.clone());

    Ok(ResolvedComparison {
        comparison,
        before_revision: merge_base,
        after_revision: head_commit,
    })
}

fn resolve_compare_ref(
    repository_root: &Path,
    compare_ref: &str,
) -> Result<String, CompareRefError> {
    resolve_compare_ref_with_env(repository_root, compare_ref, &[])
}

fn resolve_compare_ref_with_env(
    repository_root: &Path,
    compare_ref: &str,
    environment: &[(&str, &str)],
) -> Result<String, CompareRefError> {
    if compare_ref.is_empty() {
        return Err(CompareRefError::Unavailable(
            "the comparison ref is empty".to_owned(),
        ));
    }

    if compare_ref.starts_with("refs/") {
        return resolve_commit_revision(repository_root, compare_ref, environment)
            .map_err(|error| CompareRefError::Unavailable(error.message));
    }

    if !is_valid_comparison_ref_name(repository_root, compare_ref, environment)? {
        return resolve_commit_revision(repository_root, compare_ref, environment)
            .map_err(|error| CompareRefError::Unavailable(error.message));
    }

    if let Some(commit) = resolve_git_directory_ref(repository_root, compare_ref, environment)? {
        return Ok(commit);
    }

    let candidates = comparison_ref_candidates(repository_root, compare_ref, environment)?;
    if !candidates.is_empty() {
        if candidates.len() > 1 {
            return Err(CompareRefError::Ambiguous(format!(
                "the comparison ref is ambiguous; candidates: {}",
                candidates.join(", ")
            )));
        }
        return resolve_commit_revision(repository_root, &candidates[0], environment)
            .map_err(|error| CompareRefError::Unavailable(error.message));
    }

    resolve_commit_revision(repository_root, compare_ref, environment)
        .map_err(|error| CompareRefError::Unavailable(error.message))
}

fn comparison_ref_candidates(
    repository_root: &Path,
    compare_ref: &str,
    environment: &[(&str, &str)],
) -> Result<Vec<String>, CompareRefError> {
    let patterns = [
        format!("refs/{compare_ref}"),
        format!("refs/tags/{compare_ref}"),
        format!("refs/heads/{compare_ref}"),
        format!("refs/remotes/{compare_ref}"),
        format!("refs/remotes/{compare_ref}/HEAD"),
    ];
    let mut args = vec![
        OsString::from("for-each-ref"),
        OsString::from("--format=%(refname)"),
        OsString::from("--"),
    ];
    args.extend(patterns.into_iter().map(OsString::from));
    let output = run_git_with_env(
        repository_root,
        "list comparison ref candidates",
        args,
        environment,
    )
    .map_err(CompareRefError::Failed)?;
    if !output.status.success() {
        return Err(CompareRefError::Failed(GitCommandError::from_output(
            "list comparison ref candidates",
            &output,
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|error| {
            CompareRefError::Failed(parse_error(
                "list comparison ref candidates",
                &error.to_string(),
            ))
        })
        .map(|output| {
            output
                .lines()
                .map(str::trim)
                .filter(|candidate| !candidate.is_empty())
                .map(str::to_owned)
                .collect()
        })
}

fn is_valid_comparison_ref_name(
    repository_root: &Path,
    compare_ref: &str,
    environment: &[(&str, &str)],
) -> Result<bool, CompareRefError> {
    let output = run_git_with_env(
        repository_root,
        "validate comparison ref",
        [
            OsStr::new("check-ref-format"),
            OsStr::new("--allow-onelevel"),
            OsStr::new(compare_ref),
        ],
        environment,
    )
    .map_err(CompareRefError::Failed)?;
    Ok(output.status.success())
}

fn resolve_git_directory_ref(
    repository_root: &Path,
    compare_ref: &str,
    environment: &[(&str, &str)],
) -> Result<Option<String>, CompareRefError> {
    let output = run_git_with_env(
        repository_root,
        "resolve Git directory ref path",
        [
            OsStr::new("rev-parse"),
            OsStr::new("--git-path"),
            OsStr::new(compare_ref),
        ],
        environment,
    )
    .map_err(CompareRefError::Failed)?;
    if !output.status.success() {
        return Err(CompareRefError::Failed(GitCommandError::from_output(
            "resolve Git directory ref path",
            &output,
        )));
    }

    let path = String::from_utf8(output.stdout)
        .map_err(|error| {
            CompareRefError::Failed(parse_error(
                "resolve Git directory ref path",
                &error.to_string(),
            ))
        })?
        .trim()
        .to_owned();
    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        repository_root.join(path)
    };
    if !path.is_file() {
        return Ok(None);
    }

    let content = fs::read(&path).map_err(|error| {
        CompareRefError::Failed(parse_error("read Git directory ref", &error.to_string()))
    })?;
    if !looks_like_git_directory_ref(compare_ref, &content) {
        return Ok(None);
    }

    resolve_commit_revision(repository_root, compare_ref, environment)
        .map(Some)
        .map_err(|error| CompareRefError::Unavailable(error.message))
}

fn looks_like_git_directory_ref(compare_ref: &str, content: &[u8]) -> bool {
    if compare_ref == "HEAD"
        && content
            .strip_prefix(b"ref: refs/")
            .is_some_and(|ref_name| !ref_name.is_empty())
    {
        return true;
    }

    let Ok(content) = std::str::from_utf8(content) else {
        return false;
    };
    let lines = content.lines().filter(|line| !line.is_empty());
    let mut has_line = false;
    for line in lines {
        let Some(object_id) = line.split_whitespace().next() else {
            return false;
        };
        if !is_object_id(object_id) {
            return false;
        }
        has_line = true;
    }
    has_line
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn resolve_commit_revision(
    repository_root: &Path,
    revision: &str,
    environment: &[(&str, &str)],
) -> Result<String, GitCommandError> {
    let revision = format!("{revision}^{{commit}}");
    let output = run_git_with_env(
        repository_root,
        "resolve comparison ref",
        [
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new("--quiet"),
            OsStr::new("--end-of-options"),
            OsStr::new(&revision),
        ],
        environment,
    )?;
    if output.status.success() {
        single_commit(&output, "resolve comparison ref")
    } else {
        Err(GitCommandError::from_output(
            "resolve comparison ref",
            &output,
        ))
    }
}

fn resolve_merge_base(
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

fn single_commit(output: &Output, operation: &str) -> Result<String, GitCommandError> {
    let commits = String::from_utf8(output.stdout.clone())
        .map_err(|error| parse_error(operation, &error.to_string()))?
        .lines()
        .map(str::trim)
        .filter(|commit| !commit.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    match commits.as_slice() {
        [commit] => Ok(commit.clone()),
        _ => Err(parse_error(
            operation,
            "Git did not resolve exactly one commit",
        )),
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

fn changed_files_between(
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

fn load_commit_sources(
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
            OsStr::new("--src-prefix=a/"),
            OsStr::new("--dst-prefix=b/"),
            OsStr::new("HEAD"),
            OsStr::new("--"),
            root_spec.as_os_str(),
        ],
    )?;
    parse_diff_hunks(&output.stdout, repository_root, root)
}

fn changed_lines_between(
    repository_root: &Path,
    root: &Path,
    root_spec: &Path,
    before_revision: &str,
    after_revision: &str,
) -> Result<Vec<SourceLineChange>, GitCommandError> {
    let output = checked_git(
        repository_root,
        "read Git commit diff hunks",
        [
            OsStr::new("-c"),
            OsStr::new("core.quotePath=false"),
            OsStr::new("diff"),
            OsStr::new("--no-ext-diff"),
            OsStr::new("--no-renames"),
            OsStr::new("--unified=0"),
            OsStr::new("--no-color"),
            OsStr::new("--src-prefix=a/"),
            OsStr::new("--dst-prefix=b/"),
            OsStr::new(before_revision),
            OsStr::new(after_revision),
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

fn is_direct_repository_path(repository_root: &Path, root: &Path, path: &Path) -> bool {
    repository_root.join(path).parent() == Some(root)
}

fn is_configuration_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    path.extension()
        .is_some_and(|extension| extension == OsStr::new("tf"))
        || name.is_some_and(|name| {
            name.ends_with(".tf.json")
                || name.ends_with(".tfvars")
                || name.ends_with(".tfvars.json")
                || name == ".terraform.lock.hcl"
        })
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
    run_git_with_env(directory, operation, args, &[])
}

fn run_git_with_env<I, S>(
    directory: &Path,
    operation: &str,
    args: I,
    environment: &[(&str, &str)],
) -> Result<Output, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(args);
    for (key, value) in environment {
        command.env(key, value);
    }
    command
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
    unavailable_diff_with_comparison(
        root,
        repository_root,
        status,
        ComparisonMetadata::working_tree(),
    )
}

const fn unavailable_diff_with_comparison(
    root: PathBuf,
    repository_root: Option<PathBuf>,
    status: GitDiffStatus,
    comparison: ComparisonMetadata,
) -> GitDiff {
    GitDiff {
        root,
        repository_root,
        comparison,
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
    failed_diff_with_comparison(
        root,
        repository_root,
        operation,
        message,
        ComparisonMetadata::working_tree(),
    )
}

fn failed_diff_with_comparison(
    root: PathBuf,
    repository_root: Option<PathBuf>,
    operation: &str,
    message: &str,
    comparison: ComparisonMetadata,
) -> GitDiff {
    unavailable_diff_with_comparison(
        root,
        repository_root,
        GitDiffStatus::Failed {
            operation: operation.to_owned(),
            message: message.to_owned(),
        },
        comparison,
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

enum CompareRefError {
    Unavailable(String),
    Ambiguous(String),
    Failed(GitCommandError),
}

enum MergeBaseError {
    NoCommonAncestor,
    Ambiguous(Vec<String>),
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

    fn git_output(repository: &Path, args: &[&str]) -> String {
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
        String::from_utf8(output.stdout)
            .expect("git output is UTF-8")
            .trim()
            .to_owned()
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
    fn compares_commits_without_including_dirty_worktree_changes() {
        let repository = TestRepository::new();
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"base\"\n}\n",
        );
        repository.commit("initial");
        git(&repository.path, &["branch", "compare"]);
        git(&repository.path, &["switch", "compare"]);
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"compare\"\n}\n",
        );
        repository.commit("compare change");
        git(&repository.path, &["switch", "-c", "feature"]);
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"feature\"\n}\n",
        );
        repository.commit("feature change");
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"dirty\"\n}\n",
        );

        let result = collect_diff_against_ref(&repository.path, "compare");

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert_eq!(result.basis(), ComparisonBasis::HeadVsMergeBase);
        assert_eq!(result.compare_ref(), Some("compare"));
        assert_eq!(
            result.before()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"compare\"\n}\n"
        );
        assert_eq!(
            result.after()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"feature\"\n}\n"
        );
        assert_eq!(result.resolved_commit(), result.merge_base());
        assert!(result.head_commit().is_some());
        assert_eq!(result.changed_lines().len(), 2);
        assert_eq!(result.changed_lines()[0].range(), SourceRange::new(2, 2));
        assert_eq!(result.changed_lines()[1].range(), SourceRange::new(2, 2));
    }

    #[test]
    fn reports_an_unavailable_comparison_ref_with_the_requested_ref() {
        let repository = TestRepository::new();
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");
        repository.commit("initial");

        let result = collect_diff_against_ref(&repository.path, "missing");

        assert!(matches!(
            result.status(),
            GitDiffStatus::CompareRefUnavailable { reference, message }
                if reference == "missing" && !message.is_empty()
        ));
        assert_eq!(result.compare_ref(), Some("missing"));
        assert!(result.resolved_commit().is_none());
        assert!(result.head_commit().is_some());
    }

    #[test]
    fn reports_when_a_comparison_ref_name_is_ambiguous() {
        let repository = TestRepository::new();
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");
        repository.commit("initial");
        git(&repository.path, &["branch", "compare"]);
        git(&repository.path, &["tag", "compare"]);
        git(
            &repository.path,
            &["config", "core.warnAmbiguousRefs", "false"],
        );

        let result = collect_diff_against_ref(&repository.path, "compare");

        assert!(matches!(
            result.status(),
            GitDiffStatus::AmbiguousCompareRef { reference, message }
                if reference == "compare"
                    && message.contains("refs/heads/compare")
                    && message.contains("refs/tags/compare")
        ));
        assert_eq!(result.compare_ref(), Some("compare"));
        assert!(result.resolved_commit().is_none());
        assert!(result.head_commit().is_some());
    }

    #[test]
    fn does_not_treat_an_arbitrary_git_directory_file_as_a_pseudo_ref() {
        let repository = TestRepository::new();
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");
        repository.commit("initial");
        git(&repository.path, &["branch", "config"]);
        git(&repository.path, &["tag", "config"]);

        let result = collect_diff_against_ref(&repository.path, "config");

        assert!(matches!(
            result.status(),
            GitDiffStatus::AmbiguousCompareRef { reference, message }
                if reference == "config"
                    && message.contains("refs/heads/config")
                    && message.contains("refs/tags/config")
        ));
    }

    #[test]
    fn resolves_an_unambiguous_ref_when_git_trace_writes_to_stderr() {
        let repository = TestRepository::new();
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");
        repository.commit("initial");
        git(&repository.path, &["branch", "compare"]);

        let result =
            resolve_compare_ref_with_env(&repository.path, "compare", &[("GIT_TRACE", "1")]);

        assert!(matches!(result, Ok(commit) if commit.len() == 40));
    }

    #[test]
    fn resolves_a_fully_qualified_ref_without_short_name_expansion() {
        let repository = TestRepository::new();
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"branch\"\n}\n",
        );
        repository.commit("branch target");
        git(&repository.path, &["branch", "foo"]);
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"tag\"\n}\n",
        );
        repository.commit("tag target");
        git(&repository.path, &["tag", "refs/heads/foo"]);

        let result = collect_diff_against_ref(&repository.path, "refs/heads/foo");

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert_eq!(
            result.before()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"branch\"\n}\n"
        );
        assert_eq!(
            result.after()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"tag\"\n}\n"
        );
    }

    #[test]
    fn resolves_a_git_directory_pseudo_ref_before_namespace_expansion() {
        let repository = TestRepository::new();
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"pseudo-ref\"\n}\n",
        );
        repository.commit("pseudo-ref target");
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"tag\"\n}\n",
        );
        repository.commit("tag target");
        git(&repository.path, &["tag", "ORIG_HEAD"]);
        let orig_head_path =
            git_output(&repository.path, &["rev-parse", "--git-path", "ORIG_HEAD"]);
        let pseudo_ref_target = git_output(&repository.path, &["rev-parse", "HEAD^"]);
        let orig_head_path = PathBuf::from(orig_head_path);
        let orig_head_path = if orig_head_path.is_absolute() {
            orig_head_path
        } else {
            repository.path.join(orig_head_path)
        };
        assert!(
            Path::new(&orig_head_path)
                .parent()
                .is_some_and(Path::is_dir),
            "{orig_head_path:?}"
        );
        fs::write(orig_head_path, format!("{pseudo_ref_target}\n"))
            .expect("write Git directory pseudo-ref");

        let result = collect_diff_against_ref(&repository.path, "ORIG_HEAD");

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert_eq!(
            result.before()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"pseudo-ref\"\n}\n"
        );
        assert_eq!(
            result.after()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"tag\"\n}\n"
        );
    }

    #[test]
    fn resolves_a_custom_git_directory_root_ref_before_namespace_expansion() {
        let repository = TestRepository::new();
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"root-ref\"\n}\n",
        );
        repository.commit("root-ref target");
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"tag\"\n}\n",
        );
        repository.commit("tag target");
        git(&repository.path, &["tag", "CUSTOM_HEAD"]);
        let root_ref_path = git_output(
            &repository.path,
            &["rev-parse", "--git-path", "CUSTOM_HEAD"],
        );
        let root_ref_target = git_output(&repository.path, &["rev-parse", "HEAD^"]);
        let root_ref_path = PathBuf::from(root_ref_path);
        let root_ref_path = if root_ref_path.is_absolute() {
            root_ref_path
        } else {
            repository.path.join(root_ref_path)
        };
        fs::write(root_ref_path, format!("{root_ref_target}\n"))
            .expect("write Git directory root ref");

        let result = collect_diff_against_ref(&repository.path, "CUSTOM_HEAD");

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert_eq!(
            result.before()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"root-ref\"\n}\n"
        );
        assert_eq!(
            result.after()[0].source(),
            "resource \"example\" \"one\" {\n  value = \"tag\"\n}\n"
        );
    }

    #[test]
    fn reports_a_failed_root_ref_without_falling_back_to_a_namespace_ref() {
        let repository = TestRepository::new();
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");
        repository.commit("root-ref target");
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = \"tag\"\n}\n",
        );
        repository.commit("tag target");
        git(&repository.path, &["tag", "CUSTOM_HEAD"]);
        let root_ref_path = git_output(
            &repository.path,
            &["rev-parse", "--git-path", "CUSTOM_HEAD"],
        );
        let blob_target = git_output(&repository.path, &["rev-parse", "HEAD^:main.tf"]);
        let root_ref_path = PathBuf::from(root_ref_path);
        let root_ref_path = if root_ref_path.is_absolute() {
            root_ref_path
        } else {
            repository.path.join(root_ref_path)
        };
        fs::write(root_ref_path, format!("{blob_target}\n")).expect("write Git directory root ref");

        let result = collect_diff_against_ref(&repository.path, "CUSTOM_HEAD");

        assert!(matches!(
            result.status(),
            GitDiffStatus::CompareRefUnavailable { reference, message }
                if reference == "CUSTOM_HEAD" && !message.is_empty()
        ));
        assert!(result.resolved_commit().is_none());
        assert!(result.head_commit().is_some());
    }

    #[test]
    fn reports_when_comparison_commits_have_no_common_ancestor() {
        let repository = TestRepository::new();
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");
        repository.commit("initial");
        git(&repository.path, &["branch", "root"]);
        git(&repository.path, &["switch", "--orphan", "unrelated"]);
        write(&repository, "other.tf", "resource \"example\" \"two\" {}\n");
        repository.commit("unrelated");

        let result = collect_diff_against_ref(&repository.path, "root");

        assert!(matches!(
            result.status(),
            GitDiffStatus::NoCommonAncestor { reference, message }
                if reference == "root" && !message.is_empty()
        ));
        assert!(result.resolved_commit().is_some());
        assert!(result.head_commit().is_some());
        assert!(result.merge_base().is_none());
    }

    #[test]
    fn reports_ambiguous_comparison_when_merge_base_is_not_unique() {
        let repository = TestRepository::new();
        write(&repository, "base.tf", "resource \"example\" \"base\" {}\n");
        repository.commit("initial");
        git(&repository.path, &["branch", "base"]);
        git(&repository.path, &["switch", "-c", "branch-a"]);
        write(&repository, "a.tf", "resource \"example\" \"a\" {}\n");
        repository.commit("branch a");
        git(&repository.path, &["branch", "a-tip"]);
        git(&repository.path, &["switch", "-c", "branch-b", "base"]);
        write(&repository, "b.tf", "resource \"example\" \"b\" {}\n");
        repository.commit("branch b");
        git(&repository.path, &["branch", "b-tip"]);
        git(&repository.path, &["switch", "branch-a"]);
        git(
            &repository.path,
            &["merge", "--no-ff", "--no-edit", "b-tip"],
        );
        git(&repository.path, &["switch", "branch-b"]);
        git(
            &repository.path,
            &["merge", "--no-ff", "--no-edit", "a-tip"],
        );

        let result = collect_diff_against_ref(&repository.path, "branch-a");

        assert!(matches!(
            result.status(),
            GitDiffStatus::AmbiguousMergeBase {
                reference,
                merge_bases,
                message,
            } if reference == "branch-a" && merge_bases.len() == 2 && !message.is_empty()
        ));
        assert!(result.resolved_commit().is_some());
        assert!(result.head_commit().is_some());
        assert!(result.merge_base().is_none());
    }

    #[test]
    fn fixes_diff_path_prefixes_despite_git_configuration() {
        let repository = TestRepository::new();
        write(&repository, "main.tf", "resource \"example\" \"one\" {}\n");
        repository.commit("initial");
        git(&repository.path, &["config", "diff.noPrefix", "true"]);
        write(
            &repository,
            "main.tf",
            "resource \"example\" \"one\" {\n  value = true\n}\n",
        );

        let result = collect_diff(&repository.path);

        assert_eq!(result.status(), &GitDiffStatus::Complete);
        assert_eq!(result.changed_lines().len(), 2);
        assert_eq!(result.changed_lines()[0].side(), SourceSide::Before);
        assert_eq!(result.changed_lines()[1].side(), SourceSide::After);
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
    fn missing_head_uses_only_existing_working_tree_files() {
        let repository = TestRepository::new();
        write(
            &repository,
            "removed.tf",
            "resource \"example\" \"removed\" {}\n",
        );
        write(
            &repository,
            "present.tf",
            "resource \"example\" \"present\" {}\n",
        );
        git(&repository.path, &["add", "removed.tf", "present.tf"]);
        fs::remove_file(repository.path.join("removed.tf")).expect("remove staged source");

        let result = collect_diff(&repository.path);
        let root = result.root().to_owned();

        assert!(matches!(
            result.status(),
            GitDiffStatus::HeadUnavailable { .. }
        ));
        assert_eq!(
            source_names(result.after()),
            vec![root.join("present.tf").as_path()]
        );
        assert_eq!(result.changed_lines().len(), 1);
        assert_eq!(result.changed_lines()[0].path(), root.join("present.tf"));
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
