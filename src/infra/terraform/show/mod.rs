use std::{ffi::OsString, path::Path};

use crate::app::plan::Plan;
use crate::infra::CancellationToken;

use super::command::{
    ProcessOutput, ProcessRunner, ProcessStatus, TerraformCommand, TerraformExecutionError,
    TerraformExecutionErrorKind, interrupted_error, non_zero_error, run_command,
};

mod json;

pub(super) use json::PlanParseError;
use json::parse_plan_json_bytes;

pub(super) fn read_plan(
    root: &Path,
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<Plan, TerraformExecutionError> {
    if cancellation.is_cancelled() {
        return Err(TerraformExecutionError::new(
            TerraformExecutionErrorKind::Interrupted {
                command: TerraformCommand::Show,
                output: ProcessOutput::empty(),
                kill_error: None,
            },
        ));
    }

    let show_arguments = show_arguments(plan_path);
    let show_output = run_command(
        root,
        TerraformCommand::Show,
        &show_arguments,
        cancellation,
        runner,
    )?;
    if show_output.interrupted {
        return Err(interrupted_error(TerraformCommand::Show, show_output));
    }
    if !show_output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(TerraformCommand::Show, show_output));
    }

    let json = show_output.output.stdout;
    let plan = parse_plan_json_bytes(&json).map_err(|source| {
        TerraformExecutionError::new(TerraformExecutionErrorKind::InvalidPlan { source })
    })?;

    Ok(plan)
}

fn show_arguments(plan_path: &Path) -> Vec<OsString> {
    vec![
        OsString::from("show"),
        OsString::from("-json"),
        plan_path.as_os_str().to_owned(),
    ]
}
