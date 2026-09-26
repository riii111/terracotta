use std::{
    ffi::OsString,
    fmt::{Display, Formatter},
    path::Path,
};

use crate::app::{
    execution::Tool,
    plan::{Plan, PlanRelations},
    review::{PlanDocument, PlanMetadata},
};
use crate::infra::CancellationToken;

use super::command::{
    ProcessOutput, ProcessRunner, ProcessStatus, TerraformCommand, TerraformExecutionError,
    TerraformExecutionErrorKind, interrupted_error, non_zero_error, run_command,
};

mod json;
mod metadata;
mod relations;
mod text;

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
            Self::InvalidJson => formatter.write_str("plan JSON is invalid"),
            Self::InvalidUtf8 => formatter.write_str("plan text is not valid UTF-8"),
            Self::RootMustBeObject => formatter.write_str("plan JSON root must be an object"),
            Self::MissingField(field) => {
                write!(formatter, "plan JSON is missing {field}")
            }
            Self::InvalidField(field) => {
                write!(formatter, "plan JSON has an invalid {field}")
            }
            Self::UnsupportedFormatMajor(major) => write!(
                formatter,
                "plan JSON format major version {major} is unsupported"
            ),
        }
    }
}

impl std::error::Error for PlanParseError {}

pub(super) fn read_review_with_arguments(
    tool: Tool,
    root: &Path,
    global_arguments: &[OsString],
    plan_path: &Path,
    plan_changed: bool,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<(PlanDocument, PlanMetadata, Plan, PlanRelations, bool), TerraformExecutionError> {
    let text = run_show(
        tool,
        root,
        global_arguments,
        plan_path,
        false,
        cancellation,
        runner,
    )?;
    let json = run_show(
        tool,
        root,
        global_arguments,
        plan_path,
        true,
        cancellation,
        runner,
    )?;
    let (plan, metadata, relation_analysis) =
        json::parse_plan_json_with_metadata(&json.output.stdout, plan_changed)
            .map_err(|error| invalid_plan(tool, error))?;
    let resource_addresses = plan
        .resource_changes
        .iter()
        .map(|change| change.address.clone())
        .collect::<Vec<_>>();
    let output_names = plan
        .output_changes
        .iter()
        .map(|output| output.address.clone())
        .collect::<Vec<_>>();
    let document = parse_document(text.output.stdout, &resource_addresses, &output_names)
        .map_err(|error| invalid_plan(tool, error))?;
    Ok((
        document,
        metadata,
        plan,
        relation_analysis.relations,
        relation_analysis.has_prior_state,
    ))
}

fn run_show(
    tool: Tool,
    root: &Path,
    global_arguments: &[OsString],
    plan_path: &Path,
    json: bool,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<super::command::ProcessResult, TerraformExecutionError> {
    if cancellation.is_cancelled() {
        return Err(TerraformExecutionError::new_for_tool(
            tool,
            TerraformExecutionErrorKind::Interrupted {
                command: TerraformCommand::Show,
                output: Box::new(ProcessOutput::empty()),
                interrupt_error: None,
            },
        ));
    }
    let mut arguments = global_arguments.to_vec();
    arguments.extend(show_arguments(plan_path, json));
    let output = run_command(
        tool,
        root,
        TerraformCommand::Show,
        &arguments,
        cancellation,
        runner,
    )?;
    if output.interrupted {
        return Err(interrupted_error(tool, TerraformCommand::Show, output));
    }
    if !output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(tool, TerraformCommand::Show, output));
    }
    Ok(output)
}

const fn invalid_plan(tool: Tool, source: PlanParseError) -> TerraformExecutionError {
    TerraformExecutionError::new_for_tool(tool, TerraformExecutionErrorKind::InvalidPlan { source })
}

fn show_arguments(plan_path: &Path, json: bool) -> Vec<OsString> {
    let mut arguments = vec![OsString::from("show")];
    arguments.push(OsString::from(if json { "-json" } else { "-no-color" }));
    arguments.push(plan_path.as_os_str().to_owned());
    arguments
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::app::execution::Tool;
    use crate::app::plan::Plan;

    use super::{
        CancellationToken, Path, ProcessRunner, TerraformExecutionError, invalid_plan, run_show,
    };

    pub(crate) fn read_plan(
        root: &Path,
        plan_path: &Path,
        cancellation: &CancellationToken,
        runner: &dyn ProcessRunner,
    ) -> Result<Plan, TerraformExecutionError> {
        let output = run_show(
            Tool::Terraform,
            root,
            &[],
            plan_path,
            true,
            cancellation,
            runner,
        )?;
        super::json::parse_plan_json_bytes(&output.output.stdout)
            .map_err(|error| invalid_plan(Tool::Terraform, error))
    }
}
