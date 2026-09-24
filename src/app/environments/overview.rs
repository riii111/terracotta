use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
};

use super::{
    EnvironmentPlan,
    comparison::{
        CellState, ComparisonRow, ComparisonScope, DifferenceReason, EnvironmentSelection,
        SourceReference, compare_environments_for_selection,
    },
};
use crate::app::{
    plan::{
        grouping::{GroupingCandidate, GroupingKey, grouping_candidate},
        path::normalize_resource_addresses,
    },
    session::ReviewSessionState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EnvironmentOverview {
    pub(crate) scope: ComparisonScope,
    pub(crate) rows: Vec<OverviewRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverviewRow {
    Individual(ComparisonRow),
    Group(OverviewGroup),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OverviewGroup {
    pub(crate) id: GroupId,
    pub(crate) display_address: String,
    pub(crate) cells: Vec<GroupCell>,
    pub(crate) children: Vec<ComparisonRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverviewRowId {
    Group(GroupId),
    Individual(String),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GroupId(GroupingKey);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroupCell {
    pub(crate) state: CellState,
    pub(crate) members: Vec<String>,
    pub(crate) source: Option<SourceReference>,
}

pub(crate) fn environment_overview(plans: &[EnvironmentPlan]) -> EnvironmentOverview {
    let selection = EnvironmentSelection::new(None, plans.len())
        .expect("all environment indexes form a valid selection");
    environment_overview_for_selection(plans, &selection)
}

pub(crate) fn environment_overview_for_selection(
    plans: &[EnvironmentPlan],
    selection: &EnvironmentSelection,
) -> EnvironmentOverview {
    let comparison = compare_environments_for_selection(plans, selection);
    let candidates: Vec<BTreeMap<_, _>> = selection
        .indexes()
        .iter()
        .map(|index| {
            let plan = &plans[*index];
            let Some(review) = plan.review().map(ReviewSessionState::review) else {
                return BTreeMap::new();
            };
            review
                .plan()
                .resource_changes
                .iter()
                .filter_map(|change| {
                    Some((
                        change.address.as_str(),
                        grouping_candidate(change, review.provider_schemas())?,
                    ))
                })
                .collect()
        })
        .collect();
    let mut rows = Vec::new();
    let mut groups = BTreeMap::<GroupingKey, Vec<ComparisonRow>>::new();
    for row in comparison.rows {
        if let Some(candidate) = shared_candidate(&row, &candidates) {
            groups.entry(candidate.key.clone()).or_default().push(row);
        } else {
            rows.push(OverviewRow::Individual(row));
        }
    }
    for (key, mut children) in groups {
        children.sort_by(|a, b| a.address.cmp(&b.address));
        let cells = group_cells(&children, selection.indexes().len());
        if is_common_group(&cells) {
            let address =
                normalize_resource_addresses(children.iter().map(|row| row.address.as_str()))
                    .expect("grouping candidates share a normalized address");
            rows.push(OverviewRow::Group(OverviewGroup {
                id: GroupId(key),
                display_address: address.display().to_owned(),
                cells,
                children,
            }));
        } else {
            rows.extend(children.into_iter().map(OverviewRow::Individual));
        }
    }
    rows.sort_by(|left, right| row_order(left).cmp(&row_order(right)));
    EnvironmentOverview {
        scope: comparison.scope,
        rows,
    }
}

fn shared_candidate<'a>(
    row: &ComparisonRow,
    candidates: &'a [BTreeMap<&str, GroupingCandidate>],
) -> Option<&'a GroupingCandidate> {
    let mut shared: Option<&GroupingCandidate> = None;
    for (cell, candidates) in row.cells.iter().zip(candidates) {
        match &cell.state {
            CellState::Change { .. } => {
                let candidate = candidates.get(row.address.as_str())?;
                if shared.is_some_and(|shared| shared.key != candidate.key) {
                    return None;
                }
                shared = Some(candidate);
            }
            CellState::NoOp => return None,
            CellState::Missing | CellState::Unavailable => {}
        }
    }
    shared
}

fn group_cells(children: &[ComparisonRow], environment_count: usize) -> Vec<GroupCell> {
    (0..environment_count)
        .map(|environment| {
            let members: Vec<_> = children
                .iter()
                .filter(|row| matches!(row.cells[environment].state, CellState::Change { .. }))
                .collect();
            let cell = &members.first().copied().unwrap_or(&children[0]).cells[environment];
            GroupCell {
                state: cell.state.clone(),
                source: cell.source.clone(),
                members: members.iter().map(|row| row.address.clone()).collect(),
            }
        })
        .collect()
}

fn is_common_group(cells: &[GroupCell]) -> bool {
    cells
        .iter()
        .filter(|cell| cell.state != CellState::Unavailable)
        .all(|cell| !cell.members.is_empty())
        && cells.iter().any(|cell| cell.members.len() >= 2)
}

fn row_order(row: &OverviewRow) -> (bool, Option<DifferenceReason>, &str) {
    match row {
        OverviewRow::Individual(row) => (row.difference.is_none(), row.difference, &row.address),
        OverviewRow::Group(group) => (true, None, &group.display_address),
    }
}

impl Debug for GroupId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupId(<opaque>)")
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::PathBuf};

    use rstest::rstest;

    use super::*;
    use crate::app::{
        environments::{
            Environment, EnvironmentAvailability, EnvironmentIdentity, EnvironmentSession,
            PlanResult,
        },
        execution::Tool,
        plan::{Plan, PlanAction, PlanValue, ResourceChange, ResourceChangeKind, ResourceMode},
        review::{PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata, PlanReview},
    };

    #[test]
    fn unequal_counts_group_patterns_and_restore_exact_address_comparisons() {
        let session = ready_session([20, 20, 200].map(|count| changes(count, "new")));

        let overview = environment_overview(session.plans());

        let group = only_group(&overview);
        assert_eq!(member_counts(group), vec![20, 20, 200]);
        assert_eq!(group.children.len(), 200);
        assert_eq!(group.display_address, "test_resource.item[*]");
        let extra = group
            .children
            .iter()
            .find(|row| row.address == "test_resource.item[199]")
            .unwrap();
        assert_eq!(extra.difference, Some(DifferenceReason::Missing));
        assert_eq!(extra.cells[0].state, CellState::Missing);
        assert_partition(&session, &overview);
    }

    #[test]
    fn selected_environments_group_only_their_common_changes_and_keep_original_sources() {
        let session = ready_session([changes(2, "excluded"), changes(2, "new"), changes(3, "new")]);
        let all = environment_overview(session.plans());
        let selection = EnvironmentSelection::new(Some(vec![2, 1]), session.plans().len()).unwrap();

        let overview = environment_overview_for_selection(session.plans(), &selection);

        assert!(
            all.rows
                .iter()
                .all(|row| matches!(row, OverviewRow::Individual(_)))
        );
        let group = only_group(&overview);
        assert_eq!(member_counts(group), [2, 3]);
        assert_eq!(group.children.len(), 3);
        assert_eq!(
            group
                .cells
                .iter()
                .map(|cell| cell.source.as_ref().unwrap().environment)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        let child = group
            .children
            .iter()
            .find(|row| row.address == "test_resource.item[0]")
            .unwrap();
        assert_eq!(
            child
                .cells
                .iter()
                .map(|cell| cell.source.as_ref().unwrap().environment)
                .collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn common_changes_keep_the_exception_individual() {
        let mut resources = changes(200, "new");
        resources.push(change("test_resource.item[200]", "exception"));
        let session = ready_session([resources.clone(), resources]);

        let overview = environment_overview(session.plans());

        assert_eq!(overview.rows.len(), 2);
        assert!(overview.rows.iter().any(|row| matches!(row,
            OverviewRow::Group(group) if member_counts(group) == [200, 200]
        )));
        assert!(overview.rows.iter().any(|row| matches!(row,
            OverviewRow::Individual(row) if row.address == "test_resource.item[200]"
        )));
        assert_partition(&session, &overview);
    }

    #[test]
    fn conflicting_full_addresses_remain_individual_in_every_environment() {
        let mut action = change("test_resource.item[0]", "new");
        action.actions = vec![PlanAction::Delete, PlanAction::Create];
        action.kind = ResourceChangeKind::Replace;
        let mut no_op = change("test_resource.item[0]", "old");
        no_op.actions = vec![PlanAction::NoOp];
        no_op.kind = ResourceChangeKind::NoOp;
        let mut attrs = change("test_resource.item[0]", "new");
        attrs.after = Some(PlanValue::Object(BTreeMap::from([
            ("name".to_owned(), PlanValue::String("new".to_owned())),
            ("extra".to_owned(), PlanValue::Bool(true)),
        ])));
        for (name, conflict, reason) in [
            (
                "value",
                change("test_resource.item[0]", "other"),
                DifferenceReason::Value,
            ),
            ("action", action, DifferenceReason::Action),
            ("no-op", no_op, DifferenceReason::Action),
            ("attrs", attrs, DifferenceReason::Attrs),
        ] {
            let mut right = changes(3, "new");
            right[0] = conflict;
            let session = ready_session([changes(3, "new"), right]);

            let overview = environment_overview(session.plans());

            assert_eq!(overview.rows.len(), 2, "{name}");
            let OverviewRow::Individual(row) = &overview.rows[0] else {
                panic!("conflict must stay individual: {name}");
            };
            assert_eq!(row.address, "test_resource.item[0]", "{name}");
            assert_eq!(row.difference, Some(reason), "{name}");
            assert!(
                matches!(&overview.rows[1], OverviewRow::Group(group)
                if member_counts(group) == [2, 2]),
                "{name}"
            );
            assert_partition(&session, &overview);
        }
    }

    #[test]
    fn missing_reason_does_not_hide_value_conflicts_between_present_environments() {
        let session = ready_session([
            changes(3, "new"),
            vec![
                change("test_resource.item[0]", "different"),
                change("test_resource.item[1]", "new"),
            ],
            vec![change("test_resource.item[1]", "new")],
        ]);

        let overview = environment_overview(session.plans());

        assert!(matches!(&overview.rows[0], OverviewRow::Individual(row)
            if row.address == "test_resource.item[0]" && row.difference == Some(DifferenceReason::Missing)));
        assert!(matches!(&overview.rows[1], OverviewRow::Group(group)
            if member_counts(group) == [2, 1, 1]));
        assert_partition(&session, &overview);
    }

    #[test]
    fn presence_without_a_change_record_prevents_grouping_that_address() {
        let mut session = pending_session(2);
        complete_next(&mut session, review(changes(3, "new")));
        let mut plan = Plan::empty();
        plan.resource_changes = changes(3, "new").into_iter().skip(1).collect();
        plan.value_addresses
            .insert("test_resource.item[0]".to_owned());
        complete_next(&mut session, review(Vec::new()).with_plan(plan));

        let overview = environment_overview(session.plans());

        assert!(matches!(&overview.rows[0], OverviewRow::Individual(row)
            if row.cells[1].state == CellState::NoOp));
        assert_partition(&session, &overview);
    }

    #[test]
    fn module_keys_are_grouped_without_inventing_individual_matches() {
        let session = ready_session([
            vec![
                change(r#"module.app["dev"].test_resource.item[0]"#, "new"),
                change(r#"module.app["dev"].test_resource.item[1]"#, "new"),
            ],
            vec![change(r#"module.app["prod"].test_resource.item[9]"#, "new")],
        ]);

        let overview = environment_overview(session.plans());

        let group = only_group(&overview);
        assert_eq!(group.display_address, "module.app[*].test_resource.item[*]");
        assert_eq!(member_counts(group), vec![2, 1]);
        assert!(
            group
                .children
                .iter()
                .all(|row| row.difference == Some(DifferenceReason::Missing))
        );
        assert_eq!(
            group.cells[1].members,
            [r#"module.app["prod"].test_resource.item[9]"#]
        );
        assert_eq!(
            group.cells[1].source,
            Some(SourceReference {
                environment: 1,
                line: Some(0)
            })
        );
        assert_partition(&session, &overview);
    }

    #[test]
    fn group_display_retains_key_positions_from_every_member() {
        struct Case {
            name: &'static str,
            left: &'static str,
            right: [&'static str; 2],
            display: &'static str,
        }

        for case in [
            Case {
                name: "unkeyed_and_keyed",
                left: "test_resource.item",
                right: ["test_resource.item[0]", "test_resource.item[1]"],
                display: "test_resource.item[*]",
            },
            Case {
                name: "module_and_resource_keys",
                left: r#"module.app["dev"].test_resource.item"#,
                right: [
                    "module.app.test_resource.item[0]",
                    "module.app.test_resource.item[1]",
                ],
                display: "module.app[*].test_resource.item[*]",
            },
        ] {
            let session = ready_session([
                vec![change(case.left, "new")],
                case.right.map(|address| change(address, "new")).to_vec(),
            ]);

            let overview = environment_overview(session.plans());

            let group = only_group(&overview);
            assert_eq!(group.display_address, case.display, "case: {}", case.name);
            assert_eq!(member_counts(group), [1, 2], "case: {}", case.name);
            assert_partition(&session, &overview);
        }
    }

    #[rstest]
    #[case::one_each([1, 1])]
    #[case::only_one_environment([2, 0])]
    fn insufficient_members_stay_individual(#[case] counts: [usize; 2]) {
        let session = ready_session(counts.map(|count| changes(count, "new")));

        let overview = environment_overview(session.plans());

        assert!(
            overview
                .rows
                .iter()
                .all(|row| matches!(row, OverviewRow::Individual(_)))
        );
        assert_partition(&session, &overview);
    }

    #[test]
    fn multiple_patterns_have_distinct_stable_opaque_ids() {
        let mut resources = changes(2, "synthetic-private-pattern");
        resources.extend([
            change("test_resource.item[2]", "other-pattern"),
            change("test_resource.item[3]", "other-pattern"),
        ]);
        let session = ready_session([resources.clone(), resources.clone()]);
        let overview = environment_overview(session.plans());
        let ids: BTreeSet<_> = overview
            .rows
            .iter()
            .map(|row| match row {
                OverviewRow::Group(group) => group.id.clone(),
                OverviewRow::Individual(_) => panic!("both patterns should repeat"),
            })
            .collect();
        resources.reverse();
        let reordered = ready_session([resources.clone(), resources]);

        assert_eq!(ids.len(), 2);
        assert_eq!(overview, environment_overview(reordered.plans()));
        assert!(!format!("{overview:?}").contains("synthetic-private-pattern"));
        assert!(!format!("{overview:?}").contains("other-pattern"));
        assert_partition(&session, &overview);
    }

    #[rstest]
    #[case::unknown(true)]
    #[case::sensitive(false)]
    fn ungroupable_markers_keep_even_equal_changes_individual(#[case] unknown: bool) {
        let mut marked = changes(2, "synthetic-secret");
        for change in &mut marked {
            if unknown {
                change.after_unknown = Some(PlanValue::Bool(true));
            } else {
                change.after_sensitive = Some(PlanValue::Bool(true));
            }
        }
        let session = ready_session([marked.clone(), marked]);

        let overview = environment_overview(session.plans());

        assert_eq!(overview.rows.len(), 2);
        assert!(
            overview
                .rows
                .iter()
                .all(|row| matches!(row, OverviewRow::Individual(row) if row.difference.is_none()))
        );
        assert!(!format!("{overview:?}").contains("synthetic-secret"));
        assert_partition(&session, &overview);
    }

    #[test]
    fn sensitivity_in_one_environment_excludes_the_address_everywhere() {
        let left = changes(3, "new");
        let mut right = left.clone();
        right[0].after_sensitive = Some(PlanValue::Bool(true));
        let session = ready_session([left, right]);

        let overview = environment_overview(session.plans());

        assert!(overview.rows.iter().any(|row| matches!(row,
            OverviewRow::Individual(row) if row.address == "test_resource.item[0]" && row.difference.is_none()
        )));
        assert!(overview.rows.iter().any(|row| matches!(row,
            OverviewRow::Group(group) if member_counts(group) == [2, 2]
        )));
        assert_partition(&session, &overview);
    }

    #[test]
    fn ready_additions_rebuild_groups_and_keep_unavailable_cells_distinct() {
        let mut session = pending_session(3);
        let waiting = environment_overview(session.plans());
        assert_eq!(waiting.scope, ComparisonScope::Waiting);
        assert!(waiting.rows.is_empty());
        complete_next(&mut session, review(changes(2, "new")));
        let partial = environment_overview(session.plans());
        let group = only_group(&partial);
        assert_eq!(member_counts(group), vec![2, 0, 0]);
        assert_eq!(group.cells[1].state, CellState::Unavailable);
        assert_eq!(group.cells[1].source, None);
        let original_id = group.id.clone();
        assert_eq!(
            partial.scope,
            ComparisonScope::Partial { compared: vec![0] }
        );
        let run = session.start_next().unwrap();
        assert_eq!(partial, environment_overview(session.plans()));
        session.complete(
            run,
            PlanResult::Error("synthetic failure".to_owned()),
            Vec::new(),
        );
        assert_eq!(partial, environment_overview(session.plans()));
        assert!(session.retry(1));
        complete_next(&mut session, review(changes(3, "new")));
        let added = environment_overview(session.plans());
        assert_eq!(only_group(&added).id, original_id);
        assert_eq!(member_counts(only_group(&added)), vec![2, 3, 0]);
        assert_partition(&session, &added);

        complete_next(&mut session, review(changes(1, "different")));
        let complete = environment_overview(session.plans());

        assert_eq!(
            complete.scope,
            ComparisonScope::All {
                compared: vec![0, 1, 2]
            }
        );
        assert!(
            complete
                .rows
                .iter()
                .all(|row| matches!(row, OverviewRow::Individual(_)))
        );
        assert_partition(&session, &complete);
    }

    fn only_group(overview: &EnvironmentOverview) -> &OverviewGroup {
        assert_eq!(overview.rows.len(), 1);
        let OverviewRow::Group(group) = &overview.rows[0] else {
            panic!("expected a common group");
        };
        group
    }

    fn member_counts(group: &OverviewGroup) -> Vec<usize> {
        group.cells.iter().map(|cell| cell.members.len()).collect()
    }

    fn assert_partition(session: &EnvironmentSession, overview: &EnvironmentOverview) {
        let selection = EnvironmentSelection::new(None, session.plans().len()).unwrap();
        let comparison = compare_environments_for_selection(session.plans(), &selection);
        assert_eq!(overview.scope, comparison.scope);
        let mut expanded = Vec::new();
        for row in &overview.rows {
            match row {
                OverviewRow::Individual(row) => expanded.push(row.clone()),
                OverviewRow::Group(group) => {
                    expanded.extend(group.children.clone());
                    for (environment, cell) in group.cells.iter().enumerate() {
                        let changes: Vec<_> = group
                            .children
                            .iter()
                            .filter(|row| {
                                matches!(row.cells[environment].state, CellState::Change { .. })
                            })
                            .collect();
                        assert_eq!(
                            cell.members,
                            changes
                                .iter()
                                .map(|row| row.address.clone())
                                .collect::<Vec<_>>()
                        );
                        assert_eq!(
                            cell.source,
                            changes
                                .first()
                                .and_then(|row| row.cells[environment].source.clone())
                        );
                    }
                }
            }
        }
        assert_eq!(expanded.len(), comparison.rows.len());
        let by_address = |rows: Vec<ComparisonRow>| {
            rows.into_iter()
                .map(|row| (row.address.clone(), row))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(by_address(expanded), by_address(comparison.rows));
    }

    fn pending_session(count: usize) -> EnvironmentSession {
        EnvironmentSession::new(
            (0..count)
                .map(|index| Environment {
                    tool: Tool::Terraform,
                    availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                        directory: PathBuf::from(format!("/synthetic/env{index:02}")),
                        workspace: "default".to_owned(),
                    }),
                })
                .collect(),
            false,
        )
    }

    fn ready_session<const N: usize>(changes: [Vec<ResourceChange>; N]) -> EnvironmentSession {
        let mut session = pending_session(N);
        for changes in changes {
            complete_next(&mut session, review(changes));
        }
        session
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

    fn review(changes: Vec<ResourceChange>) -> PlanReview {
        let mut addresses: Vec<_> = changes
            .iter()
            .map(|change| change.address.clone())
            .collect();
        addresses.sort();
        let document = PlanDocument::with_blocks_and_line_kinds(
            addresses.iter().map(|_| "safe plan\n").collect(),
            addresses
                .into_iter()
                .enumerate()
                .map(|(line, address)| {
                    PlanBlock::with_addresses(
                        line..line + 1,
                        PlanBlockKind::Resource,
                        vec![address],
                    )
                })
                .collect(),
            Vec::new(),
        );
        let mut plan = Plan::empty();
        plan.resource_changes = changes;
        PlanReview::new(
            PathBuf::from("/synthetic"),
            "default".to_owned(),
            document,
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, true),
            Vec::new(),
        )
        .with_plan(plan)
    }

    fn changes(count: usize, after: &str) -> Vec<ResourceChange> {
        (0..count)
            .map(|index| change(&format!("test_resource.item[{index}]"), after))
            .collect()
    }

    fn change(address: &str, after: &str) -> ResourceChange {
        let attributes = |name: &str| {
            PlanValue::Object(BTreeMap::from([(
                "name".to_owned(),
                PlanValue::String(name.to_owned()),
            )]))
        };
        ResourceChange {
            address: address.to_owned(),
            provider: None,
            resource_type: None,
            resource_name: None,
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: Some(attributes("old")),
            after: Some(attributes(after)),
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
            previous_address: None,
            importing: None,
        }
    }
}
