use serde_json::{Map, Value};

use crate::app::plan::{PlanAction, PlanResource};
use crate::app::review::PlanMetadata;

use super::PlanParseError;

const SUPPORTED_FORMAT_MAJOR: u64 = 1;

pub(super) fn parse_metadata(
    input: &[u8],
    detailed_exit_has_changes: bool,
) -> Result<PlanMetadata, PlanParseError> {
    let document =
        serde_json::from_slice::<Value>(input).map_err(|_| PlanParseError::InvalidJson)?;
    let root = document
        .as_object()
        .ok_or(PlanParseError::RootMustBeObject)?;
    parse_format_version(root)?;

    let resources = optional_array(root, "resource_changes")?;
    let mut resource_addresses = Vec::with_capacity(resources.len());
    let mut resource_changes = Vec::with_capacity(resources.len());
    let mut additions = 0;
    let mut changes = 0;
    let mut replacements = 0;
    let mut deletions = 0;
    for resource in resources {
        let resource = resource
            .as_object()
            .ok_or(PlanParseError::InvalidField("resource change"))?;
        let address = required_string(resource, "address")?.to_owned();
        let change = required_object(resource, "change")?;
        let actions = super::json::parse_actions(change, "resource change actions")?;
        let plan_resource = PlanResource { address, actions };
        match plan_resource.actions.as_slice() {
            [PlanAction::Create] => additions += 1,
            [PlanAction::Update] => changes += 1,
            [PlanAction::Delete] => deletions += 1,
            [PlanAction::Create, PlanAction::Delete] | [PlanAction::Delete, PlanAction::Create] => {
                replacements += 1;
            }
            _ => {}
        }
        resource_addresses.push(plan_resource.address.clone());
        resource_changes.push(plan_resource);
    }

    super::json::parse_plan_json_bytes(input)?;

    let output_names = root
        .get("output_changes")
        .and_then(Value::as_object)
        .map(|outputs| outputs.keys().cloned().collect())
        .unwrap_or_default();
    let errored = root.get("errored").and_then(Value::as_bool) == Some(true);
    let applyable = !errored
        && root
            .get("applyable")
            .and_then(Value::as_bool)
            .unwrap_or(detailed_exit_has_changes);

    Ok(PlanMetadata::new(
        resource_addresses,
        output_names,
        additions,
        changes,
        deletions,
        applyable,
    )
    .with_resource_changes(resource_changes, replacements))
}

fn parse_format_version(root: &Map<String, Value>) -> Result<(), PlanParseError> {
    let version = required_string(root, "format_version")?;
    let major = version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u64>().ok())
        .ok_or(PlanParseError::InvalidField("format_version"))?;
    if major == SUPPORTED_FORMAT_MAJOR {
        Ok(())
    } else {
        Err(PlanParseError::UnsupportedFormatMajor(major))
    }
}

fn optional_array<'a>(
    object: &'a Map<String, Value>,
    field: &'static str,
) -> Result<&'a [Value], PlanParseError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(&[]),
        Some(value) => value
            .as_array()
            .map(Vec::as_slice)
            .ok_or(PlanParseError::InvalidField(field)),
    }
}

fn required_object<'a>(
    object: &'a Map<String, Value>,
    field: &'static str,
) -> Result<&'a Map<String, Value>, PlanParseError> {
    object
        .get(field)
        .ok_or(PlanParseError::MissingField(field))?
        .as_object()
        .ok_or(PlanParseError::InvalidField(field))
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &'static str,
) -> Result<&'a str, PlanParseError> {
    object
        .get(field)
        .ok_or(PlanParseError::MissingField(field))?
        .as_str()
        .ok_or(PlanParseError::InvalidField(field))
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

        assert_eq!(metadata.replacements(), 2);
        assert_eq!(metadata.deletions(), 2);
        assert_eq!(
            metadata.replacement_addresses().collect::<Vec<_>>(),
            ["terraform_data.create_first", "terraform_data.delete_first"]
        );
        assert_eq!(
            metadata.destructive_addresses().collect::<Vec<_>>(),
            ["terraform_data.destroy", "terraform_data.moved_destroy"]
        );
    }
}
