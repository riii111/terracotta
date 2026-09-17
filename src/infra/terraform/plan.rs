use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use serde_json::{Map, Value};

use crate::app::plan::{
    Plan, PlanAction, PlanSummary, PlanValue, ReplacePathSegment, ResourceChange,
    ResourceChangeKind, ResourceMode, UnsupportedChange, UnsupportedChangeKind,
    UnsupportedChangeScope,
};

const SUPPORTED_FORMAT_MAJOR: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanParseError {
    InvalidJson,
    RootMustBeObject,
    MissingField(&'static str),
    InvalidField(&'static str),
    UnsupportedFormatMajor(u64),
}

impl Display for PlanParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => formatter.write_str("Terraform plan JSON is invalid"),
            Self::RootMustBeObject => {
                formatter.write_str("Terraform plan JSON root must be an object")
            }
            Self::MissingField(field) => {
                write!(formatter, "Terraform plan JSON is missing {field}")
            }
            Self::InvalidField(field) => {
                write!(formatter, "Terraform plan JSON has an invalid {field}")
            }
            Self::UnsupportedFormatMajor(major) => {
                write!(
                    formatter,
                    "Terraform plan JSON format major version {major} is unsupported"
                )
            }
        }
    }
}

impl std::error::Error for PlanParseError {}

/// Parses the JSON document emitted by `terraform show -json`.
///
/// # Errors
///
/// Returns an error when the JSON is malformed, does not have the required
/// plan shape, or uses an unsupported format major version.
pub fn parse_plan_json(input: &str) -> Result<Plan, PlanParseError> {
    let document = serde_json::from_str::<Value>(input).map_err(|_| PlanParseError::InvalidJson)?;
    parse_plan_document(&document)
}

/// Parses bytes containing the JSON document emitted by `terraform show -json`.
///
/// # Errors
///
/// Returns an error when the bytes are not valid JSON, the document does not
/// have the required plan shape, or it uses an unsupported format major
/// version.
pub fn parse_plan_json_bytes(input: &[u8]) -> Result<Plan, PlanParseError> {
    let document =
        serde_json::from_slice::<Value>(input).map_err(|_| PlanParseError::InvalidJson)?;
    parse_plan_document(&document)
}

fn parse_plan_document(document: &Value) -> Result<Plan, PlanParseError> {
    let root = document
        .as_object()
        .ok_or(PlanParseError::RootMustBeObject)?;
    parse_format_version(root)?;
    let resources = required_array(root, "resource_changes")?;

    let mut changes = Vec::new();
    let mut unsupported_changes = Vec::new();

    for resource in resources {
        parse_resource_change(resource, &mut changes, &mut unsupported_changes)?;
    }

    if let Some(resource_drift) = root.get("resource_drift") {
        parse_resource_drift(resource_drift, &mut unsupported_changes)?;
    }

    if let Some(output_changes) = root.get("output_changes") {
        parse_output_changes(output_changes, &mut unsupported_changes)?;
    }

    let summary = summarize(&changes);

    Ok(Plan {
        changes,
        summary,
        unsupported_changes,
    })
}

fn parse_format_version(root: &Map<String, Value>) -> Result<(), PlanParseError> {
    let version = required_string(root, "format_version")?;
    let mut components = version.split('.');
    let major = components
        .next()
        .and_then(|component| component.parse::<u64>().ok())
        .ok_or(PlanParseError::InvalidField("format_version"))?;
    components
        .next()
        .and_then(|component| component.parse::<u64>().ok())
        .ok_or(PlanParseError::InvalidField("format_version"))?;

    if components.next().is_some() {
        return Err(PlanParseError::InvalidField("format_version"));
    }
    if major != SUPPORTED_FORMAT_MAJOR {
        return Err(PlanParseError::UnsupportedFormatMajor(major));
    }

    Ok(())
}

