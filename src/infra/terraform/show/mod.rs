use std::{
    ffi::OsString,
    fmt::{Display, Formatter},
    path::Path,
};

use crate::app::review::{PlanDocument, PlanMetadata};
use crate::infra::CancellationToken;

use super::command::{
    ProcessOutput, ProcessRunner, ProcessStatus, TerraformCommand, TerraformExecutionError,
    TerraformExecutionErrorKind, interrupted_error, non_zero_error, run_command,
};

#[cfg(test)]
mod json;
mod metadata;
mod text;

#[cfg(test)]
use crate::app::plan::Plan;
#[cfg(test)]
use json::parse_plan_json_bytes;
use metadata::parse_metadata;
use text::parse_document;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanParseError {
    InvalidJson,
    InvalidUtf8,
    RootMustBeObject,
    MissingField(&'static str),
    InvalidField(&'static str),
    UnsupportedFormatMajor(u64),
}

impl Display for PlanParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => formatter.write_str("Terraform plan JSON is invalid"),
            Self::InvalidUtf8 => formatter.write_str("Terraform plan text is not valid UTF-8"),
            Self::RootMustBeObject => {
                formatter.write_str("Terraform plan JSON root must be an object")
            }
            Self::MissingField(field) => {
                write!(formatter, "Terraform plan JSON is missing {field}")
            }
            Self::InvalidField(field) => {
                write!(formatter, "Terraform plan JSON has an invalid {field}")
            }
            Self::UnsupportedFormatMajor(major) => write!(
                formatter,
                "Terraform plan JSON format major version {major} is unsupported"
            ),
        }
    }
}

impl std::error::Error for PlanParseError {}

pub(super) fn read_review(
    root: &Path,
    plan_path: &Path,
    plan_changed: bool,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<(PlanDocument, PlanMetadata), TerraformExecutionError> {
    let text = run_show(root, plan_path, false, cancellation, runner)?;
    let document = parse_document(text.output.stdout).map_err(invalid_plan)?;
    let json = run_show(root, plan_path, true, cancellation, runner)?;
    let metadata = parse_metadata(&json.output.stdout, plan_changed).map_err(invalid_plan)?;
    Ok((document, metadata))
}

#[cfg(test)]
pub(super) fn read_plan(
    root: &Path,
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<Plan, TerraformExecutionError> {
    let output = run_show(root, plan_path, true, cancellation, runner)?;
    parse_plan_json_bytes(&output.output.stdout).map_err(invalid_plan)
}

fn run_show(
    root: &Path,
    plan_path: &Path,
    json: bool,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<super::command::ProcessResult, TerraformExecutionError> {
    if cancellation.is_cancelled() {
        return Err(TerraformExecutionError::new(
            TerraformExecutionErrorKind::Interrupted {
                command: TerraformCommand::Show,
                output: Box::new(ProcessOutput::empty()),
                interrupt_error: None,
            },
        ));
    }
    let output = run_command(
        root,
        TerraformCommand::Show,
        &show_arguments(plan_path, json),
        cancellation,
        runner,
    )?;
    if output.interrupted {
        return Err(interrupted_error(TerraformCommand::Show, output));
    }
    if !output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(TerraformCommand::Show, output));
    }
    Ok(output)
}

const fn invalid_plan(source: PlanParseError) -> TerraformExecutionError {
    TerraformExecutionError::new(TerraformExecutionErrorKind::InvalidPlan { source })
}

fn show_arguments(plan_path: &Path, json: bool) -> Vec<OsString> {
    let mut arguments = vec![OsString::from("show")];
    arguments.push(OsString::from(if json { "-json" } else { "-no-color" }));
    arguments.push(plan_path.as_os_str().to_owned());
    arguments
}
