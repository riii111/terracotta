mod command;
mod events;
// HCL parsing is dormant with Git attribution and remains covered by unit tests.
#[cfg(test)]
#[allow(
    dead_code,
    reason = "dormant HCL attribution remains compiled only for unit tests"
)]
pub(crate) mod hcl;
mod plan;
mod show;
mod workspace;

pub(crate) use command::SystemProcessRunner;
pub(crate) use plan::{SavedPlan, run_review};

#[cfg(test)]
pub(crate) use command::{ProcessRunner, TerraformExecutionError};
#[cfg(test)]
pub(crate) use plan::run_plan;
#[cfg(test)]
pub(crate) use workspace::read_workspace_with_runner;

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::command::{ProcessOutput, ProcessStatus, RunningProcess};
}