fn parse_resource_change(
    resource: &Value,
    changes: &mut Vec<ResourceChange>,
    unsupported_changes: &mut Vec<UnsupportedChange>,
) -> Result<(), PlanParseError> {
    let resource = resource
        .as_object()
        .ok_or(PlanParseError::InvalidField("resource_changes item"))?;
    let address = required_string(resource, "address")?.to_owned();
    let mode = parse_resource_mode(resource)?;
    let change = required_object(resource, "change")?;
    let actions = parse_actions(change, "resource change actions")?;
    let is_import = parse_importing(change)?;
    let is_move = parse_optional_string(resource, "previous_address")?.is_some();

    if is_import || is_move {
        unsupported_changes.push(UnsupportedChange {
            scope: UnsupportedChangeScope::Resource,
            address,
            actions,
            kind: if is_import {
                UnsupportedChangeKind::Import
            } else {
                UnsupportedChangeKind::Move
            },
        });
        return Ok(());
    }

    match classify_actions(&actions) {
        ActionClassification::NoOp => Ok(()),
        ActionClassification::Supported(kind) => {
            changes.push(ResourceChange {
                address,
                mode,
                actions,
                kind,
                before: optional_plan_value(change, "before"),
                after: optional_plan_value(change, "after"),
                before_sensitive: optional_plan_value(change, "before_sensitive"),
                after_sensitive: optional_plan_value(change, "after_sensitive"),
                after_unknown: optional_plan_value(change, "after_unknown"),
                replace_paths: parse_replace_paths(change)?,
                action_reason: parse_optional_string(change, "action_reason")?,
            });
            Ok(())
        }
        ActionClassification::Unsupported(kind) => {
            unsupported_changes.push(UnsupportedChange {
                scope: UnsupportedChangeScope::Resource,
                address,
                actions,
                kind,
            });
            Ok(())
        }
    }
}

fn parse_output_changes(
    output_changes: &Value,
    unsupported_changes: &mut Vec<UnsupportedChange>,
) -> Result<(), PlanParseError> {
    if output_changes.is_null() {
        return Ok(());
    }

    let output_changes = output_changes
        .as_object()
        .ok_or(PlanParseError::InvalidField("output_changes"))?;

    for (address, output) in output_changes {
        let output = output
            .as_object()
            .ok_or(PlanParseError::InvalidField("output change"))?;
        let change = required_object(output, "change")?;
        let actions = parse_actions(change, "output change actions")?;

        if !matches!(classify_actions(&actions), ActionClassification::NoOp) {
            unsupported_changes.push(UnsupportedChange {
                scope: UnsupportedChangeScope::Output,
                address: address.clone(),
                actions,
                kind: UnsupportedChangeKind::Output,
            });
        }
    }

    Ok(())
}

fn parse_resource_drift(
    resource_drift: &Value,
    unsupported_changes: &mut Vec<UnsupportedChange>,
) -> Result<(), PlanParseError> {
    if resource_drift.is_null() {
        return Ok(());
    }

    let resource_drift = resource_drift
        .as_array()
        .ok_or(PlanParseError::InvalidField("resource_drift"))?;

    for resource in resource_drift {
        let resource = resource
            .as_object()
            .ok_or(PlanParseError::InvalidField("resource drift item"))?;
        let address = required_string(resource, "address")?.to_owned();
        parse_resource_mode(resource)?;
        let change = required_object(resource, "change")?;
        let actions = parse_actions(change, "resource drift actions")?;

        if !matches!(classify_actions(&actions), ActionClassification::NoOp) {
            unsupported_changes.push(UnsupportedChange {
                scope: UnsupportedChangeScope::ResourceDrift,
                address,
                actions,
                kind: UnsupportedChangeKind::Drift,
            });
        }
    }

    Ok(())
}

fn parse_resource_mode(resource: &Map<String, Value>) -> Result<ResourceMode, PlanParseError> {
    match required_string(resource, "mode")? {
        "managed" => Ok(ResourceMode::Managed),
        "data" => Ok(ResourceMode::Data),
        _ => Err(PlanParseError::InvalidField("resource mode")),
    }
}

