use std::path::PathBuf;

use crate::app::execution::Tool;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EnvironmentIdentity {
    pub(crate) directory: PathBuf,
    pub(crate) workspace: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Environment {
    pub(crate) tool: Tool,
    pub(crate) availability: EnvironmentAvailability,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EnvironmentAvailability {
    Available(EnvironmentIdentity),
    ExcludedHcp { directory: PathBuf },
    Error { directory: PathBuf, message: String },
}

impl Environment {
    pub(crate) const fn is_available(&self) -> bool {
        matches!(self.availability, EnvironmentAvailability::Available(_))
    }
}
