use crate::app::plan::Plan;

use super::super::{
    CancellationToken, Path, ProcessRunner, TerraformExecutionError, invalid_plan, run_show,
};

pub(crate) fn read_plan(
    root: &Path,
    plan_path: &Path,
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<Plan, TerraformExecutionError> {
    let output = run_show(root, &[], plan_path, true, cancellation, runner)?;
    super::super::json::parse_plan_json_bytes(&output.output.stdout).map_err(invalid_plan)
}