fn parse_actions(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Vec<PlanAction>, PlanParseError> {
    let actions = object
        .get("actions")
        .ok_or(PlanParseError::MissingField(field))?
        .as_array()
        .ok_or(PlanParseError::InvalidField(field))?;

    if actions.is_empty() {
        return Err(PlanParseError::InvalidField(field));
    }

    actions
        .iter()
        .map(|action| {
            let action = action.as_str().ok_or(PlanParseError::InvalidField(field))?;
            Ok(match action {
                "create" => PlanAction::Create,
                "read" => PlanAction::Read,
                "update" => PlanAction::Update,
                "delete" => PlanAction::Delete,
                "no-op" => PlanAction::NoOp,
                action => PlanAction::Unknown(action.to_owned()),
            })
        })
        .collect()
}

fn classify_actions(actions: &[PlanAction]) -> ActionClassification {
    match actions {
        [PlanAction::NoOp] => ActionClassification::NoOp,
        [PlanAction::Create] => ActionClassification::Supported(ResourceChangeKind::Create),
        [PlanAction::Update] => ActionClassification::Supported(ResourceChangeKind::Update),
        [PlanAction::Delete] => ActionClassification::Supported(ResourceChangeKind::Delete),
        [PlanAction::Create, PlanAction::Delete] | [PlanAction::Delete, PlanAction::Create] => {
            ActionClassification::Supported(ResourceChangeKind::Replace)
        }
        [PlanAction::Read] => ActionClassification::Unsupported(UnsupportedChangeKind::Read),
        [PlanAction::Unknown(action)] if action == "move" => {
            ActionClassification::Unsupported(UnsupportedChangeKind::Move)
        }
        [PlanAction::Unknown(action)] if action == "import" => {
            ActionClassification::Unsupported(UnsupportedChangeKind::Import)
        }
        actions
            if actions
                .iter()
                .any(|action| matches!(action, PlanAction::Unknown(_))) =>
        {
            ActionClassification::Unsupported(UnsupportedChangeKind::UnknownAction)
        }
        _ => ActionClassification::Unsupported(UnsupportedChangeKind::UnsupportedActions),
    }
}

fn parse_replace_paths(
    change: &Map<String, Value>,
) -> Result<Option<Vec<Vec<ReplacePathSegment>>>, PlanParseError> {
    let Some(value) = change.get("replace_paths") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let paths = value
        .as_array()
        .ok_or(PlanParseError::InvalidField("replace_paths"))?;

    paths
        .iter()
        .map(|path| {
            path.as_array()
                .ok_or(PlanParseError::InvalidField("replace_paths"))?
                .iter()
                .map(|segment| match segment {
                    Value::String(segment) => Ok(ReplacePathSegment::Attribute(segment.clone())),
                    Value::Number(number) => number
                        .as_u64()
                        .map(ReplacePathSegment::Index)
                        .ok_or(PlanParseError::InvalidField("replace_paths")),
                    _ => Err(PlanParseError::InvalidField("replace_paths")),
                })
                .collect()
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn summarize(changes: &[ResourceChange]) -> PlanSummary {
    let mut summary = PlanSummary {
        creates: 0,
        updates: 0,
        replaces: 0,
        deletes: 0,
    };

    for change in changes {
        match change.kind {
            ResourceChangeKind::Create => summary.creates += 1,
            ResourceChangeKind::Update => summary.updates += 1,
            ResourceChangeKind::Replace => summary.replaces += 1,
            ResourceChangeKind::Delete => summary.deletes += 1,
        }
    }

    summary
}

fn required_array<'a>(
    object: &'a Map<String, Value>,
    field: &'static str,
) -> Result<&'a Vec<Value>, PlanParseError> {
    object
        .get(field)
        .ok_or(PlanParseError::MissingField(field))?
        .as_array()
        .ok_or(PlanParseError::InvalidField(field))
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

fn optional_plan_value(object: &Map<String, Value>, field: &str) -> Option<PlanValue> {
    object.get(field).map(plan_value)
}

fn plan_value(value: &Value) -> PlanValue {
    match value {
        Value::Null => PlanValue::Null,
        Value::Bool(value) => PlanValue::Bool(*value),
        Value::Number(value) => PlanValue::Number(value.to_string()),
        Value::String(value) => PlanValue::String(value.clone()),
        Value::Array(values) => PlanValue::Array(values.iter().map(plan_value).collect()),
        Value::Object(values) => PlanValue::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), plan_value(value)))
                .collect::<BTreeMap<_, _>>(),
        ),
    }
}

