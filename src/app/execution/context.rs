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
    workspace: ExecutionContextValue,
    git: ExecutionContextValue,
    comparison: ExecutionContextValue,
}

impl ExecutionContext {
    #[must_use]
    pub(crate) const fn loading() -> Self {
        Self {
            cwd: ExecutionContextValue::Loading,
            workspace: ExecutionContextValue::Loading,
            git: ExecutionContextValue::Loading,
            comparison: ExecutionContextValue::Loading,
        }
    }

    #[must_use]
    pub(crate) const fn unavailable() -> Self {
        Self {
            cwd: ExecutionContextValue::Unavailable,
            workspace: ExecutionContextValue::Unavailable,
            git: ExecutionContextValue::Unavailable,
            comparison: ExecutionContextValue::Unavailable,
        }
    }

    #[must_use]
    pub(crate) fn known(
        cwd: impl Into<String>,
        workspace: impl Into<String>,
        git: impl Into<String>,
        comparison: impl Into<String>,
    ) -> Self {
        Self {
            cwd: ExecutionContextValue::Known(cwd.into()),
            workspace: ExecutionContextValue::Known(workspace.into()),
            git: ExecutionContextValue::Known(git.into()),
            comparison: ExecutionContextValue::Known(comparison.into()),
        }
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
    pub(crate) const fn workspace(&self) -> &ExecutionContextValue {
        &self.workspace
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
