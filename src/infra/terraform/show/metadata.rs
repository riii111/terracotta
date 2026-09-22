use serde_json::{Map, Value};

use crate::app::execution::{ExecutionTargetSpec, SensitiveValue};
use crate::app::plan::{Plan, PlanResource};
use crate::app::review::PlanMetadata;

#[cfg(test)]
use super::PlanParseError;

pub(super) fn metadata_from_document(
    root: &Map<String, Value>,
    plan: &Plan,
    detailed_exit_has_changes: bool,
) -> PlanMetadata {
    let resource_addresses = plan
        .resource_changes
        .iter()
        .map(|change| change.address.clone())
        .collect::<Vec<_>>();
    let resource_changes = plan
        .resource_changes
        .iter()
        .map(|change| PlanResource {
            address: change.address.clone(),
            actions: change.actions.clone(),
            kind: change.kind,
        })
        .collect::<Vec<_>>();
    let mut apply_targets = Vec::new();
    for resource in &plan.resource_changes {
        if resource.kind.is_standard_change()
            && resource.previous_address.is_none()
            && resource.importing.is_none()
        {
            apply_targets.push(ExecutionTargetSpec {
                address: resource.address.clone(),
                actions: resource.actions.clone(),
            });
        }
    }

    let output_names = root
        .get("output_changes")
        .and_then(Value::as_object)
        .map(|outputs| outputs.keys().cloned().collect())
        .unwrap_or_default();
    let sensitive_values = sensitive_values(root);
    let errored = root.get("errored").and_then(Value::as_bool) == Some(true);
    let applyable = !errored
        && root
            .get("applyable")
            .and_then(Value::as_bool)
            .unwrap_or(detailed_exit_has_changes);

    PlanMetadata::new(
        resource_addresses,
        output_names,
        plan.summary.creates,
        plan.summary.updates,
        plan.summary.deletes,
        applyable,
    )
    .with_resource_changes(resource_changes, plan.summary.replaces)
    .with_apply_targets(apply_targets)
    .with_sensitive_values(sensitive_values)
}

fn sensitive_values(root: &Map<String, Value>) -> Vec<SensitiveValue> {
    let mut values = Vec::new();
    for resource in root
        .get("resource_changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        let Some(change) = resource.get("change").and_then(Value::as_object) else {
            continue;
        };
        for (value_field, mask_field) in
            [("before", "before_sensitive"), ("after", "after_sensitive")]
        {
            if let (Some(value), Some(mask)) = (change.get(value_field), change.get(mask_field)) {
                collect_masked_values(value, mask, &mut values);
            }
        }
    }
    if let Some(outputs) = root.get("output_changes").and_then(Value::as_object) {
        for output in outputs.values().filter_map(Value::as_object) {
            let change = output
                .get("change")
                .and_then(Value::as_object)
                .unwrap_or(output);
            if change.get("sensitive") == Some(&Value::Bool(true))
                && let Some(value) = change.get("value").or_else(|| change.get("after"))
            {
                collect_scalar_values(value, &mut values);
            }
            for (value_field, mask_field) in
                [("before", "before_sensitive"), ("after", "after_sensitive")]
            {
                if let (Some(value), Some(mask)) = (change.get(value_field), change.get(mask_field))
                {
                    collect_masked_values(value, mask, &mut values);
                }
            }
        }
    }
    values.retain(|value| match value {
        SensitiveValue::Text(value) | SensitiveValue::Number(value) => !value.is_empty(),
        SensitiveValue::Bool(_) => true,
    });
    values.sort();
    values.dedup();
    values
}

fn collect_masked_values(value: &Value, mask: &Value, values: &mut Vec<SensitiveValue>) {
    match mask {
        Value::Bool(true) => collect_scalar_values(value, values),
        Value::Object(mask) => {
            let Some(value) = value.as_object() else {
                return;
            };
            for (key, mask) in mask {
                if let Some(value) = value.get(key) {
                    collect_masked_values(value, mask, values);
                }
            }
        }
        Value::Array(mask) => {
            let Some(value) = value.as_array() else {
                return;
            };
            for (value, mask) in value.iter().zip(mask) {
                collect_masked_values(value, mask, values);
            }
        }
        _ => {}
    }
}

fn collect_scalar_values(value: &Value, values: &mut Vec<SensitiveValue>) {
    match value {
        Value::String(value) => values.push(SensitiveValue::Text(value.clone())),
        Value::Number(value) => values.push(SensitiveValue::Number(value.to_string())),
        Value::Bool(value) => values.push(SensitiveValue::Bool(*value)),
        Value::Array(values_array) => {
            for value in values_array {
                collect_scalar_values(value, values);
            }
        }
        Value::Object(values_object) => {
            for value in values_object.values() {
                collect_scalar_values(value, values);
            }
        }
        Value::Null => {}
    }
}