fn parse_optional_string(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<String>, PlanParseError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(str::to_owned)
            .ok_or(PlanParseError::InvalidField(field))
            .map(Some),
    }
}

fn parse_importing(change: &Map<String, Value>) -> Result<bool, PlanParseError> {
    match change.get("importing") {
        None => Ok(false),
        Some(value) => {
            let importing = value
                .as_object()
                .ok_or(PlanParseError::InvalidField("importing"))?;
            let import_id = importing
                .get("id")
                .ok_or(PlanParseError::MissingField("import id"))?;
            if !import_id.is_string() {
                return Err(PlanParseError::InvalidField("import id"));
            }
            Ok(true)
        }
    }
}

enum ActionClassification {
    NoOp,
    Supported(ResourceChangeKind),
    Unsupported(UnsupportedChangeKind),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn plan_with_resources(resources: Value) -> String {
        let mut document = Map::new();
        document.insert("format_version".to_owned(), json!("1.2"));
        document.insert("terraform_version".to_owned(), json!("1.9.0"));
        document.insert("resource_changes".to_owned(), resources);
        document.insert("output_changes".to_owned(), json!({}));

        serde_json::to_string(&Value::Object(document)).expect("synthetic plan should serialize")
    }

    fn resource(address: &str, mode: &str, actions: Value) -> Value {
        let mut change = Map::new();
        change.insert("actions".to_owned(), actions);
        change.insert("before".to_owned(), Value::Null);
        change.insert("after".to_owned(), json!({"id": address}));
        change.insert("before_sensitive".to_owned(), json!(false));
        change.insert("after_sensitive".to_owned(), json!({"id": false}));
        change.insert("after_unknown".to_owned(), json!({"id": true}));
        change.insert("replace_paths".to_owned(), json!([["id"]]));

        let mut resource = Map::new();
        resource.insert("address".to_owned(), json!(address));
        resource.insert("mode".to_owned(), json!(mode));
        resource.insert("type".to_owned(), json!("synthetic_resource"));
        resource.insert("name".to_owned(), json!("example"));
        resource.insert("change".to_owned(), Value::Object(change));

        Value::Object(resource)
    }

    #[test]
    fn reads_supported_resource_changes_and_summarizes_each_kind() {
        let input = plan_with_resources(json!([
            resource("aws_vpc.main", "managed", json!(["create"])),
            resource("aws_subnet.private", "managed", json!(["update"])),
            resource("aws_instance.api", "managed", json!(["create", "delete"])),
            resource("aws_instance.worker", "data", json!(["delete"])),
            resource("aws_instance.noop", "managed", json!(["no-op"]))
        ]));

        let plan = parse_plan_json(&input).expect("plan should parse");

        assert_eq!(plan.changes.len(), 4);
        assert_eq!(plan.changes[0].kind, ResourceChangeKind::Create);
        assert_eq!(plan.changes[1].kind, ResourceChangeKind::Update);
        assert_eq!(plan.changes[2].kind, ResourceChangeKind::Replace);
        assert_eq!(plan.changes[3].kind, ResourceChangeKind::Delete);
        assert_eq!(plan.changes[3].mode, ResourceMode::Data);
        assert_eq!(plan.summary.creates, 1);
        assert_eq!(plan.summary.updates, 1);
        assert_eq!(plan.summary.replaces, 1);
        assert_eq!(plan.summary.deletes, 1);
        assert_eq!(plan.summary.total(), 4);
    }

