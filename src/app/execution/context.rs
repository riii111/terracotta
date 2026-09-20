use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionContextValue {
    Loading,
    Known(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionContext {
    cwd: String,
    workspace: ExecutionContextValue,
}

impl ExecutionContext {
    pub(crate) fn loading(cwd: impl Into<String>) -> Self {
        Self {
            cwd: cwd.into(),
            workspace: ExecutionContextValue::Loading,
        }
    }

    pub(crate) fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = ExecutionContextValue::Known(workspace.into());
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
}
