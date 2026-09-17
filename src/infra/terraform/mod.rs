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

pub(crate) use command::{
    CancellationToken, ProcessRunner, SystemProcessRunner, TerraformExecutionError,
    TerraformExecutionErrorKind,
};
pub(crate) use plan::run_plan_with_events_with_runner_and_phase;
pub(crate) use workspace::read_workspace_with_runner;

#[cfg(test)]
pub(crate) use command::{ProcessOutput, ProcessStatus, RunningProcess};