    #[test]
    fn treats_both_replacement_orders_as_one_replace() {
        let input = plan_with_resources(json!([
            resource(
                "aws_instance.create_first",
                "managed",
                json!(["create", "delete"])
            ),
            resource(
                "aws_instance.delete_first",
                "managed",
                json!(["delete", "create"])
            )
        ]));

        let plan = parse_plan_json(&input).expect("plan should parse");

        assert_eq!(plan.summary.replaces, 2);
        assert_eq!(plan.summary.total(), 2);
        assert_eq!(
            plan.changes[0].actions,
            vec![PlanAction::Create, PlanAction::Delete]
        );
        assert_eq!(
            plan.changes[1].actions,
            vec![PlanAction::Delete, PlanAction::Create]
        );
    }

    #[test]
    fn preserves_change_values_and_replacement_paths_for_follow_up_diffing() {
        let input = plan_with_resources(json!([resource(
            "aws_instance.api",
            "managed",
            json!(["update"])
        )]));

        let plan = parse_plan_json(&input).expect("plan should parse");
        let change = &plan.changes[0];

        assert_eq!(change.before, Some(PlanValue::Null));
        assert_eq!(
            change.after,
            Some(plan_value(&json!({"id": "aws_instance.api"})))
        );
        assert_eq!(change.before_sensitive, Some(PlanValue::Bool(false)));
        assert_eq!(
            change.after_sensitive,
            Some(plan_value(&json!({"id": false})))
        );
        assert_eq!(change.after_unknown, Some(plan_value(&json!({"id": true}))));
        assert_eq!(
            change.replace_paths,
            Some(vec![vec![ReplacePathSegment::Attribute("id".to_owned())]])
        );
    }

