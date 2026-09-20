use std::path::Path;

#[cfg(test)]
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionContextValue {
    Loading,
    #[cfg(test)]
    Unavailable,
    Known(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionContext {
    cwd: String,
    workspace: ExecutionContextValue,
    #[cfg(test)]
    repository_root: ExecutionContextValue,
    #[cfg(test)]
    repository_root_path: Option<PathBuf>,
    #[cfg(test)]
    git: ExecutionContextValue,
    #[cfg(test)]
    comparison: ExecutionContextValue,
}

impl ExecutionContext {
    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "retained for dormant Git comparison regression fixtures"
    )]
    #[must_use]
    pub(crate) fn known(
        cwd: impl Into<String>,
        workspace: impl Into<String>,
        git: impl Into<String>,
        comparison: impl Into<String>,
    ) -> Self {
        Self {
            cwd: cwd.into(),
            workspace: ExecutionContextValue::Known(workspace.into()),
            repository_root: ExecutionContextValue::Unavailable,
            repository_root_path: None,
            git: ExecutionContextValue::Known(git.into()),
            comparison: ExecutionContextValue::Known(comparison.into()),
        }
    }

    pub(crate) fn loading(cwd: impl Into<String>, comparison: &str) -> Self {
        #[cfg(not(test))]
        let _ = comparison;
        Self {
            cwd: cwd.into(),
            workspace: ExecutionContextValue::Loading,
            #[cfg(test)]
            repository_root: ExecutionContextValue::Loading,
            #[cfg(test)]
            repository_root_path: None,
            #[cfg(test)]
            git: ExecutionContextValue::Loading,
            #[cfg(test)]
            comparison: ExecutionContextValue::Known(comparison.to_owned()),
        }
    }

    #[cfg(test)]
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

    #[cfg(test)]
    pub(crate) fn with_git(mut self, git: Option<String>) -> Self {
        self.git = git.map_or(
            ExecutionContextValue::Unavailable,
            ExecutionContextValue::Known,
        );
        self
    }

    #[must_use]
    pub(crate) fn cwd_path(&self) -> &Path {
        Path::new(&self.cwd)
    }

    #[must_use]
    pub(crate) const fn workspace(&self) -> &ExecutionContextValue {
        &self.workspace
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) const fn repository_root(&self) -> &ExecutionContextValue {
        &self.repository_root
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn repository_root_path(&self) -> Option<&Path> {
        self.repository_root_path.as_deref()
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "retained for dormant Git comparison regression fixtures"
    )]
    #[must_use]
    pub(crate) const fn git(&self) -> &ExecutionContextValue {
        &self.git
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "retained for dormant Git comparison regression fixtures"
    )]
    #[must_use]
    pub(crate) const fn comparison(&self) -> &ExecutionContextValue {
        &self.comparison
    }
}