#[cfg(test)]
fn parse_metadata(
    input: &[u8],
    detailed_exit_has_changes: bool,
) -> Result<PlanMetadata, PlanParseError> {
    let document =
        serde_json::from_slice::<Value>(input).map_err(|_| PlanParseError::InvalidJson)?;
    let root = document
        .as_object()
        .ok_or(PlanParseError::RootMustBeObject)?;
    let plan = super::json::parse_plan_document(&document)?;
    Ok(metadata_from_document(
        root,
        &plan,
        detailed_exit_has_changes,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn extracts_boundaries_counts_and_output_only_applyability_without_values() {
        let document = json!({
            "format_version": "1.2",
            "resource_changes": [
                {"address": "terraform_data.replace", "change": {"actions": ["delete", "create"], "after": "secret"}},
                {"address": "terraform_data.update", "change": {"actions": ["update"]}}
            ],
            "output_changes": {"endpoint": {"after": "secret-output"}}
        });

        let metadata =
            parse_metadata(document.to_string().as_bytes(), true).expect("metadata should parse");

        assert_eq!(metadata.additions(), 0);
        assert_eq!(metadata.changes(), 1);
        assert_eq!(metadata.replacements(), 1);
        assert_eq!(metadata.deletions(), 0);
        assert_eq!(metadata.resource_addresses().len(), 2);
        assert!(
            metadata
                .output_names()
                .iter()
                .any(|output| output == "endpoint")
        );
        assert!(metadata.applyable());
        assert_eq!(
            metadata.replacement_addresses().collect::<Vec<_>>(),
            ["terraform_data.replace"]
        );
        let debug = format!("{metadata:?}");
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn errored_plan_is_never_applyable_and_exit_zero_is_no_change() {
        let errored = json!({"format_version": "1.0", "errored": true, "applyable": true});
        let errored_metadata =
            parse_metadata(errored.to_string().as_bytes(), true).expect("metadata should parse");
        assert!(!errored_metadata.applyable());

        let no_change = json!({"format_version": "1.0"});
        let no_change_metadata =
            parse_metadata(no_change.to_string().as_bytes(), false).expect("metadata should parse");
        assert!(!no_change_metadata.applyable());
    }

    #[test]
    fn output_only_import_and_move_metadata_do_not_require_create_counts() {
        let document = json!({
            "format_version": "1.0",
            "applyable": true,
            "resource_changes": [{
                "address": "terraform_data.moved_or_imported",
                "previous_address": "terraform_data.previous",
                "importing": {"id": "example"},
                "change": {"actions": ["no-op"]}
            }],
            "output_changes": {"endpoint": {"actions": ["update"]}}
        });

        let metadata =
            parse_metadata(document.to_string().as_bytes(), true).expect("metadata should parse");

        assert_eq!(metadata.additions(), 0);
        assert_eq!(metadata.changes(), 0);
        assert_eq!(metadata.deletions(), 0);
        assert!(
            metadata
                .output_names()
                .iter()
                .any(|output| output == "endpoint")
        );
        assert!(metadata.applyable());
    }

    #[test]
    fn lists_replacements_in_either_order_without_counting_them_as_deletes() {
        let document = json!({
            "format_version": "1.0",
            "applyable": true,
            "resource_changes": [
                {"address": "terraform_data.create_first", "previous_address": "terraform_data.old", "change": {"actions": ["create", "delete"]}},
                {"address": "terraform_data.delete_first", "change": {"actions": ["delete", "create"]}},
                {"address": "terraform_data.destroy", "change": {"actions": ["delete"]}},
                {"address": "terraform_data.moved_destroy", "previous_address": "terraform_data.previous", "change": {"actions": ["delete"]}}
            ]
        });

        let metadata =
            parse_metadata(document.to_string().as_bytes(), true).expect("metadata should parse");

        assert_eq!(metadata.replacements(), 1);
        assert_eq!(metadata.deletions(), 1);
        assert_eq!(
            metadata.replacement_addresses().collect::<Vec<_>>(),
            ["terraform_data.delete_first"]
        );
        assert_eq!(
            metadata.destructive_addresses().collect::<Vec<_>>(),
            ["terraform_data.destroy"]
        );
    }

    #[test]
    fn no_op_resources_are_not_reported_as_changes() {
        let document = json!({
            "format_version": "1.0",
            "resource_changes": [
                {"address": "terraform_data.unchanged", "change": {"actions": ["no-op"]}}
            ]
        });

        let metadata =
            parse_metadata(document.to_string().as_bytes(), true).expect("metadata should parse");

        assert!(!metadata.has_changes());
    }

    #[test]
    fn extracts_apply_targets_and_sensitive_scalars_without_debug_leaks() {
        let document = json!({
            "format_version": "1.0",
            "applyable": true,
            "resource_changes": [
                {
                    "address": "terraform_data.api",
                    "change": {
                        "actions": ["update"],
                        "before": {"token": "old-secret"},
                        "before_sensitive": {"token": true},
                        "after": {"token": "new-secret"},
                        "after_sensitive": {"token": true}
                    }
                },
                {
                    "address": "terraform_data.imported",
                    "change": {
                        "actions": ["create"],
                        "importing": {"id": "import-id"}
                    }
                },
                {
                    "address": "terraform_data.moved",
                    "previous_address": "terraform_data.old",
                    "change": {"actions": ["create"]}
                }
            ],
            "output_changes": {
                "endpoint": {"change": {
                    "actions": ["update"],
                    "before": "previous-output-secret",
                    "before_sensitive": true,
                    "after": "output-secret",
                    "after_sensitive": true
                }}
            }
        });

        let metadata =
            parse_metadata(document.to_string().as_bytes(), true).expect("metadata should parse");

        assert_eq!(metadata.apply_targets().len(), 1);
        assert_eq!(metadata.apply_targets()[0].address, "terraform_data.api");
        assert_eq!(
            metadata.sensitive_values(),
            [
                SensitiveValue::Text("new-secret".to_owned()),
                SensitiveValue::Text("old-secret".to_owned()),
                SensitiveValue::Text("output-secret".to_owned()),
                SensitiveValue::Text("previous-output-secret".to_owned()),
            ]
        );
        let debug = format!("{metadata:?}");
        assert!(!debug.contains("old-secret"));
        assert!(!debug.contains("new-secret"));
        assert!(!debug.contains("output-secret"));
        assert!(!debug.contains("previous-output-secret"));
    }
}
