mod apply;
mod command;
mod events;
mod line_buffer;
// HCL parsing is dormant with Git attribution and remains covered by unit tests.
#[cfg(test)]
pub(crate) mod hcl;
mod plan;
mod show;
mod workspace;

pub(crate) use apply::run_apply;
pub(crate) use command::SystemProcessRunner;
pub(crate) use plan::{SavedPlan, run_review};

#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) use super::command::{
        ProcessOutput, ProcessRunner, ProcessStatus, RunningProcess,
        TerraformExecutionError as CommandTerraformExecutionError,
    };
    pub(crate) use super::plan::test_support::{PlanTestError, run_plan};
    pub(crate) use super::workspace::read_workspace_with_runner;
}
