#![allow(
    clippy::redundant_pub_crate,
    reason = "Terraform integration is shared only within the crate"
)]

mod command;
mod events;
pub(crate) mod hcl;
mod plan;
mod show;
mod workspace;

#[allow(
    unused_imports,
    reason = "Terraform execution API is shared with crate callers"
)]
pub(crate) use self::{
    command::{
        CancellationToken, ProcessOutput, ProcessOutputChunk, ProcessRunner, ProcessStatus,
        RunningProcess, SystemProcessRunner, TerraformCommand, TerraformExecutionError,
        TerraformExecutionErrorKind,
    },
    plan::{run_plan, run_plan_with_events, run_plan_with_events_with_runner},
    show::PlanExecution,
    workspace::{read_workspace, read_workspace_with_runner},
};
