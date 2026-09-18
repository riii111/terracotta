use std::{ffi::OsString, path::Path};

use crate::infra::CancellationToken;

use super::command::{
    ProcessRunner, ProcessStatus, TerraformCommand, TerraformExecutionError,
    TerraformExecutionErrorKind, interrupted_error, non_zero_error, run_command,
};

pub(crate) fn read_workspace_with_runner(
    root: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<String, TerraformExecutionError> {
    let output = run_command(
        root,
        TerraformCommand::WorkspaceShow,
        &[OsString::from("workspace"), OsString::from("show")],
        cancellation,
        runner,
    )?;
    if output.interrupted {
        return Err(interrupted_error(TerraformCommand::WorkspaceShow, output));
    }
    if !output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(TerraformCommand::WorkspaceShow, output));
    }
    let workspace = String::from_utf8(output.output.stdout).map_err(|error| {
        TerraformExecutionError::new(TerraformExecutionErrorKind::InvalidWorkspace {
            message: error.to_string(),
        })
    })?;
    let workspace = workspace.trim();
    if workspace.is_empty() {
        return Err(TerraformExecutionError::new(
            TerraformExecutionErrorKind::InvalidWorkspace {
                message: "workspace name is empty".to_owned(),
            },
        ));
    }
    Ok(workspace.to_owned())
}
