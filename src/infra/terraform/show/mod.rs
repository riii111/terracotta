use std::{
    ffi::OsString,
    fmt::{Debug, Formatter},
    path::Path,
};

use crate::app::plan::Plan;

use super::command::{
    CancellationToken, ProcessOutput, ProcessRunner, ProcessStatus, TerraformCommand,
    TerraformExecutionError, TerraformExecutionErrorKind, interrupted_error, non_zero_error,
    run_command,
};

mod json;

pub(super) use json::PlanParseError;
use json::parse_plan_json_bytes;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PlanExecution {
    json: Vec<u8>,
    plan: Plan,
}

impl PlanExecution {
    #[must_use]
    pub(crate) fn json(&self) -> &[u8] {
        &self.json
    }

    #[must_use]
    pub(crate) const fn plan(&self) -> &Plan {
        &self.plan
    }
}

impl Debug for PlanExecution {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlanExecution")
            .field("json", &"<redacted>")
            .field("plan", &self.plan)
            .finish()
    }
}

pub(super) fn read_plan(
    root: &Path,
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<PlanExecution, TerraformExecutionError> {
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

    Ok(PlanExecution { json, plan })
}

fn show_arguments(plan_path: &Path) -> Vec<OsString> {
    vec![
        OsString::from("show"),
        OsString::from("-json"),
        plan_path.as_os_str().to_owned(),
    ]
}
