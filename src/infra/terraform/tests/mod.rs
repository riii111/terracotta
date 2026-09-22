// HCL parsing is dormant with Git attribution and remains covered by unit tests.
pub(in crate::infra) mod hcl;
pub(in crate::infra) mod support {
    pub(crate) use super::super::command::{
        ProcessOutput, ProcessRunner, ProcessStatus, RunningProcess,
        TerraformExecutionError as CommandTerraformExecutionError,
    };
    pub(crate) use super::super::plan::tests::support::{PlanTestError, run_plan};
}
