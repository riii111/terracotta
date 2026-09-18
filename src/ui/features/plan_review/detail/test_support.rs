use std::path::PathBuf;

use ratatui::buffer::Buffer;
use serde_json::{Value, json};

use crate::app::attribution::{
    ResourceAddress, ResourceSourceLocation, SourceFileAnalysis, SourceIssue, SourceIssueKind,
    SourceLineChange, SourceRange, SourceSide, attribute_changes,
};
use crate::app::plan::{
    Plan, PlanAction, PlanSummary, PlanValue, ReplacePathSegment, ResourceChange,
    ResourceChangeKind, ResourceMode, format_attribute_path,
};
use crate::app::review::{
    AttributeGroup, DetailAction, DetailRow, PlanReview, ReviewComparison, ReviewComparisonBasis,
    ReviewComparisonStatus,
};
use crate::ui::test_support::render_to_buffer;

use super::*;

pub(super) struct DetailFixture {
    pub(super) list: PlanListState,
    pub(super) detail: ReviewDetailState,
    pub(super) view: DetailViewState,
    pub(super) copy_notice: Option<CopyNotice>,
}

impl DetailFixture {
    pub(super) fn apply_action(
        &mut self,
        action: DetailAction,
        width: u16,
        height: u16,
        now: std::time::Instant,
    ) {
        self.detail.apply(action, now);
        if matches!(
            action,
            DetailAction::SelectPrevious | DetailAction::SelectNext | DetailAction::ToggleExpansion
        ) {
            let content = super::rows::detail_content(
                &self.list,
                &self.detail,
                self.view.sources_expanded(),
                now,
            );
            super::viewport::ensure_selected_visible(&mut self.view, &content, width, height);
        }
    }

    pub(super) fn apply_scroll(
        &mut self,
        scroll: DetailScroll,
        width: u16,
        height: u16,
        now: std::time::Instant,
    ) {
        let content = super::rows::detail_content(
            &self.list,
            &self.detail,
            self.view.sources_expanded(),
            now,
        );
        super::viewport::apply_scroll(&mut self.view, scroll, &content, width, height);
    }

    pub(super) fn scroll(&self) -> u16 {
        self.view.scroll()
    }

    pub(super) fn set_copy_notice(&mut self, notice: CopyNotice) {
        self.copy_notice = Some(notice);
    }

    pub(super) fn toggle_sources(&mut self, width: u16, height: u16, now: Instant) {
        self.view.toggle_sources();
        super::super::clamp_detail_scroll(
            &mut self.view,
            &self.list,
            &self.detail,
            self.copy_notice,
            now,
            Rect::new(0, 0, width, height),
        );
    }
}

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

pub(super) fn state() -> DetailFixture {
    state_with_changed_lines(&[])
}

pub(super) fn state_with_changed_lines(changed_lines: &[SourceLineChange]) -> DetailFixture {
    state_for_change(change(), changed_lines)
}

pub(super) fn state_with_sources(source_files: Vec<SourceFileAnalysis>) -> DetailFixture {
    state_for_change_with_sources(change(), &[], source_files)
}

pub(super) fn state_for_change(
    change: ResourceChange,
    changed_lines: &[SourceLineChange],
) -> DetailFixture {
    state_for_change_with_sources(
        change,
        changed_lines,
        vec![SourceFileAnalysis::new(
            PathBuf::from("main.tf"),
            SourceSide::After,
            vec![ResourceSourceLocation::new(
                ResourceAddress::new("aws_instance", "api"),
                PathBuf::from("main.tf"),
                SourceSide::After,
                SourceRange::new(42, 46),
            )],
            Vec::new(),
        )],
    )
}

pub(super) fn state_for_change_with_sources(
    change: ResourceChange,
    changed_lines: &[SourceLineChange],
    source_files: Vec<SourceFileAnalysis>,
) -> DetailFixture {
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
    DetailFixture {
        detail: ReviewDetailState::from_list(&list).expect("selected item should open"),
        list,
        view: DetailViewState::default(),
        copy_notice: None,
    }
}

pub(super) fn source_file(
    path: impl Into<PathBuf>,
    side: SourceSide,
    issues: Vec<SourceIssue>,
) -> SourceFileAnalysis {
    SourceFileAnalysis::new(path.into(), side, Vec::new(), issues)
}

pub(super) fn incomplete_source_file(path: impl Into<PathBuf>) -> SourceFileAnalysis {
    source_file(
        path,
        SourceSide::After,
        vec![SourceIssue::new(
            SourceIssueKind::SyntaxError,
            "syntax error in analyzed source",
        )],
    )
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

pub(super) fn select_group(state: &mut DetailFixture, target: &AttributeGroup) {
    for _ in 0..state.detail.rows().len() {
        if state
            .detail
            .rows()
            .get(state.detail.selected())
            .and_then(DetailRow::group)
            == Some(target)
        {
            return;
        }
        state.apply_action(DetailAction::SelectNext, 96, 40, Instant::now());
    }
    panic!("group should be selectable");
}

pub(super) fn select_attribute(state: &mut DetailFixture, target: &str) {
    for _ in 0..state.detail.rows().len() {
        if let Some(DetailRow::Attribute(index)) = state.detail.rows().get(state.detail.selected())
            && format_attribute_path(&state.detail.attributes().attributes[*index].path) == target
        {
            return;
        }
        state.apply_action(DetailAction::SelectNext, 96, 40, Instant::now());
    }
    panic!("attribute {target} should be selectable");
}

pub(super) fn sensitive_sibling_state() -> DetailFixture {
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

pub(super) fn unknown_sensitive_state() -> DetailFixture {
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

pub(super) fn render(state: &DetailFixture, width: u16, height: u16) -> Buffer {
    render_to_buffer((width, height), |frame| {
        render_resource_detail(
            frame,
            &state.list,
            &state.detail,
            state.copy_notice,
            &state.view,
        );
    })
}

pub(super) fn render_at(state: &DetailFixture, width: u16, height: u16, now: Instant) -> Buffer {
    render_to_buffer((width, height), |frame| {
        super::render::render_resource_detail_at(
            frame,
            &state.list,
            &state.detail,
            state.copy_notice,
            &state.view,
            now,
        );
    })
}
