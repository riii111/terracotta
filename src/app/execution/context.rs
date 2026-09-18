use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionContextValue {
    Loading,
    Unavailable,
    Known(String),
}

impl ExecutionContextValue {
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Loading => "loading...",
            Self::Unavailable => "unavailable",
            Self::Known(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionContext {
    cwd: ExecutionContextValue,
    repository_root: ExecutionContextValue,
    repository_root_path: Option<PathBuf>,
    workspace: ExecutionContextValue,
    git: ExecutionContextValue,
    comparison: ExecutionContextValue,
}

impl ExecutionContext {
    #[must_use]
    pub(crate) fn known(
        cwd: impl Into<String>,
        workspace: impl Into<String>,
        git: impl Into<String>,
        comparison: impl Into<String>,
    ) -> Self {
        Self {
            cwd: ExecutionContextValue::Known(cwd.into()),
            repository_root: ExecutionContextValue::Unavailable,
            repository_root_path: None,
            workspace: ExecutionContextValue::Known(workspace.into()),
            git: ExecutionContextValue::Known(git.into()),
            comparison: ExecutionContextValue::Known(comparison.into()),
        }
    }

    pub(crate) fn loading(cwd: impl Into<String>, comparison: impl Into<String>) -> Self {
        Self {
            cwd: ExecutionContextValue::Known(cwd.into()),
            repository_root: ExecutionContextValue::Loading,
            repository_root_path: None,
            workspace: ExecutionContextValue::Loading,
            git: ExecutionContextValue::Loading,
            comparison: ExecutionContextValue::Known(comparison.into()),
        }
    }

    pub(crate) fn with_repository_root(mut self, repository_root: Option<PathBuf>) -> Self {
        self.repository_root_path.clone_from(&repository_root);
        self.repository_root = repository_root.map_or(ExecutionContextValue::Unavailable, |path| {
            ExecutionContextValue::Known(path.display().to_string())
        });
        self
    }

    pub(crate) fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = ExecutionContextValue::Known(workspace.into());
        self
    }

    pub(crate) fn with_git(mut self, git: Option<String>) -> Self {
        self.git = git.map_or(
            ExecutionContextValue::Unavailable,
            ExecutionContextValue::Known,
        );
        self
    }

    #[must_use]
    pub(crate) const fn cwd(&self) -> &ExecutionContextValue {
        &self.cwd
    }

    #[must_use]
    pub(crate) fn cwd_path(&self) -> &Path {
        match &self.cwd {
            ExecutionContextValue::Known(value) => Path::new(value),
            ExecutionContextValue::Loading | ExecutionContextValue::Unavailable => Path::new(""),
        }
    }

    #[must_use]
    pub(crate) const fn workspace(&self) -> &ExecutionContextValue {
        &self.workspace
    }

    #[must_use]
    pub(crate) const fn repository_root(&self) -> &ExecutionContextValue {
        &self.repository_root
    }

    #[must_use]
    pub(crate) fn repository_root_path(&self) -> Option<&Path> {
        self.repository_root_path.as_deref()
    }

    #[must_use]
    pub(crate) const fn git(&self) -> &ExecutionContextValue {
        &self.git
    }

    #[must_use]
    pub(crate) const fn comparison(&self) -> &ExecutionContextValue {
        &self.comparison
    }
}