    #[test]
    fn redacts_attribute_values_from_plan_debug_output() {
        let mut resource = resource("aws_instance.api", "managed", json!(["update"]));
        resource["change"]["after"] = json!("synthetic-secret");

        let plan =
            parse_plan_json(&plan_with_resources(json!([resource]))).expect("plan should parse");
        let debug = format!("{plan:?}");

        assert!(!debug.contains("synthetic-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn retains_move_and_import_markers_as_unsupported_changes() {
        let mut moved = resource("aws_instance.renamed", "managed", json!(["no-op"]));
        moved["previous_address"] = json!("aws_instance.old_name");

        let mut imported = resource("aws_instance.imported", "managed", json!(["create"]));
        imported["change"]["importing"] = json!({"id": "synthetic-import-id"});

        let plan = parse_plan_json(&plan_with_resources(json!([moved, imported])))
            .expect("plan should parse");

        assert!(plan.changes.is_empty());
        assert_eq!(plan.unsupported_change_count(), 2);
        assert_eq!(
            plan.unsupported_changes[0].kind,
            UnsupportedChangeKind::Move
        );
        assert_eq!(
            plan.unsupported_changes[1].kind,
            UnsupportedChangeKind::Import
        );
    }

    #[test]
    fn retains_unsupported_resource_and_output_changes_without_listing_them() {
        let mut document = json!({
            "format_version": "1.0",
            "resource_changes": [
                resource("aws_instance.read", "managed", json!(["read"])),
                resource("aws_instance.move", "managed", json!(["move"])),
                resource("aws_instance.import", "managed", json!(["import"])),
                resource("aws_instance.unknown", "managed", json!(["future-action"]))
            ],
            "output_changes": {
                "public_ip": {
                    "change": {"actions": ["update"], "before": null, "after": "synthetic"}
                }
            }
        });
        document["extra_future_field"] = json!({"accepted": true});

        let plan = parse_plan_json(&document.to_string()).expect("plan should parse");

        assert!(plan.changes.is_empty());
        assert_eq!(plan.unsupported_change_count(), 5);
        assert_eq!(
            plan.unsupported_changes[0].kind,
            UnsupportedChangeKind::Read
        );
        assert_eq!(
            plan.unsupported_changes[1].kind,
            UnsupportedChangeKind::Move
        );
        assert_eq!(
            plan.unsupported_changes[2].kind,
            UnsupportedChangeKind::Import
        );
        assert_eq!(
            plan.unsupported_changes[3].kind,
            UnsupportedChangeKind::UnknownAction
        );
        assert_eq!(
            plan.unsupported_changes[4].scope,
            UnsupportedChangeScope::Output
        );
        assert_eq!(plan.unsupported_changes[4].address, "public_ip");
    }

    #[test]
    fn retains_resource_drift_as_an_unsupported_change() {
        let input = json!({
            "format_version": "1.2",
            "resource_changes": [],
            "resource_drift": [resource(
                "aws_instance.drifted",
                "managed",
                json!(["update"])
            )],
            "output_changes": null
        });

        let plan = parse_plan_json(&input.to_string()).expect("plan should parse");

        assert!(plan.changes.is_empty());
        assert_eq!(plan.unsupported_change_count(), 1);
        assert_eq!(
            plan.unsupported_changes[0].scope,
            UnsupportedChangeScope::ResourceDrift
        );
        assert_eq!(
            plan.unsupported_changes[0].kind,
            UnsupportedChangeKind::Drift
        );
    }

    #[test]
    fn preserves_numeric_replacement_path_steps() {
        let mut resource = resource("aws_instance.api", "managed", json!(["replace"]));
        resource["change"]["actions"] = json!(["delete", "create"]);
        resource["change"]["replace_paths"] = json!([["disks", 0, "size"]]);

        let plan =
            parse_plan_json(&plan_with_resources(json!([resource]))).expect("plan should parse");

        assert_eq!(
            plan.changes[0].replace_paths,
            Some(vec![vec![
                ReplacePathSegment::Attribute("disks".to_owned()),
                ReplacePathSegment::Index(0),
                ReplacePathSegment::Attribute("size".to_owned()),
            ]])
        );
    }

    #[test]
    fn accepts_empty_plan_and_plan_with_only_noop_resources() {
        let empty =
            parse_plan_json(&plan_with_resources(json!([]))).expect("empty plan should parse");
        assert!(!empty.has_changes());
        assert_eq!(empty.summary.total(), 0);

        let noops = parse_plan_json(&plan_with_resources(json!([resource(
            "aws_instance.noop",
            "managed",
            json!(["no-op"])
        )])))
        .expect("no-op plan should parse");
        assert!(noops.changes.is_empty());
        assert!(noops.unsupported_changes.is_empty());
    }

    #[test]
    fn accepts_null_optional_sections_from_terraform_show() {
        let mut resource = resource("terraform_data.api", "managed", json!(["update"]));
        resource["change"]["replace_paths"] = Value::Null;

        let input = json!({
            "format_version": "1.2",
            "resource_changes": [resource],
            "output_changes": null
        });

        let plan = parse_plan_json(&input.to_string()).expect("Terraform plan should parse");

        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].replace_paths, None);
        assert!(plan.unsupported_changes.is_empty());
    }

    #[test]
    fn reports_schema_and_version_errors_without_attribute_values() {
        let missing_actions = plan_with_resources(json!([{
            "address": "aws_instance.secret",
            "mode": "managed",
            "change": {"after": "synthetic-secret"}
        }]));
        let error = parse_plan_json(&missing_actions).expect_err("missing actions should fail");
        assert_eq!(
            error,
            PlanParseError::MissingField("resource change actions")
        );
        assert!(!error.to_string().contains("synthetic-secret"));

        let invalid_version = serde_json::json!({
            "format_version": "2.0",
            "resource_changes": []
        });
        assert_eq!(
            parse_plan_json(&invalid_version.to_string()),
            Err(PlanParseError::UnsupportedFormatMajor(2))
        );

        assert_eq!(
            parse_plan_json("not json"),
            Err(PlanParseError::InvalidJson)
        );
        assert_eq!(
            parse_plan_json(r#"{"format_version":"1.0"}"#),
            Err(PlanParseError::MissingField("resource_changes"))
        );
    }
}
