use std::path::PathBuf;

use ratatui::buffer::Buffer;
use serde_json::{Value, json};

use crate::app::attribution::{ResourceAddress, ResourceSourceLocation, SourceRange, SourceSide};
use crate::app::attribution::{SourceLineChange, attribute_changes};
use crate::app::plan::{
    Plan, PlanAction, PlanSummary, PlanValue, ReplacePathSegment, ResourceChange,
    ResourceChangeKind, ResourceMode, format_attribute_path,
};
use crate::app::review::{
    PlanReview, ReviewComparison, ReviewComparisonBasis, ReviewComparisonStatus,
};
use crate::ui::test_support::render_to_buffer;

use super::*;

pub(super) fn plan_value(value: Value) -> PlanValue {
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

pub(super) fn change() -> ResourceChange {
    ResourceChange {
        address: "aws_instance.api".to_owned(),
        mode: ResourceMode::Managed,
        actions: vec![PlanAction::Update],
        kind: ResourceChangeKind::Update,
        before: Some(plan_value(json!({
            "instance_type": "t3.small",
            "private_ip": null,
            "password": "old-secret",
            "tags": {"environment": "old"},
            "long_path": "a-value-that-is-long-enough-to-wrap-across-the-detail-width"
        }))),
        after: Some(plan_value(json!({
            "instance_type": "t3.medium",
            "private_ip": null,
            "password": "new-secret",
            "tags": {"environment": "new"},
            "long_path": "another-value-that-is-long-enough-to-wrap-across-the-detail-width"
        }))),
        before_sensitive: Some(plan_value(json!({"password": true}))),
        after_sensitive: Some(plan_value(json!({"password": true}))),
        after_unknown: Some(plan_value(json!({"private_ip": true}))),
        replace_paths: Some(vec![vec![ReplacePathSegment::Attribute(
            "instance_type".to_owned(),
        )]]),
        action_reason: Some("replace_because_cannot_update".to_owned()),
    }
}

pub(super) fn state() -> ResourceDetailState {
    state_with_changed_lines(&[])
}

pub(super) fn state_with_changed_lines(changed_lines: &[SourceLineChange]) -> ResourceDetailState {
    state_for_change(change(), changed_lines)
}

pub(super) fn state_for_change(
    change: ResourceChange,
    changed_lines: &[SourceLineChange],
) -> ResourceDetailState {
    let source_files = vec![SourceFileAnalysis::new(
        PathBuf::from("main.tf"),
        SourceSide::After,
        vec![ResourceSourceLocation::new(
            ResourceAddress::new("aws_instance", "api"),
            PathBuf::from("main.tf"),
            SourceSide::After,
            SourceRange::new(42, 46),
        )],
        Vec::new(),
    )];
    let attribution =
        attribute_changes(std::slice::from_ref(&change), &source_files, changed_lines)
            .pop()
            .expect("one change should produce one attribution");
    let review = PlanReview::new(
        PathBuf::from("/infra/prod"),
        "default".to_owned(),
        Plan {
            changes: vec![change],
            summary: PlanSummary {
                updates: 1,
                ..PlanSummary::default()
            },
            unsupported_changes: Vec::new(),
        },
        source_files,
        vec![attribution],
        ReviewComparison::new(
            ReviewComparisonBasis::WorkingTreeVsHead,
            None,
            None,
            None,
            None,
            ReviewComparisonStatus::Complete,
        ),
        Vec::new(),
    )
    .with_git("feature/resize".to_owned());
    let list = PlanListState::from_review(&review).expect("review should build a list");
    ResourceDetailState::from_list(&list).expect("selected item should open")
}

pub(super) fn filtered_review_list() -> PlanListState {
    let mut worker = change();
    worker.address = "aws_instance.worker".to_owned();
    let changes = vec![change(), worker];
    let attributions = attribute_changes(&changes, &[], &[]);
    let review = PlanReview::new(
        PathBuf::from("/infra/prod"),
        "default".to_owned(),
        Plan {
            changes,
            summary: PlanSummary {
                updates: 2,
                ..PlanSummary::default()
            },
            unsupported_changes: Vec::new(),
        },
        Vec::new(),
        attributions,
        ReviewComparison::new(
            ReviewComparisonBasis::WorkingTreeVsHead,
            None,
            None,
            None,
            None,
            ReviewComparisonStatus::Complete,
        ),
        Vec::new(),
    )
    .with_git("feature/resize".to_owned());
    PlanListState::from_review(&review).expect("review should build a list")
}

pub(super) fn navigation_list() -> PlanListState {
    let mut worker = change();
    worker.address = "aws_instance.worker".to_owned();
    let mut bucket = change();
    bucket.address = "aws_s3_bucket.logs".to_owned();
    let changes = vec![change(), worker, bucket];
    let attributions = attribute_changes(&changes, &[], &[]);
    PlanListState::from_plan(
        Plan {
            changes,
            summary: PlanSummary {
                updates: 3,
                ..PlanSummary::default()
            },
            unsupported_changes: Vec::new(),
        },
        attributions,
        "working tree vs HEAD",
    )
    .expect("navigation fixture should build a list")
}

pub(super) fn expansion_change() -> ResourceChange {
    ResourceChange {
        address: "aws_instance.api".to_owned(),
        mode: ResourceMode::Managed,
        actions: vec![PlanAction::Update],
        kind: ResourceChangeKind::Update,
        before: Some(plan_value(json!({
            "group_a": {
                "changed": "old",
                "unchanged": "same",
                "nested": {"value": "old"}
            },
            "group_b": {"secret": "old-secret"},
            "root_changed": "old",
            "root_unchanged": "same"
        }))),
        after: Some(plan_value(json!({
            "group_a": {
                "changed": "new",
                "unchanged": "same",
                "nested": {"value": "new"}
            },
            "group_b": {"secret": "new-secret"},
            "root_changed": "new",
            "root_unchanged": "same"
        }))),
        before_sensitive: Some(plan_value(json!({"group_b": {"secret": true}}))),
        after_sensitive: Some(plan_value(json!({"group_b": {"secret": true}}))),
        after_unknown: Some(plan_value(json!({}))),
        replace_paths: None,
        action_reason: None,
    }
}

pub(super) fn select_group(state: &mut ResourceDetailState, target: &AttributeGroup) {
    for _ in 0..detail_rows(state).len() {
        if detail_rows(state)
            .get(state.selected)
            .and_then(DetailRow::group)
            == Some(target)
        {
            return;
        }
        state.apply_at(DetailAction::SelectNext, 96, 40, Instant::now());
    }
    panic!("group should be selectable");
}

pub(super) fn select_attribute(state: &mut ResourceDetailState, target: &str) {
    for _ in 0..detail_rows(state).len() {
        if let Some(DetailRow::Attribute(index)) = detail_rows(state).get(state.selected)
            && format_attribute_path(&state.attributes.attributes[*index].path) == target
        {
            return;
        }
        state.apply_at(DetailAction::SelectNext, 96, 40, Instant::now());
    }
    panic!("attribute {target} should be selectable");
}

pub(super) fn sensitive_sibling_state() -> ResourceDetailState {
    let mut change = change();
    change.before = Some(plan_value(json!({
        "password": "old-secret",
        "api_token": "old-token",
        "public": "old-public"
    })));
    change.after = Some(plan_value(json!({
        "password": "new-secret",
        "api_token": "new-token",
        "public": "new-public"
    })));
    change.before_sensitive = Some(plan_value(json!({
        "password": true,
        "api_token": true
    })));
    change.after_sensitive = Some(plan_value(json!({
        "password": true,
        "api_token": true
    })));
    state_for_change(change, &[])
}

pub(super) fn unknown_sensitive_state() -> ResourceDetailState {
    let mut change = change();
    change.kind = ResourceChangeKind::Create;
    change.actions = vec![PlanAction::Create];
    change.before = Some(PlanValue::Null);
    change.after = Some(plan_value(json!({"future_secret": "not-known-yet"})));
    change.before_sensitive = Some(plan_value(json!(false)));
    change.after_sensitive = Some(plan_value(json!({"future_secret": true})));
    change.after_unknown = Some(plan_value(json!({"future_secret": true})));
    state_for_change(change, &[])
}

pub(super) fn render(state: &ResourceDetailState, width: u16, height: u16) -> Buffer {
    let mut state = state.clone();
    render_to_buffer((width, height), |frame| {
        render_resource_detail(frame, &mut state);
    })
}

pub(super) fn render_at(
    state: &mut ResourceDetailState,
    width: u16,
    height: u16,
    now: Instant,
) -> Buffer {
    render_to_buffer((width, height), |frame| {
        super::render::render_resource_detail_at(frame, state, now);
    })
}
