use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionContextValue {
    Loading,
    Known(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct VariableSources {
    automatic_files: Vec<PathBuf>,
    explicit_files: Vec<PathBuf>,
    has_var_argument: bool,
    environment_variables: Vec<String>,
}

impl VariableSources {
    #[must_use]
    pub(crate) const fn new(
        automatic_files: Vec<PathBuf>,
        explicit_files: Vec<PathBuf>,
        has_var_argument: bool,
        environment_variables: Vec<String>,
    ) -> Self {
        Self {
            automatic_files,
            explicit_files,
            has_var_argument,
            environment_variables,
        }
    }

    #[must_use]
    pub(crate) fn automatic_files(&self) -> &[PathBuf] {
        &self.automatic_files
    }

    #[must_use]
    pub(crate) fn explicit_files(&self) -> &[PathBuf] {
        &self.explicit_files
    }

    #[must_use]
    pub(crate) const fn has_var_argument(&self) -> bool {
        self.has_var_argument
    }

    #[must_use]
    pub(crate) fn environment_variables(&self) -> &[String] {
        &self.environment_variables
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionContext {
    cwd: PathBuf,
    workspace: ExecutionContextValue,
    display_name: ExecutionContextValue,
    production: Option<bool>,
    tool_name: String,
    tool_version: ExecutionContextValue,
    variable_sources: VariableSources,
}

impl ExecutionContext {
    pub(crate) fn loading(cwd: impl Into<String>) -> Self {
        Self {
            cwd: PathBuf::from(cwd.into()),
            workspace: ExecutionContextValue::Loading,
            display_name: ExecutionContextValue::Loading,
            production: None,
            tool_name: "terraform".to_owned(),
            tool_version: ExecutionContextValue::Loading,
            variable_sources: VariableSources::default(),
        }
    }

    pub(crate) fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        let workspace = workspace.into();
        self.display_name = ExecutionContextValue::Known(display_name(&self.cwd, &workspace));
        self.production = Some(is_production(&self.cwd, &workspace));
        self.workspace = ExecutionContextValue::Known(workspace);
        self
    }

    pub(crate) fn with_tool_version(
        mut self,
        tool_name: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        self.tool_name = tool_name.into();
        self.tool_version = ExecutionContextValue::Known(version.into());
        self
    }

    pub(crate) fn with_variable_sources(mut self, variable_sources: VariableSources) -> Self {
        self.variable_sources = variable_sources;
        self
    }

    #[must_use]
    pub(crate) fn cwd_path(&self) -> &Path {
        &self.cwd
    }

    #[must_use]
    pub(crate) const fn workspace(&self) -> &ExecutionContextValue {
        &self.workspace
    }

    #[must_use]
    pub(crate) const fn display_name(&self) -> &ExecutionContextValue {
        &self.display_name
    }

    #[must_use]
    pub(crate) const fn is_production(&self) -> Option<bool> {
        self.production
    }

    #[must_use]
    pub(crate) fn tool_name(&self) -> &str {
        &self.tool_name
    }

    #[must_use]
    pub(crate) const fn tool_version(&self) -> &ExecutionContextValue {
        &self.tool_version
    }

    #[must_use]
    pub(crate) const fn variable_sources(&self) -> &VariableSources {
        &self.variable_sources
    }
}

fn display_name(cwd: &Path, workspace: &str) -> String {
    if workspace == "default" {
        cwd.file_name().map_or_else(
            || cwd.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    } else {
        workspace.to_owned()
    }
}

fn is_production(cwd: &Path, workspace: &str) -> bool {
    cwd.components()
        .filter_map(|component| component.as_os_str().to_str())
        .chain(std::iter::once(workspace))
        .flat_map(|component| component.split(['-', '_', '/']))
        .any(|token| {
            matches!(
                token.to_ascii_lowercase().as_str(),
                "prod" | "production" | "prd"
            )
        })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::prod("/repo/prod", "default", true)]
    #[case::production("/repo/productionapp", "default", false)]
    #[case::production_token("/repo/production/prod", "default", true)]
    #[case::workspace("/repo/staging", "prd", true)]
    #[case::product("/repo/product", "default", false)]
    fn production_detection_uses_complete_tokens(
        #[case] cwd: &str,
        #[case] workspace: &str,
        #[case] expected: bool,
    ) {
        assert_eq!(is_production(Path::new(cwd), workspace), expected);
    }

    #[test]
    fn default_workspace_uses_directory_name_as_display_name() {
        let context = ExecutionContext::loading("/repo/infra/prod").with_workspace("default");

        assert_eq!(
            context.display_name(),
            &ExecutionContextValue::Known("prod".to_owned())
        );
        assert_eq!(context.is_production(), Some(true));
    }

    #[test]
    fn non_default_workspace_is_the_display_name() {
        let context = ExecutionContext::loading("/repo/infra").with_workspace("production");

        assert_eq!(
            context.display_name(),
            &ExecutionContextValue::Known("production".to_owned())
        );
        assert_eq!(context.is_production(), Some(true));
    }
}
