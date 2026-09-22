use std::collections::{BTreeMap, BTreeSet};

use super::{
    AttributeType, PlanAction, ProviderSchemas, ResourceChange, ResourceSchema,
    attribute_diff::{
        AttributeChangeKind, AttributeDiff, AttributePathSegment, GroupingValue,
        diff_resource_attributes,
    },
    path::normalize_resource_address,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangeGroup {
    pub(crate) display_address: String,
    pub(crate) members: Vec<ResourceChange>,
}

impl ChangeGroup {
    #[must_use]
    pub(crate) const fn is_repeated(&self) -> bool {
        self.members.len() >= 2
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanGrouping {
    pub(crate) groups: Vec<ChangeGroup>,
    pub(crate) repeated: usize,
}

pub(crate) fn group_resource_changes(
    changes: &[ResourceChange],
    schemas: Option<&ProviderSchemas>,
) -> PlanGrouping {
    let mut groups = Vec::new();
    let mut candidates = BTreeMap::<GroupingKey, Vec<Candidate<'_>>>::new();

    for (position, change) in changes.iter().enumerate() {
        let Some(candidate) = grouping_candidate(change, schemas) else {
            groups.push(LocatedGroup {
                position,
                group: single_group(change),
            });
            continue;
        };
        candidates
            .entry(candidate.key)
            .or_default()
            .push(Candidate {
                position,
                display_address: candidate.display_address,
                change,
            });
    }

    for bucket in candidates.into_values() {
        if bucket.len() < 2 || has_duplicate_addresses(&bucket) {
            groups.extend(bucket.into_iter().map(|candidate| LocatedGroup {
                position: candidate.position,
                group: single_group(candidate.change),
            }));
            continue;
        }

        let position = bucket
            .iter()
            .map(|candidate| candidate.position)
            .min()
            .expect("a non-empty grouping bucket should have a first position");
        let display_address = bucket[0].display_address.clone();
        let mut members = bucket
            .into_iter()
            .map(|candidate| candidate.change.clone())
            .collect::<Vec<_>>();
        members.sort_by(|left, right| left.address.cmp(&right.address));
        groups.push(LocatedGroup {
            position,
            group: ChangeGroup {
                display_address,
                members,
            },
        });
    }

    groups.sort_by_key(|located| located.position);
    let repeated = groups
        .iter()
        .filter(|located| located.group.is_repeated())
        .map(|located| located.group.members.len())
        .sum();

    PlanGrouping {
        groups: groups.into_iter().map(|located| located.group).collect(),
        repeated,
    }
}

struct LocatedGroup {
    position: usize,
    group: ChangeGroup,
}

fn single_group(change: &ResourceChange) -> ChangeGroup {
    ChangeGroup {
        display_address: change.address.clone(),
        members: vec![change.clone()],
    }
}

struct Candidate<'a> {
    position: usize,
    display_address: String,
    change: &'a ResourceChange,
}

struct GroupingCandidate {
    display_address: String,
    key: GroupingKey,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GroupingKey {
    normalized_address: String,
    actions: Vec<PlanAction>,
    attributes: Vec<AttributeSignature>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AttributeSignature {
    path: Vec<AttributePathSegment>,
    before: GroupingValue,
    after: GroupingValue,
}

fn grouping_candidate(
    change: &ResourceChange,
    schemas: Option<&ProviderSchemas>,
) -> Option<GroupingCandidate> {
    if !change.kind.is_standard_change() {
        return None;
    }

    let address = normalize_resource_address(&change.address)?;
    let diffs = diff_resource_attributes(change);
    let mut attributes = Vec::with_capacity(diffs.changed_count);

    for attribute in diffs
        .attributes
        .iter()
        .filter(|attribute| attribute.kind == AttributeChangeKind::Changed)
    {
        if !is_comparable_attribute(attribute, change, schemas) {
            return None;
        }
        attributes.push(AttributeSignature {
            path: attribute.path.clone(),
            before: attribute.before.grouping_value()?,
            after: attribute.after.grouping_value()?,
        });
    }

    if attributes.is_empty() {
        return None;
    }
    attributes.sort();

    Some(GroupingCandidate {
        display_address: address.display().to_owned(),
        key: GroupingKey {
            normalized_address: address.normalized().to_owned(),
            actions: change.actions.clone(),
            attributes,
        },
    })
}

fn is_comparable_attribute(
    attribute: &AttributeDiff,
    change: &ResourceChange,
    schemas: Option<&ProviderSchemas>,
) -> bool {
    if attribute.before.is_sensitive()
        || attribute.after.is_sensitive()
        || attribute.before.is_unknown()
        || attribute.after.is_unknown()
    {
        return false;
    }

    if attribute.before.grouping_value().is_none() || attribute.after.grouping_value().is_none() {
        return false;
    }

    match attribute.path.as_slice() {
        [AttributePathSegment::Key(_)] => true,
        [
            AttributePathSegment::Key(attribute_name),
            AttributePathSegment::Key(_),
        ] => is_simple_map_attribute(change, attribute_name, schemas),
        _ => false,
    }
}

fn is_simple_map_attribute(
    change: &ResourceChange,
    attribute_name: &str,
    schemas: Option<&ProviderSchemas>,
) -> bool {
    resource_schema(change, schemas)
        .and_then(|schema| schema.attributes.get(attribute_name))
        .is_some_and(AttributeType::is_simple_map)
}

fn resource_schema<'a>(
    change: &ResourceChange,
    schemas: Option<&'a ProviderSchemas>,
) -> Option<&'a ResourceSchema> {
    let provider = change.provider.as_ref()?;
    let resource_type = change.resource_type.as_ref()?;
    schemas?
        .providers
        .get(provider)?
        .resources
        .get(resource_type)
}

fn has_duplicate_addresses(bucket: &[Candidate<'_>]) -> bool {
    let mut addresses = BTreeSet::new();
    bucket
        .iter()
        .any(|candidate| !addresses.insert(candidate.change.address.as_str()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::app::plan::{PlanValue, ProviderSchema, ResourceChangeKind, ResourceMode};

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

    fn change(address: &str, before: Value, after: Value) -> ResourceChange {
        ResourceChange {
            address: address.to_owned(),
            provider: None,
            resource_type: None,
            resource_name: None,
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: Some(plan_value(before)),
            after: Some(plan_value(after)),
            before_sensitive: Some(PlanValue::Bool(false)),
            after_sensitive: Some(PlanValue::Bool(false)),
            after_unknown: Some(PlanValue::Bool(false)),
            replace_paths: None,
            action_reason: None,
            previous_address: None,
            importing: None,
        }
    }

    fn schema_for(
        change: &mut ResourceChange,
        attributes: BTreeMap<String, AttributeType>,
    ) -> ProviderSchemas {
        change.provider = Some("registry.example/provider".to_owned());
        change.resource_type = Some("example_resource".to_owned());
        ProviderSchemas {
            providers: BTreeMap::from([(
                "registry.example/provider".to_owned(),
                ProviderSchema {
                    resources: BTreeMap::from([(
                        "example_resource".to_owned(),
                        ResourceSchema {
                            attributes,
                            block_types: BTreeMap::new(),
                        },
                    )]),
                },
            )]),
        }
    }

    fn simple_map_schema() -> BTreeMap<String, AttributeType> {
        BTreeMap::from([(
            "labels".to_owned(),
            AttributeType::Map(Box::new(AttributeType::String)),
        )])
    }

    #[test]
    fn groups_count_and_for_each_keys_at_every_module_and_resource_level() {
        let changes = vec![
            change(
                r"module.network[0].aws_instance.web[0]",
                json!({"name": "old"}),
                json!({"name": "new"}),
            ),
            change(
                r#"module.network["blue.green"].aws_instance.web["blue"]"#,
                json!({"name": "old"}),
                json!({"name": "new"}),
            ),
        ];

        let grouping = group_resource_changes(&changes, None);

        assert_eq!(grouping.repeated, 2);
        assert_eq!(grouping.groups.len(), 1);
        assert_eq!(
            grouping.groups[0].display_address,
            "module.network[*].aws_instance.web[*]"
        );
        assert_eq!(
            grouping.groups[0]
                .members
                .iter()
                .map(|change| change.address.as_str())
                .collect::<Vec<_>>(),
            vec![
                r#"module.network["blue.green"].aws_instance.web["blue"]"#,
                r"module.network[0].aws_instance.web[0]",
            ]
        );
    }

    #[test]
    fn separates_key_changes_and_keeps_different_modules_or_resource_names_apart() {
        let mut replacement = change(
            "aws_instance.web[0]",
            json!({"name": "old"}),
            json!({"name": "new"}),
        );
        replacement.actions = vec![PlanAction::Delete, PlanAction::Create];
        replacement.kind = ResourceChangeKind::Replace;

        let changes = vec![
            change(
                "aws_instance.web[0]",
                json!({"name": "old"}),
                json!({"name": "new"}),
            ),
            replacement,
            change(
                "module.other.aws_instance.web[0]",
                json!({"name": "old"}),
                json!({"name": "new"}),
            ),
            change(
                "aws_instance.worker[0]",
                json!({"name": "old"}),
                json!({"name": "new"}),
            ),
        ];

        let grouping = group_resource_changes(&changes, None);

        assert_eq!(grouping.repeated, 0);
        assert_eq!(grouping.groups.len(), changes.len());
        assert!(grouping.groups.iter().all(|group| !group.is_repeated()));
    }

    #[test]
    fn ignores_unchanged_attributes_when_the_other_resource_does_not_have_them() {
        let changes = vec![
            change(
                "aws_instance.web[0]",
                json!({"name": "old", "unchanged": "same"}),
                json!({"name": "new", "unchanged": "same"}),
            ),
            change(
                "aws_instance.web[1]",
                json!({"name": "old"}),
                json!({"name": "new"}),
            ),
        ];

        let grouping = group_resource_changes(&changes, None);

        assert_eq!(grouping.repeated, 2);
        assert_eq!(grouping.groups[0].members.len(), 2);
    }

    #[test]
    fn groups_simple_map_elements_only_when_schema_proves_the_map_type() {
        let mut first = change(
            "aws_instance.web[0]",
            json!({"labels": {"environment": "old"}}),
            json!({"labels": {"environment": "new"}}),
        );
        let schemas = schema_for(&mut first, simple_map_schema());
        let mut second = change(
            "aws_instance.web[1]",
            json!({"labels": {"environment": "old"}}),
            json!({"labels": {"environment": "new"}}),
        );
        second.provider = first.provider.clone();
        second.resource_type = first.resource_type.clone();

        let grouping = group_resource_changes(&[first, second], Some(&schemas));

        assert_eq!(grouping.repeated, 2);
        assert_eq!(grouping.groups.len(), 1);
        assert_eq!(grouping.groups[0].display_address, "aws_instance.web[*]");
    }

    #[test]
    fn keeps_map_and_composite_changes_individual_when_schema_is_missing_or_shape_is_unsupported() {
        let mut map_change = change(
            "aws_instance.map[0]",
            json!({"labels": {"environment": "old"}}),
            json!({"labels": {"environment": "new"}}),
        );
        let schemas = schema_for(&mut map_change, simple_map_schema());
        let mut map_without_schema = map_change.clone();
        map_without_schema.address = "aws_instance.map[1]".to_owned();
        map_without_schema.provider = None;
        map_without_schema.resource_type = None;

        let mut list_change = change(
            "aws_instance.list[0]",
            json!({"ports": [80]}),
            json!({"ports": [443]}),
        );
        list_change.provider = map_change.provider.clone();
        list_change.resource_type = map_change.resource_type.clone();
        let mut object_change = change(
            "aws_instance.object[0]",
            json!({"settings": {"enabled": false}}),
            json!({"settings": {"enabled": true}}),
        );
        object_change.provider = map_change.provider.clone();
        object_change.resource_type = map_change.resource_type.clone();

        let grouping = group_resource_changes(
            &[map_change, map_without_schema, list_change, object_change],
            Some(&schemas),
        );

        assert_eq!(grouping.repeated, 0);
        assert_eq!(grouping.groups.len(), 4);
    }

    #[test]
    fn excludes_changed_unknown_or_sensitive_values_but_allows_unchanged_sensitive_values() {
        let mut unchanged_sensitive = change(
            "aws_instance.web[0]",
            json!({"name": "old", "token": "secret"}),
            json!({"name": "new", "token": "secret"}),
        );
        unchanged_sensitive.after_sensitive = Some(plan_value(json!({"token": true})));

        let mut changed_sensitive = change(
            "aws_instance.web[1]",
            json!({"name": "old", "token": "old-secret"}),
            json!({"name": "new", "token": "new-secret"}),
        );
        changed_sensitive.after_sensitive = Some(plan_value(json!({"token": true})));

        let mut changed_unknown = change(
            "aws_instance.web[2]",
            json!({"name": "old"}),
            json!({"name": "new"}),
        );
        changed_unknown.after_unknown = Some(plan_value(json!({"name": true})));

        let grouping = group_resource_changes(
            &[unchanged_sensitive, changed_sensitive, changed_unknown],
            None,
        );

        assert_eq!(grouping.repeated, 0);
        assert_eq!(grouping.groups.len(), 3);
        assert!(grouping.groups[0].members[0].address.ends_with("[0]"));
    }

    #[test]
    fn compares_numbers_without_float_rounding_or_json_serialization() {
        let changes = vec![
            change(
                "aws_instance.web[0]",
                json!({"size": 1}),
                json!({"size": 2}),
            ),
            change(
                "aws_instance.web[1]",
                json!({"size": 1.0}),
                json!({"size": 2.0}),
            ),
            change(
                "aws_instance.web[2]",
                serde_json::from_str(r#"{"size":9007199254740992}"#).unwrap(),
                serde_json::from_str(r#"{"size":9007199254740993}"#).unwrap(),
            ),
        ];

        let grouping = group_resource_changes(&changes, None);

        assert_eq!(grouping.repeated, 2);
        assert_eq!(grouping.groups.len(), 2);
        assert_eq!(grouping.groups[0].members.len(), 2);
        assert_eq!(grouping.groups[1].members.len(), 1);
    }

    #[test]
    fn reports_repeated_members_from_the_unfiltered_plan_and_keeps_every_member_once() {
        let mut changes = (0..200)
            .map(|index| {
                change(
                    &format!("aws_instance.web[{index}]"),
                    json!({"name": "old"}),
                    json!({"name": "new"}),
                )
            })
            .collect::<Vec<_>>();
        changes.push(change(
            "aws_instance.web[200]",
            json!({"name": "old"}),
            json!({"name": "different"}),
        ));

        let grouping = group_resource_changes(&changes, None);

        assert_eq!(grouping.repeated, 200);
        assert_eq!(grouping.groups.len(), 2);
        assert_eq!(grouping.groups[0].members.len(), 200);
        assert_eq!(grouping.groups[1].members.len(), 1);
        let addresses = grouping
            .groups
            .iter()
            .flat_map(|group| group.members.iter().map(|change| change.address.as_str()))
            .collect::<BTreeSet<_>>();
        assert_eq!(addresses.len(), changes.len());
    }

    #[test]
    fn does_not_put_duplicate_full_addresses_in_a_repeated_group() {
        let changes = vec![
            change(
                "aws_instance.web[0]",
                json!({"name": "old"}),
                json!({"name": "new"}),
            ),
            change(
                "aws_instance.web[0]",
                json!({"name": "old"}),
                json!({"name": "new"}),
            ),
        ];

        let grouping = group_resource_changes(&changes, None);

        assert_eq!(grouping.repeated, 0);
        assert_eq!(grouping.groups.len(), 2);
        assert!(grouping.groups.iter().all(|group| group.members.len() == 1));
    }
}
