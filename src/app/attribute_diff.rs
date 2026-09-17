use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};

use super::plan::{PlanValue, ReplacePathSegment, ResourceChange};

const ABSENT_DISPLAY: &str = "<absent>";
const SENSITIVE_DISPLAY: &str = "<sensitive>";
const UNKNOWN_DISPLAY: &str = "<unknown>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttributePathSegment {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttributeValueKind {
    Absent,
    Null,
    Unknown,
    Known,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AttributeValue {
    kind: AttributeValueKind,
    original: Option<PlanValue>,
    sensitive: bool,
}

impl Debug for AttributeValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AttributeValue")
            .field("kind", &self.kind)
            .field("sensitive", &self.sensitive)
            .field("original", &self.original.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl AttributeValue {
    #[must_use]
    pub(crate) const fn kind(&self) -> AttributeValueKind {
        self.kind
    }

    #[must_use]
    pub(crate) const fn is_sensitive(&self) -> bool {
        self.sensitive
    }

    #[must_use]
    pub(crate) const fn is_unknown(&self) -> bool {
        matches!(self.kind, AttributeValueKind::Unknown)
    }

    #[must_use]
    pub(crate) fn display(&self) -> String {
        if self.sensitive && !matches!(self.kind, AttributeValueKind::Absent) {
            return SENSITIVE_DISPLAY.to_owned();
        }

        match self.kind {
            AttributeValueKind::Absent => ABSENT_DISPLAY.to_owned(),
            AttributeValueKind::Null => "null".to_owned(),
            AttributeValueKind::Unknown => UNKNOWN_DISPLAY.to_owned(),
            AttributeValueKind::Known => self
                .original
                .as_ref()
                .map_or_else(|| ABSENT_DISPLAY.to_owned(), display_plan_value),
        }
    }

    #[must_use]
    pub(crate) const fn revealed(&self) -> Option<&PlanValue> {
        self.original.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttributeChangeKind {
    Changed,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributeDiff {
    pub(crate) path: Vec<AttributePathSegment>,
    pub(crate) before: AttributeValue,
    pub(crate) after: AttributeValue,
    pub(crate) kind: AttributeChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributeDiffs {
    pub(crate) attributes: Vec<AttributeDiff>,
    pub(crate) changed_count: usize,
    pub(crate) unchanged_count: usize,
    pub(crate) replace_paths: Option<Vec<Vec<ReplacePathSegment>>>,
    pub(crate) action_reason: Option<String>,
}

impl AttributeDiffs {
    #[must_use]
    pub(crate) const fn total_count(&self) -> usize {
        self.changed_count + self.unchanged_count
    }
}

pub(crate) fn diff_resource_attributes(change: &ResourceChange) -> AttributeDiffs {
    let mut attributes = Vec::new();
    collect_diffs(
        &mut attributes,
        Vec::new(),
        DiffInput {
            before: change.before.as_ref(),
            after: change.after.as_ref(),
            before_sensitive: change.before_sensitive.as_ref(),
            after_sensitive: change.after_sensitive.as_ref(),
            after_unknown: change.after_unknown.as_ref(),
        },
        false,
        false,
    );

    let changed_count = attributes
        .iter()
        .filter(|attribute| attribute.kind == AttributeChangeKind::Changed)
        .count();
    let unchanged_count = attributes.len() - changed_count;

    AttributeDiffs {
        attributes,
        changed_count,
        unchanged_count,
        replace_paths: change.replace_paths.clone(),
        action_reason: change.action_reason.clone(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
    Object,
    Array,
}

#[derive(Clone, Copy)]
struct DiffInput<'a> {
    before: Option<&'a PlanValue>,
    after: Option<&'a PlanValue>,
    before_sensitive: Option<&'a PlanValue>,
    after_sensitive: Option<&'a PlanValue>,
    after_unknown: Option<&'a PlanValue>,
}

impl DiffInput<'_> {
    fn child(self, segment: &AttributePathSegment) -> Self {
        Self {
            before: child_value(self.before, segment),
            after: child_value(self.after, segment),
            before_sensitive: child_value(self.before_sensitive, segment),
            after_sensitive: child_value(self.after_sensitive, segment),
            after_unknown: child_value(self.after_unknown, segment),
        }
    }
}

fn collect_diffs(
    attributes: &mut Vec<AttributeDiff>,
    path: Vec<AttributePathSegment>,
    input: DiffInput<'_>,
    inherited_before_sensitive: bool,
    inherited_after_sensitive: bool,
) {
    let before_is_sensitive = inherited_before_sensitive || marker_is_true(input.before_sensitive);
    let after_is_sensitive = inherited_after_sensitive || marker_is_true(input.after_sensitive);

    if marker_is_true(input.after_unknown) {
        push_diff(
            attributes,
            path,
            input,
            before_is_sensitive,
            after_is_sensitive,
        );
        return;
    }

    let Some(container_kind) = container_kind(input) else {
        push_diff(
            attributes,
            path,
            input,
            before_is_sensitive,
            after_is_sensitive,
        );
        return;
    };

    let children = child_segments(container_kind, input);
    if children.is_empty() {
        push_diff(
            attributes,
            path,
            input,
            before_is_sensitive,
            after_is_sensitive,
        );
        return;
    }

    for segment in children {
        let mut child_path = path.clone();
        child_path.push(segment.clone());
        collect_diffs(
            attributes,
            child_path,
            input.child(&segment),
            before_is_sensitive,
            after_is_sensitive,
        );
    }
}

fn push_diff(
    attributes: &mut Vec<AttributeDiff>,
    path: Vec<AttributePathSegment>,
    input: DiffInput<'_>,
    inherited_before_sensitive: bool,
    inherited_after_sensitive: bool,
) {
    let before_value = attribute_value(
        input.before,
        input.before_sensitive,
        None,
        inherited_before_sensitive,
    );
    let after_value = attribute_value(
        input.after,
        input.after_sensitive,
        input.after_unknown,
        inherited_after_sensitive,
    );
    let kind = if same_attribute_value(&before_value, &after_value) {
        AttributeChangeKind::Unchanged
    } else {
        AttributeChangeKind::Changed
    };

    attributes.push(AttributeDiff {
        path,
        before: before_value,
        after: after_value,
        kind,
    });
}

fn attribute_value(
    value: Option<&PlanValue>,
    sensitive: Option<&PlanValue>,
    unknown: Option<&PlanValue>,
    inherited_sensitive: bool,
) -> AttributeValue {
    let unknown = marker_is_true(unknown);
    let sensitive = inherited_sensitive || marker_contains_true(sensitive);
    let kind = if unknown {
        AttributeValueKind::Unknown
    } else {
        match value {
            None => AttributeValueKind::Absent,
            Some(PlanValue::Null) => AttributeValueKind::Null,
            Some(_) => AttributeValueKind::Known,
        }
    };

    AttributeValue {
        kind,
        original: value.cloned(),
        sensitive,
    }
}

fn same_attribute_value(before: &AttributeValue, after: &AttributeValue) -> bool {
    before.kind == after.kind && before.original == after.original
}

fn container_kind(input: DiffInput<'_>) -> Option<ContainerKind> {
    match (input.before, input.after) {
        (Some(before), Some(after)) => {
            matching_container_kinds(value_container_kind(before), value_container_kind(after))
        }
        (Some(before), None) => value_container_kind(before),
        (None, Some(after)) => value_container_kind(after),
        (None, None) => matching_container_kinds(
            metadata_container_kind(input.before_sensitive),
            matching_container_kinds(
                metadata_container_kind(input.after_sensitive),
                metadata_container_kind(input.after_unknown),
            ),
        ),
    }
}

fn matching_container_kinds(
    first: Option<ContainerKind>,
    second: Option<ContainerKind>,
) -> Option<ContainerKind> {
    match (first, second) {
        (Some(first), Some(second)) if first != second => None,
        (Some(kind), _) | (_, Some(kind)) => Some(kind),
        (None, None) => None,
    }
}

fn child_segments(kind: ContainerKind, input: DiffInput<'_>) -> Vec<AttributePathSegment> {
    match kind {
        ContainerKind::Object => {
            let mut keys = BTreeSet::new();
            add_object_keys(&mut keys, input.before);
            add_object_keys(&mut keys, input.after);
            add_object_keys(&mut keys, input.before_sensitive);
            add_object_keys(&mut keys, input.after_sensitive);
            add_object_keys(&mut keys, input.after_unknown);
            keys.into_iter().map(AttributePathSegment::Key).collect()
        }
        ContainerKind::Array => {
            let length = [
                input.before,
                input.after,
                input.before_sensitive,
                input.after_sensitive,
                input.after_unknown,
            ]
            .into_iter()
            .filter_map(value_array_length)
            .max()
            .unwrap_or(0);
            (0..length).map(AttributePathSegment::Index).collect()
        }
    }
}

fn add_object_keys(keys: &mut BTreeSet<String>, value: Option<&PlanValue>) {
    if let Some(PlanValue::Object(values)) = value {
        keys.extend(values.keys().cloned());
    }
}

fn child_value<'a>(
    value: Option<&'a PlanValue>,
    segment: &AttributePathSegment,
) -> Option<&'a PlanValue> {
    match (value, segment) {
        (Some(PlanValue::Object(values)), AttributePathSegment::Key(key)) => values.get(key),
        (Some(PlanValue::Array(values)), AttributePathSegment::Index(index)) => values.get(*index),
        _ => None,
    }
}

const fn marker_is_true(value: Option<&PlanValue>) -> bool {
    matches!(value, Some(PlanValue::Bool(true)))
}

fn marker_contains_true(value: Option<&PlanValue>) -> bool {
    match value {
        Some(PlanValue::Bool(value)) => *value,
        Some(PlanValue::Array(values)) => values.iter().any(marker_contains_true_value),
        Some(PlanValue::Object(values)) => values.values().any(marker_contains_true_value),
        _ => false,
    }
}

fn marker_contains_true_value(value: &PlanValue) -> bool {
    marker_contains_true(Some(value))
}

const fn value_container_kind(value: &PlanValue) -> Option<ContainerKind> {
    match value {
        PlanValue::Object(_) => Some(ContainerKind::Object),
        PlanValue::Array(_) => Some(ContainerKind::Array),
        PlanValue::Null | PlanValue::Bool(_) | PlanValue::Number(_) | PlanValue::String(_) => None,
    }
}

fn metadata_container_kind(value: Option<&PlanValue>) -> Option<ContainerKind> {
    value.and_then(value_container_kind)
}

const fn value_array_length(value: Option<&PlanValue>) -> Option<usize> {
    match value {
        Some(PlanValue::Array(values)) => Some(values.len()),
        _ => None,
    }
}

fn display_plan_value(value: &PlanValue) -> String {
    match value {
        PlanValue::Null => "null".to_owned(),
        PlanValue::Bool(value) => value.to_string(),
        PlanValue::Number(value) | PlanValue::String(value) => value.clone(),
        PlanValue::Array(values) => {
            let values = values.iter().map(display_plan_value).collect::<Vec<_>>();
            format!("[{}]", values.join(", "))
        }
        PlanValue::Object(values) => {
            let values = values
                .iter()
                .map(|(key, value)| format!("{key} = {}", display_plan_value(value)))
                .collect::<Vec<_>>();
            format!("{{{}}}", values.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::app::plan::{PlanAction, ResourceChangeKind, ResourceMode};

    fn plan_value(value: Value) -> PlanValue {
        match value {
            Value::Null => PlanValue::Null,
            Value::Bool(value) => PlanValue::Bool(value),
            Value::Number(value) => PlanValue::Number(value.to_string()),
            Value::String(value) => PlanValue::String(value),
            Value::Array(values) => PlanValue::Array(values.into_iter().map(plan_value).collect()),
            Value::Object(values) => PlanValue::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, plan_value(value)))
                    .collect(),
            ),
        }
    }

    fn change(
        before: Value,
        after: Value,
        before_sensitive: Value,
        after_sensitive: Value,
        after_unknown: Value,
    ) -> ResourceChange {
        ResourceChange {
            address: "aws_instance.example".to_owned(),
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: Some(plan_value(before)),
            after: Some(plan_value(after)),
            before_sensitive: Some(plan_value(before_sensitive)),
            after_sensitive: Some(plan_value(after_sensitive)),
            after_unknown: Some(plan_value(after_unknown)),
            replace_paths: Some(vec![vec![ReplacePathSegment::Attribute("name".to_owned())]]),
            action_reason: Some("replace_because_cannot_update".to_owned()),
        }
    }

    fn path(segments: &[AttributePathSegment]) -> Vec<AttributePathSegment> {
        segments.to_vec()
    }

    fn attribute<'a>(
        diffs: &'a AttributeDiffs,
        path: &[AttributePathSegment],
    ) -> &'a AttributeDiff {
        diffs
            .attributes
            .iter()
            .find(|attribute| attribute.path == path)
            .expect("attribute path should exist")
    }

    #[test]
    fn compares_nested_objects_and_arrays_by_key_and_index() {
        let change = change(
            json!({
                "name": "old",
                "tags": {"keep": "same", "remove": "gone"},
                "ports": [80, 443],
                "removed_ports": [8080, 8443]
            }),
            json!({
                "name": "new",
                "tags": {"add": "new", "keep": "same"},
                "ports": [80, 8443, 9443],
                "removed_ports": [8080]
            }),
            json!(false),
            json!(false),
            json!(false),
        );

        let diffs = diff_resource_attributes(&change);

        assert_eq!(
            diffs
                .attributes
                .iter()
                .map(|attribute| (&attribute.path, attribute.kind))
                .collect::<Vec<_>>(),
            vec![
                (
                    &path(&[AttributePathSegment::Key("name".to_owned())]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("ports".to_owned()),
                        AttributePathSegment::Index(0)
                    ]),
                    AttributeChangeKind::Unchanged
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("ports".to_owned()),
                        AttributePathSegment::Index(1)
                    ]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("ports".to_owned()),
                        AttributePathSegment::Index(2)
                    ]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("removed_ports".to_owned()),
                        AttributePathSegment::Index(0)
                    ]),
                    AttributeChangeKind::Unchanged
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("removed_ports".to_owned()),
                        AttributePathSegment::Index(1)
                    ]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("tags".to_owned()),
                        AttributePathSegment::Key("add".to_owned())
                    ]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("tags".to_owned()),
                        AttributePathSegment::Key("keep".to_owned())
                    ]),
                    AttributeChangeKind::Unchanged
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("tags".to_owned()),
                        AttributePathSegment::Key("remove".to_owned())
                    ]),
                    AttributeChangeKind::Changed
                ),
            ]
        );
        assert_eq!(diffs.changed_count, 6);
        assert_eq!(diffs.unchanged_count, 3);
    }

    #[test]
    fn distinguishes_null_from_absent() {
        let change = change(
            json!({"null_value": null}),
            json!({"null_value": null, "new_value": null}),
            json!(false),
            json!(false),
            json!(false),
        );

        let diffs = diff_resource_attributes(&change);
        let null_value = attribute(
            &diffs,
            &[AttributePathSegment::Key("null_value".to_owned())],
        );
        let new_value = attribute(&diffs, &[AttributePathSegment::Key("new_value".to_owned())]);

        assert_eq!(null_value.kind, AttributeChangeKind::Unchanged);
        assert_eq!(null_value.before.kind(), AttributeValueKind::Null);
        assert_eq!(null_value.after.kind(), AttributeValueKind::Null);
        assert_eq!(new_value.before.kind(), AttributeValueKind::Absent);
        assert_eq!(new_value.after.kind(), AttributeValueKind::Null);
        assert_eq!(new_value.before.display(), "<absent>");
        assert_eq!(new_value.after.display(), "null");
    }

    #[test]
    fn applies_sensitive_markers_to_each_side_and_inherits_parent_masks() {
        let change = change(
            json!({"credentials": {"user": "alice", "token": "old"}}),
            json!({"credentials": {"user": "bob", "token": "new"}}),
            json!({"credentials": true}),
            json!({"credentials": {"token": true}}),
            json!(false),
        );

        let diffs = diff_resource_attributes(&change);

        assert_eq!(diffs.attributes.len(), 2);
        let user = attribute(
            &diffs,
            &[
                AttributePathSegment::Key("credentials".to_owned()),
                AttributePathSegment::Key("user".to_owned()),
            ],
        );
        let token = attribute(
            &diffs,
            &[
                AttributePathSegment::Key("credentials".to_owned()),
                AttributePathSegment::Key("token".to_owned()),
            ],
        );

        assert!(user.before.is_sensitive());
        assert!(!user.after.is_sensitive());
        assert_eq!(user.before.display(), "<sensitive>");
        assert_eq!(user.after.display(), "bob");
        assert!(token.before.is_sensitive() && token.after.is_sensitive());
        assert_eq!(token.before.display(), "<sensitive>");
        assert_eq!(token.after.display(), "<sensitive>");
    }

    #[test]
    fn keeps_one_sided_sensitive_values_masked_only_on_that_side() {
        let change = change(
            json!({"public": "old"}),
            json!({"public": "new"}),
            json!(false),
            json!({"public": true}),
            json!(false),
        );

        let attribute = &diff_resource_attributes(&change).attributes[0];

        assert_eq!(attribute.before.display(), "old");
        assert_eq!(attribute.after.display(), "<sensitive>");
        assert_eq!(attribute.after.revealed(), Some(&plan_value(json!("new"))));
    }

    #[test]
    fn represents_unknown_values_and_unknown_missing_attributes() {
        let change = change(
            json!({"known": "old", "null_value": null}),
            json!({"known": "new", "null_value": null}),
            json!(false),
            json!({"known": true}),
            json!({"future": true}),
        );

        let diffs = diff_resource_attributes(&change);
        let known = attribute(&diffs, &[AttributePathSegment::Key("known".to_owned())]);
        let future = attribute(&diffs, &[AttributePathSegment::Key("future".to_owned())]);

        assert_eq!(known.after.kind(), AttributeValueKind::Known);
        assert!(known.after.is_sensitive());
        assert_eq!(known.after.display(), "<sensitive>");
        assert_eq!(future.before.kind(), AttributeValueKind::Absent);
        assert_eq!(future.after.kind(), AttributeValueKind::Unknown);
        assert!(future.after.is_unknown());
        assert_eq!(future.after.display(), "<unknown>");
    }

    #[test]
    fn masks_unknown_values_without_losing_internal_value_or_state() {
        let change = change(
            json!({"token": "old"}),
            json!({"token": "planned"}),
            json!(false),
            json!({"token": true}),
            json!({"token": true}),
        );

        let attribute = &diff_resource_attributes(&change).attributes[0];

        assert_eq!(attribute.after.kind(), AttributeValueKind::Unknown);
        assert!(attribute.after.is_sensitive());
        assert_eq!(attribute.after.display(), "<sensitive>");
        assert_eq!(
            attribute.after.revealed(),
            Some(&plan_value(json!("planned")))
        );
    }

    #[test]
    fn retains_replacement_metadata_and_attribute_counts() {
        let change = change(
            json!({"name": "old", "region": "same"}),
            json!({"name": "new", "region": "same"}),
            json!(false),
            json!(false),
            json!(false),
        );

        let diffs = diff_resource_attributes(&change);

        assert_eq!(diffs.total_count(), 2);
        assert_eq!(diffs.changed_count, 1);
        assert_eq!(diffs.unchanged_count, 1);
        assert_eq!(diffs.replace_paths, change.replace_paths);
        assert_eq!(diffs.action_reason, change.action_reason);
    }

    #[test]
    fn debug_output_does_not_include_original_attribute_values() {
        let change = change(
            json!({"token": "synthetic-secret"}),
            json!({"token": "synthetic-secret-after"}),
            json!(false),
            json!({"token": true}),
            json!(false),
        );

        let debug = format!("{:?}", diff_resource_attributes(&change));

        assert!(!debug.contains("synthetic-secret"));
        assert!(!debug.contains("synthetic-secret-after"));
        assert!(debug.contains("sensitive: true"));
    }

    #[test]
    fn handles_empty_containers_as_single_attributes() {
        let change = change(
            json!({"object": {}, "array": []}),
            json!({"object": {}, "array": []}),
            json!(false),
            json!(false),
            json!(false),
        );

        let diffs = diff_resource_attributes(&change);

        assert_eq!(diffs.attributes.len(), 2);
        assert!(
            diffs
                .attributes
                .iter()
                .all(|attribute| attribute.kind == AttributeChangeKind::Unchanged)
        );
    }
}
