use std::fmt::{Display, Formatter};

use crate::app::plan::Plan;

use super::super::super::command::{
    TerraformCommand, TerraformExecutionErrorKind, interrupted_error, non_zero_error,
    run_command_with_events,
};
use super::super::{
    CancellationToken, ExecutionEvent, ExecutionPhase, OsString, Path, ProcessRunner,
    ProcessStatus, SavedPlan, TerraformExecutionError,
};

#[derive(Debug)]
pub(crate) enum PlanTestError {
    Terraform(TerraformExecutionError),
    TemporaryPlan { message: String },
    Cleanup { message: String },
}

impl From<TerraformExecutionError> for PlanTestError {
    fn from(error: TerraformExecutionError) -> Self {
        Self::Terraform(error)
    }
}

impl Display for PlanTestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Terraform(error) => Display::fmt(error, formatter),
            Self::TemporaryPlan { message } => {
                write!(
                    formatter,
                    "failed to create a temporary Terraform plan: {message}"
                )
            }
            Self::Cleanup { message } => write!(
                formatter,
                "failed to remove the temporary Terraform plan: {message}"
            ),
        }
    }
}

impl std::error::Error for PlanTestError {}

impl PlanTestError {
    pub(crate) const fn kind(&self) -> &TerraformExecutionErrorKind {
        match self {
            Self::Terraform(error) => error.kind(),
            Self::TemporaryPlan { .. } | Self::Cleanup { .. } => {
                panic!("non-Terraform errors have no Terraform error kind")
            }
        }
    }

    pub(crate) fn cleanup_error(&self) -> Option<&str> {
        match self {
            Self::Terraform(error) => error.cleanup_error(),
            Self::TemporaryPlan { .. } | Self::Cleanup { .. } => None,
        }
    }
}

pub(crate) fn run_plan(
    root: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    phase_sink: &mut dyn FnMut(ExecutionPhase),
) -> Result<Plan, PlanTestError> {
    let saved_plan = SavedPlan::create().map_err(|error| PlanTestError::TemporaryPlan {
        message: error.to_string(),
    })?;
    let result = execute_plan(
        root,
        saved_plan.path(),
        cancellation,
        runner,
        event_sink,
        phase_sink,
    );

    finish_plan(saved_plan, result)
}

pub(crate) fn execute_plan(
    root: &Path,
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    phase_sink: &mut dyn FnMut(ExecutionPhase),
) -> Result<Plan, TerraformExecutionError> {
    let plan_arguments = plan_arguments(plan_path);
    let plan_output = run_command_with_events(
        root,
        TerraformCommand::Plan,
        &plan_arguments,
        cancellation,
        runner,
        Some(event_sink),
    )?;
    if plan_output.interrupted {
        return Err(interrupted_error(TerraformCommand::Plan, plan_output));
    }
    if !plan_output.status.is_some_and(ProcessStatus::is_success) {
        return Err(non_zero_error(TerraformCommand::Plan, plan_output));
    }

    phase_sink(ExecutionPhase::Reading);
    super::super::super::show::tests::support::read_plan(root, plan_path, cancellation, runner)
}

pub(crate) fn finish_plan(
    saved_plan: SavedPlan,
    result: Result<Plan, TerraformExecutionError>,
) -> Result<Plan, PlanTestError> {
    match saved_plan.cleanup() {
        Ok(()) => result.map_err(PlanTestError::from),
        Err(error) => match result {
            Ok(_) => Err(PlanTestError::Cleanup {
                message: error.to_string(),
            }),
            Err(execution_error) => Err(PlanTestError::Terraform(
                execution_error.with_cleanup_error(&error),
            )),
        },
    }
}

fn plan_arguments(plan_path: &Path) -> Vec<OsString> {
    let mut output = OsString::from("-out=");
    output.push(plan_path.as_os_str());
    vec![
        OsString::from("plan"),
        OsString::from("-input=false"),
        OsString::from("-json"),
        output,
    ]
}
