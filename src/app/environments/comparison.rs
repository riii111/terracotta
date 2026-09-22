use std::collections::{BTreeMap, BTreeSet};

use super::EnvironmentPlan;
use crate::app::{
    plan::{
        PlanAction, ResourceChange, ResourceChangeKind,
        comparison::{compare_resource_attributes, resource_has_unknown},
    },
    review::PlanReview,
    session::ReviewSessionState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EnvironmentComparison {
    pub(crate) scope: ComparisonScope,
    pub(crate) rows: Vec<ComparisonRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComparisonScope {
    Waiting,
    Partial { compared: Vec<usize> },
    All { compared: Vec<usize> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComparisonRow {
    pub(crate) address: String,
    pub(crate) cells: Vec<ComparisonCell>,
    pub(crate) difference: Option<DifferenceReason>,
    pub(crate) has_unknown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComparisonCell {
    pub(crate) state: CellState,
    pub(crate) source: Option<SourceReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CellState {
    Change {
        actions: Vec<PlanAction>,
        kind: ResourceChangeKind,
    },
    NoOp,
    Missing,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceReference {
    pub(crate) environment: usize,
    pub(crate) line: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DifferenceReason {
    Action,
    Attrs,
    Missing,
    Unknown,
    Value,
}

pub(crate) fn compare_environments(plans: &[EnvironmentPlan]) -> EnvironmentComparison {
    let reviews: Vec<_> = plans
        .iter()
        .map(|plan| plan.review().map(ReviewSessionState::review))
        .collect();
    let compared: Vec<_> = reviews
        .iter()
        .enumerate()
        .filter_map(|(index, review)| review.map(|_| index))
        .collect();
    let scope = if compared.is_empty() {
        ComparisonScope::Waiting
    } else if compared.len() == plans.len() {
        ComparisonScope::All { compared }
    } else {
        ComparisonScope::Partial { compared }
    };
    let resources: Vec<BTreeMap<_, _>> = reviews
        .iter()
        .map(|review| {
            review
                .iter()
                .flat_map(|review| &review.plan().resource_changes)
                .map(|change| (change.address.as_str(), change))
                .collect()
        })
        .collect();
    let addresses: BTreeSet<_> = resources
        .iter()
        .flat_map(BTreeMap::values)
        .filter(|change| change.kind != ResourceChangeKind::NoOp)
        .map(|change| change.address.as_str())
        .collect();
    let mut rows: Vec<_> = addresses
        .into_iter()
        .map(|address| {
            let cells: Vec<_> = reviews
                .iter()
                .zip(&resources)
                .enumerate()
                .map(|(index, (review, resources))| {
                    comparison_cell(index, address, *review, resources.get(address).copied())
                })
                .collect();
            let difference = difference_reason(address, &reviews, &resources, &cells);
            let has_unknown = resources
                .iter()
                .filter_map(|resources| resources.get(address))
                .any(|change| resource_has_unknown(change));
            ComparisonRow {
                address: address.to_owned(),
                cells,
                difference,
                has_unknown,
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        left.difference
            .is_none()
            .cmp(&right.difference.is_none())
            .then_with(|| left.difference.cmp(&right.difference))
            .then_with(|| left.address.cmp(&right.address))
    });
    EnvironmentComparison { scope, rows }
}

fn comparison_cell(
    index: usize,
    address: &str,
    review: Option<&PlanReview>,
    resource: Option<&ResourceChange>,
) -> ComparisonCell {
    let Some(review) = review else {
        return ComparisonCell {
            state: CellState::Unavailable,
            source: None,
        };
    };
    let state = match resource {
        Some(change) if change.kind != ResourceChangeKind::NoOp => CellState::Change {
            actions: change.actions.clone(),
            kind: change.kind,
        },
        Some(_) => CellState::NoOp,
        None if review.plan().value_addresses.contains(address) => CellState::NoOp,
        None => CellState::Missing,
    };
    let source = (!matches!(state, CellState::Missing)).then(|| SourceReference {
        environment: index,
        line: review
            .document()
            .block_for_address(address)
            .map(|block| block.lines().start),
    });
    ComparisonCell { state, source }
}

fn difference_reason(
    address: &str,
    reviews: &[Option<&PlanReview>],
    resources: &[BTreeMap<&str, &ResourceChange>],
    cells: &[ComparisonCell],
) -> Option<DifferenceReason> {
    let mut present = cells
        .iter()
        .filter(|cell| matches!(cell.state, CellState::Change { .. } | CellState::NoOp));
    if let Some(first) = present.next()
        && present.any(|cell| cell.state != first.state)
    {
        return Some(DifferenceReason::Action);
    }
    let mut changes = reviews
        .iter()
        .zip(resources)
        .filter_map(|(review, resources)| {
            Some((
                *resources.get(address)?,
                review.as_ref()?.provider_schemas(),
            ))
        });
    let mut attrs_differ = false;
    let mut unknown_differ = false;
    let mut values_differ = false;
    if let Some((first, first_schemas)) = changes.next() {
        for (change, schemas) in changes {
            let comparison = compare_resource_attributes(first, first_schemas, change, schemas);
            attrs_differ |= comparison.attrs_differ;
            unknown_differ |= comparison.unknown_differ;
            values_differ |= comparison.values_differ;
        }
    }
    if attrs_differ {
        Some(DifferenceReason::Attrs)
    } else if cells.iter().any(|cell| cell.state == CellState::Missing) {
        Some(DifferenceReason::Missing)
    } else if unknown_differ {
        Some(DifferenceReason::Unknown)
    } else if values_differ {
        Some(DifferenceReason::Value)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{Value, json};

    use super::*;
    use crate::app::{
        copy,
        environments::{
            Environment, EnvironmentAvailability, EnvironmentIdentity, EnvironmentSession,
            PlanResult,
        },
        execution::Tool,
        plan::{
            AttributeType, Plan, PlanValue, ProviderSchema, ProviderSchemas, ResourceMode,
            ResourceSchema,
        },
        review::{PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata},
    };

    fn value(value: Value) -> PlanValue {
        match value {
            Value::Null => PlanValue::Null,
            Value::Bool(value) => PlanValue::Bool(value),
            Value::Number(value) => PlanValue::Number(value.to_string()),
            Value::String(value) => PlanValue::String(value),
            Value::Array(values) => PlanValue::Array(values.into_iter().map(self::value).collect()),
            Value::Object(values) => PlanValue::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, self::value(value)))
                    .collect(),
            ),
        }
    }

    fn update(before: Value, after: Value) -> ResourceChange {
        ResourceChange {
            address: "test_resource.item".to_owned(),
            provider: Some("test".to_owned()),
            resource_type: Some("test_resource".to_owned()),
            resource_name: Some("item".to_owned()),
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: Some(value(before)),
            after: Some(value(after)),
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
            previous_address: None,
            importing: None,
        }
    }

    fn unknown(mut change: ResourceChange, marker: Value) -> ResourceChange {
        change.after_unknown = Some(value(marker));
        change
    }

    fn schemas(kind: AttributeType) -> ProviderSchemas {
        ProviderSchemas {
            providers: BTreeMap::from([(
                "test".to_owned(),
                ProviderSchema {
                    resources: BTreeMap::from([(
                        "test_resource".to_owned(),
                        ResourceSchema {
                            attributes: BTreeMap::from([("items".to_owned(), kind)]),
                            block_types: BTreeMap::new(),
                        },
                    )]),
                },
            )]),
        }
    }

    fn environment(name: &str) -> Environment {
        Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from(name),
                workspace: "default".to_owned(),
            }),
        }
    }

    fn review(changes: Vec<ResourceChange>, schema: Option<ProviderSchemas>) -> PlanReview {
        let addresses = changes
            .iter()
            .map(|change| change.address.clone())
            .collect();
        let mut plan = Plan::empty();
        plan.resource_changes = changes;
        PlanReview::new(
            PathBuf::from("/test"),
            "default".to_owned(),
            PlanDocument::with_blocks_and_line_kinds(
                "safe plan\n(sensitive value)".to_owned(),
                vec![PlanBlock::with_addresses(
                    1..2,
                    PlanBlockKind::Resource,
                    addresses,
                )],
                Vec::new(),
            ),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, true),
            Vec::new(),
        )
        .with_plan(plan)
        .with_provider_schemas(schema)
    }

    fn complete_next(session: &mut EnvironmentSession, review: PlanReview) {
        let run = session.start_next().unwrap();
        assert!(session.complete(
            run,
            PlanResult::Ready {
                review: Box::new(review),
                changed: true
            },
            Vec::new()
        ));
    }

    fn compared(
        changes: Vec<Option<ResourceChange>>,
        schema: Option<&ProviderSchemas>,
    ) -> EnvironmentComparison {
        let mut session = EnvironmentSession::new(
            (0..changes.len())
                .map(|index| environment(&index.to_string()))
                .collect(),
            false,
        );
        for change in changes {
            complete_next(
                &mut session,
                review(change.into_iter().collect(), schema.cloned()),
            );
        }
        compare_environments(session.plans())
    }

    #[test]
    fn distinguishes_actions_attributes_presence_unknowns_and_values() {
        let base = update(
            json!({"name": "old", "id": "previous"}),
            json!({"name": "new", "id": "next"}),
        );
        let mut replacement = base.clone();
        replacement.actions = vec![PlanAction::Delete, PlanAction::Create];
        replacement.kind = ResourceChangeKind::Replace;
        let mut reverse_replacement = replacement.clone();
        reverse_replacement.actions.reverse();
        let mut no_op = base.clone();
        no_op.actions = vec![PlanAction::NoOp];
        no_op.kind = ResourceChangeKind::NoOp;
        no_op.after = no_op.before.clone();
        let cases = [
            ("same change", Some(base.clone()), Some(base.clone()), None),
            (
                "action",
                Some(base.clone()),
                Some(replacement.clone()),
                Some(DifferenceReason::Action),
            ),
            (
                "replacement order",
                Some(replacement),
                Some(reverse_replacement),
                Some(DifferenceReason::Action),
            ),
            (
                "no op is present",
                Some(base.clone()),
                Some(no_op),
                Some(DifferenceReason::Action),
            ),
            (
                "missing",
                Some(base.clone()),
                None,
                Some(DifferenceReason::Missing),
            ),
            (
                "attrs",
                Some(base.clone()),
                Some(update(
                    json!({"name": "old", "id": "previous"}),
                    json!({"name": "new", "id": "previous"}),
                )),
                Some(DifferenceReason::Attrs),
            ),
            (
                "value",
                Some(base),
                Some(update(
                    json!({"name": "old", "id": "previous"}),
                    json!({"name": "different", "id": "next"}),
                )),
                Some(DifferenceReason::Value),
            ),
            (
                "unchanged values do not differ",
                Some(update(
                    json!({"name": "old", "other": "left"}),
                    json!({"name": "new", "other": "left"}),
                )),
                Some(update(
                    json!({"name": "old", "other": "right"}),
                    json!({"name": "new", "other": "right"}),
                )),
                None,
            ),
            (
                "missing differs from null",
                Some(update(json!({}), json!({"name": "new"}))),
                Some(update(json!({"name": null}), json!({"name": "new"}))),
                Some(DifferenceReason::Value),
            ),
        ];

        for (name, left, right, expected) in cases {
            let comparison = compared(vec![left, right], None);

            assert_eq!(comparison.rows.len(), 1, "{name}");
            assert_eq!(comparison.rows[0].difference, expected, "{name}");
            assert_eq!(
                comparison.scope,
                ComparisonScope::All {
                    compared: vec![0, 1]
                },
                "{name}"
            );
        }
    }

    #[test]
    fn distinguishes_unknown_paths_and_sensitive_values_without_losing_known_differences() {
        let base = update(
            json!({"name": "old", "id": "previous"}),
            json!({"name": "new", "id": "next"}),
        );
        let unknown_id = unknown(base.clone(), json!({"id": true}));
        let same_unknown = unknown(
            update(
                json!({"name": "old", "id": "previous"}),
                json!({"name": "new"}),
            ),
            json!({"id": true}),
        );
        let mut sensitive = base.clone();
        sensitive.before_sensitive = Some(value(json!(true)));
        sensitive.after_sensitive = Some(value(json!({"name": true})));
        let mut different_sensitive = sensitive.clone();
        different_sensitive.after = Some(value(json!({"name": "synthetic-secret", "id": "next"})));
        let cases = [
            (
                "same unknown ignores placeholder",
                Some(unknown_id.clone()),
                Some(same_unknown),
                None,
            ),
            (
                "some unknown",
                Some(unknown_id.clone()),
                Some(base.clone()),
                Some(DifferenceReason::Unknown),
            ),
            (
                "unknown paths differ",
                Some(unknown_id.clone()),
                Some(unknown(base.clone(), json!({"name": true}))),
                Some(DifferenceReason::Unknown),
            ),
            (
                "known before differs with shared unknown",
                Some(unknown_id.clone()),
                Some(unknown(
                    update(
                        json!({"name": "old", "id": "different"}),
                        json!({"name": "new"}),
                    ),
                    json!({"id": true}),
                )),
                Some(DifferenceReason::Value),
            ),
            (
                "known after differs with shared unknown",
                Some(unknown_id),
                Some(unknown(
                    update(
                        json!({"name": "old", "id": "previous"}),
                        json!({"name": "different"}),
                    ),
                    json!({"id": true}),
                )),
                Some(DifferenceReason::Value),
            ),
            (
                "sensitive marker is not a difference",
                Some(base),
                Some(sensitive.clone()),
                None,
            ),
            (
                "sensitive value differs",
                Some(sensitive),
                Some(different_sensitive),
                Some(DifferenceReason::Value),
            ),
        ];
        for (name, left, right, expected) in cases {
            let comparison = compared(vec![left, right], None);

            assert_eq!(comparison.rows[0].difference, expected, "{name}");
        }
    }

    #[test]
    fn chooses_reason_by_priority_across_all_ready_environments() {
        let base = update(json!({"a": 0, "b": 0}), json!({"a": 1, "b": 1}));
        let attrs = update(json!({"a": 0, "b": 0}), json!({"a": 2, "b": 0}));
        let mut action = attrs.clone();
        action.actions = vec![PlanAction::Delete];
        action.kind = ResourceChangeKind::Delete;
        let unknown = unknown(
            update(json!({"a": 0, "b": 0}), json!({"a": 99})),
            json!({"b": true}),
        );
        let cases = [
            (
                "action before attrs and missing",
                vec![Some(base.clone()), Some(action), None],
                DifferenceReason::Action,
            ),
            (
                "attrs before missing and value",
                vec![Some(base.clone()), Some(attrs.clone()), None],
                DifferenceReason::Attrs,
            ),
            (
                "attrs before unknown",
                vec![Some(base.clone()), Some(unknown.clone()), Some(attrs)],
                DifferenceReason::Attrs,
            ),
            (
                "missing before unknown and value",
                vec![Some(base.clone()), Some(unknown.clone()), None],
                DifferenceReason::Missing,
            ),
            (
                "unknown before value",
                vec![Some(base), Some(unknown)],
                DifferenceReason::Unknown,
            ),
        ];

        for (name, changes, expected) in cases {
            let comparison = compared(changes, None);

            assert_eq!(comparison.rows[0].difference, Some(expected), "{name}");
        }
    }

    #[test]
    fn compares_sets_without_order_lists_with_order_and_untyped_arrays_conservatively() {
        let before = json!({"items": ["old"]});
        let cases = [
            (
                "set",
                Some(AttributeType::Set(Box::new(AttributeType::String))),
                None,
            ),
            (
                "list",
                Some(AttributeType::List(Box::new(AttributeType::String))),
                Some(DifferenceReason::Value),
            ),
            (
                "unknown collection type",
                None,
                Some(DifferenceReason::Unknown),
            ),
        ];
        for (name, kind, expected) in cases {
            let left = update(before.clone(), json!({"items": ["a", "b"]}));
            let right = update(before.clone(), json!({"items": ["b", "a"]}));

            let comparison = compared(vec![Some(left), Some(right)], kind.map(schemas).as_ref());

            assert_eq!(comparison.rows[0].difference, expected, "{name}");
        }
    }

    #[test]
    fn preserves_nested_values_and_exact_numeric_equality() {
        let cases = [
            (
                "nested numeric spelling",
                json!({"config": {"n": 1}}),
                json!({"config": {"n": 1.0}}),
                None,
            ),
            (
                "large integers",
                json!({"config": {"n": 9_007_199_254_740_992_u64}}),
                json!({"config": {"n": 9_007_199_254_740_993_u64}}),
                Some(DifferenceReason::Value),
            ),
            (
                "empty shape differs",
                json!({"config": {}}),
                json!({"config": []}),
                Some(DifferenceReason::Value),
            ),
        ];
        for (name, left, right, expected) in cases {
            let comparison = compared(
                vec![
                    Some(update(json!({"config": null}), left)),
                    Some(update(json!({"config": null}), right)),
                ],
                None,
            );

            assert_eq!(comparison.rows[0].difference, expected, "{name}");
        }
    }

    #[test]
    fn uses_full_addresses_and_omits_resources_that_are_only_no_op() {
        let mut left = update(json!({"a": 0}), json!({"a": 1}));
        left.address = "module.service[\"dev\"].test_resource.item[0]".to_owned();
        let mut right = left.clone();
        right.address = "module.service[\"prod\"].test_resource.item[0]".to_owned();
        let mut unchanged = left.clone();
        unchanged.address = "test_resource.unchanged".to_owned();
        unchanged.kind = ResourceChangeKind::NoOp;
        unchanged.actions = vec![PlanAction::NoOp];
        let mut session =
            EnvironmentSession::new(vec![environment("dev"), environment("prod")], false);
        complete_next(
            &mut session,
            review(vec![left.clone(), unchanged.clone()], None),
        );
        complete_next(&mut session, review(vec![right.clone(), unchanged], None));

        let comparison = compare_environments(session.plans());

        assert_eq!(
            comparison
                .rows
                .iter()
                .map(|row| row.address.as_str())
                .collect::<Vec<_>>(),
            vec![left.address, right.address]
        );
        assert!(
            comparison
                .rows
                .iter()
                .all(|row| row.difference == Some(DifferenceReason::Missing))
        );
        assert_eq!(
            comparison.rows[0].cells[0].source,
            Some(SourceReference {
                environment: 0,
                line: Some(1)
            })
        );
        assert_eq!(comparison.rows[0].cells[1].state, CellState::Missing);
        assert_eq!(comparison.rows[0].cells[1].source, None);
    }

    #[test]
    fn value_only_presence_is_no_op_with_an_explicit_source_fallback() {
        let change = update(json!({"a": 0}), json!({"a": 1}));
        let mut plan = Plan::empty();
        plan.value_addresses.insert(change.address.clone());
        let mut session =
            EnvironmentSession::new(vec![environment("dev"), environment("prod")], false);
        complete_next(&mut session, review(vec![change], None));
        complete_next(&mut session, review(Vec::new(), None).with_plan(plan));

        let comparison = compare_environments(session.plans());

        assert_eq!(
            comparison.rows[0].difference,
            Some(DifferenceReason::Action)
        );
        assert_eq!(
            comparison.rows[0].cells[1],
            ComparisonCell {
                state: CellState::NoOp,
                source: Some(SourceReference {
                    environment: 1,
                    line: None
                })
            }
        );
    }

    #[test]
    fn acquisition_and_retry_recompute_only_ready_scope_and_unavailable_cells() {
        let change = unknown(update(json!({"id": "old"}), json!({})), json!({"id": true}));
        let mut session = EnvironmentSession::new(
            vec![
                environment("a"),
                environment("b"),
                environment("c"),
                Environment {
                    tool: Tool::Terraform,
                    availability: EnvironmentAvailability::ExcludedHcp {
                        directory: PathBuf::from("d"),
                    },
                },
            ],
            false,
        );
        assert_eq!(
            compare_environments(session.plans()),
            EnvironmentComparison {
                scope: ComparisonScope::Waiting,
                rows: Vec::new()
            }
        );
        complete_next(&mut session, review(vec![change.clone()], None));
        let run = session.start_next().unwrap();
        for phase in [
            "running",
            "error",
            "retry pending",
            "retry running",
            "stale completion",
        ] {
            match phase {
                "error" => {
                    session.complete(run, PlanResult::Error("failed".to_owned()), Vec::new());
                }
                "retry pending" => {
                    assert!(session.retry(1));
                }
                "retry running" => {
                    session.start_next().unwrap();
                }
                "stale completion" => {
                    assert!(!session.complete(
                        run,
                        PlanResult::Ready {
                            review: Box::new(review(vec![change.clone()], None)),
                            changed: true
                        },
                        Vec::new()
                    ));
                }
                _ => {}
            }
            let comparison = compare_environments(session.plans());
            assert_eq!(
                comparison.scope,
                ComparisonScope::Partial { compared: vec![0] },
                "{phase}"
            );
            assert_eq!(comparison.rows[0].difference, None, "{phase}");
            assert!(comparison.rows[0].has_unknown, "{phase}");
            assert!(
                comparison.rows[0].cells[1..]
                    .iter()
                    .all(|cell| cell.state == CellState::Unavailable && cell.source.is_none()),
                "{phase}"
            );
        }
    }

    #[test]
    fn ready_additions_reclassify_rows_and_finish_the_compared_scope() {
        let left = update(json!({"a": 0}), json!({"a": 1}));
        let right = update(json!({"a": 0}), json!({"a": 2}));
        let mut session = EnvironmentSession::new(vec![environment("a"), environment("b")], false);
        complete_next(&mut session, review(vec![left], None));
        assert_eq!(
            compare_environments(session.plans()).scope,
            ComparisonScope::Partial { compared: vec![0] }
        );

        complete_next(&mut session, review(vec![right], None));
        let comparison = compare_environments(session.plans());

        assert_eq!(
            comparison.scope,
            ComparisonScope::All {
                compared: vec![0, 1]
            }
        );
        assert_eq!(comparison.rows[0].difference, Some(DifferenceReason::Value));
    }

    #[test]
    fn sensitive_values_never_enter_comparison_debug_or_plan_copy() {
        let mut change = update(
            json!({"token": "synthetic-secret-before"}),
            json!({"token": "synthetic-secret-after"}),
        );
        change.before_sensitive = Some(value(json!({"token": true})));
        change.after_sensitive = Some(value(json!({"token": true})));
        let mut session = EnvironmentSession::new(vec![environment("a"), environment("b")], false);
        complete_next(&mut session, review(vec![change.clone()], None));
        complete_next(&mut session, review(vec![change], None));

        let comparison = compare_environments(session.plans());
        let debug = format!("{comparison:?} {:?}", session.plans());
        let copied = copy::plan_effect(session.plans()[0].review().unwrap().review());

        assert_eq!(comparison.rows[0].difference, None);
        assert!(!debug.contains("synthetic-secret"));
        assert!(!copied.text().contains("synthetic-secret"));
        assert_eq!(copied.text(), "safe plan\n(sensitive value)");
    }
    #[test]
    fn orders_difference_reasons_then_full_addresses_before_same_changes() {
        let base = update(json!({"a": 0, "b": 0}), json!({"a": 1, "b": 1}));
        let mut left = Vec::new();
        let mut right = Vec::new();
        for name in [
            "same", "value_z", "missing", "attrs", "unknown", "action", "value_a",
        ] {
            let mut change = base.clone();
            change.address = format!("test_resource.{name}");
            left.push(change.clone());
            match name {
                "missing" => continue,
                "attrs" => change.after = Some(value(json!({"a": 1, "b": 0}))),
                "action" => {
                    change.kind = ResourceChangeKind::Create;
                    change.actions = vec![PlanAction::Create];
                }
                "unknown" => change.after_unknown = Some(value(json!({"a": true}))),
                "value_a" | "value_z" => change.after = Some(value(json!({"a": 2, "b": 2}))),
                _ => {}
            }
            right.push(change);
        }
        let mut session = EnvironmentSession::new(vec![environment("a"), environment("b")], false);
        complete_next(&mut session, review(left, None));
        complete_next(&mut session, review(right, None));

        let comparison = compare_environments(session.plans());

        assert_eq!(
            comparison
                .rows
                .iter()
                .map(|row| row.address.as_str())
                .collect::<Vec<_>>(),
            [
                "test_resource.action",
                "test_resource.attrs",
                "test_resource.missing",
                "test_resource.unknown",
                "test_resource.value_a",
                "test_resource.value_z",
                "test_resource.same",
            ]
        );
    }

    #[test]
    fn nested_sets_preserve_known_differences_alongside_shared_unknowns() {
        let kind = AttributeType::Set(Box::new(AttributeType::Object(BTreeMap::from([
            ("name".to_owned(), AttributeType::String),
            ("id".to_owned(), AttributeType::String),
        ]))));
        let before = json!({"items": [{"name": "old", "id": "before"}]});
        let cases = [
            (
                "reordered",
                json!({"items": [{"name": "a"}, {"name": "b"}]}),
                json!({"items": [{"name": "b"}, {"name": "a"}]}),
                None,
            ),
            (
                "known difference",
                json!({"items": [{"name": "a"}, {"name": "b"}]}),
                json!({"items": [{"name": "a"}, {"name": "c"}]}),
                Some(DifferenceReason::Value),
            ),
        ];
        for (name, left, right, expected) in cases {
            let markers = json!({"items": [{"id": true}, {"id": true}]});
            let left = unknown(update(before.clone(), left), markers.clone());
            let right = unknown(update(before.clone(), right), markers);

            let comparison = compared(vec![Some(left), Some(right)], Some(&schemas(kind.clone())));

            assert_eq!(comparison.rows[0].difference, expected, "{name}");
            assert!(comparison.rows[0].has_unknown, "{name}");
        }
    }
}
