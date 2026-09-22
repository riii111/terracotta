use std::{ffi::OsString, path::Path};

use serde_json::Value;

use crate::app::execution::Tool;
use crate::infra::CancellationToken;

use super::command::{
    ProcessRunner, ProcessStatus, TerraformCommand, TerraformExecutionError,
    TerraformExecutionErrorKind, interrupted_error, non_zero_error, run_command,
};

pub(crate) fn read_version_with_arguments(
    tool: Tool,
    root: &Path,
    global_arguments: &[OsString],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<String, TerraformExecutionError> {
    let mut arguments = global_arguments.to_vec();
    arguments.extend([OsString::from("version"), OsString::from("-json")]);
    let output = run_command(
        tool,
        root,
        TerraformCommand::Version,
        &arguments,
        cancellation,
        runner,
    )?;
    if output.interrupted {
        return Err(interrupted_error(tool, TerraformCommand::Version, output));
    }
    if !output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(tool, TerraformCommand::Version, output));
    }
    let document = serde_json::from_slice::<Value>(&output.output.stdout).map_err(|error| {
        TerraformExecutionError::new_for_tool(
            tool,
            TerraformExecutionErrorKind::InvalidVersion {
                message: error.to_string(),
            },
        )
    })?;
    document
        .get("terraform_version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            TerraformExecutionError::new_for_tool(
                tool,
                TerraformExecutionErrorKind::InvalidVersion {
                    message: "terraform_version is missing".to_owned(),
                },
            )
        })
}
