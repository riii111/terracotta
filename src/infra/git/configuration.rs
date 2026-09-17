use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use crate::infra::terraform::hcl::HclSourceFile;

use super::{
    command::{checked_git, nul_fields},
    diff::GitDiff,
};

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
        additional_paths: &[PathBuf],
    ) -> Vec<PathBuf> {
        let mut paths = before
            .iter()
            .map(|source| source.path().to_owned())
            .chain(after.iter().map(|source| source.path().to_owned()))
            .chain(additional_paths.iter().cloned())
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        paths
            .into_iter()
            .filter(|path| {
                if additional_paths.contains(path)
                    && !before
                        .iter()
                        .chain(after)
                        .any(|source| source.path() == path)
                {
                    return true;
                }
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
