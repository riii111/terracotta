use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::ui::primitives::atoms::separator;
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{footer, header};
use crate::ui::theme;

use super::{MIN_HEIGHT, MIN_WIDTH, ResourceDetailState};

pub(crate) fn render_resource_detail(frame: &mut Frame<'_>, state: &mut ResourceDetailState) {
    render_resource_detail_at(frame, state, Instant::now());
}

pub(crate) fn render_resource_detail_at(
    frame: &mut Frame<'_>,
    state: &mut ResourceDetailState,
    now: Instant,
) {
    state.clear_expired_reveal(now);
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        state.mask_reveal();
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    let title = format!(
        "Terracotta / Resource {}/{}",
        state.item_index() + 1,
        state.total_items()
    );
    let block = Block::new().borders(Borders::ALL).title(title);
    let content_area = block.inner(area);
    frame.render_widget(block, area);

    let notice_height = u16::from(state.is_revealed_at(now));
    let copy_notice_height = u16::from(state.copy_notice().is_some());
    let context_height = u16::from(state.context.is_some()) * 2;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(notice_height),
            Constraint::Length(context_height),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(copy_notice_height),
            Constraint::Length(1),
        ])
        .split(content_area);

    if chunks[4].height == 0 {
        state.mask_reveal();
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    if state.is_revealed_at(now) {
        let remaining = state
            .reveal
            .as_ref()
            .expect("active reveal should have state")
            .expires_at
            .saturating_duration_since(now)
            .as_secs()
            .max(1);
        frame.render_widget(
            Paragraph::new(format!(
                "! Sensitive value revealed                    {remaining}s remaining"
            ))
            .style(theme::warning_style()),
            chunks[0],
        );
    }
    if let Some(context) = &state.context {
        header::render(
            frame,
            chunks[1],
            vec![
                Line::from(format!("cwd {}", context.root().display())),
                Line::from(format!(
                    "workspace {}   git {}",
                    context.workspace(),
                    context.git()
                )),
            ],
        );
    }
    frame.render_widget(
        Paragraph::new(format!("compare {}", state.comparison)),
        chunks[2],
    );
    frame.render_widget(separator::render(chunks[3].width), chunks[3]);

    let content = super::detail_content(state, now);
    let scroll = state.scroll().min(super::max_scroll(
        state,
        chunks[4].width,
        chunks[4].height,
        now,
    ));
    frame.render_widget(
        Paragraph::new(content.lines)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[4],
    );
    if let Some(notice) = state.copy_notice() {
        frame.render_widget(Paragraph::new(notice.message()), chunks[5]);
    }
    footer::render(
        frame,
        chunks[6],
        vec![Line::from(footer_line(state, now, chunks[6].width))],
    );
}

fn footer_line(state: &ResourceDetailState, now: Instant, width: u16) -> String {
    let reveal = if state.is_revealed_at(now) {
        "r mask now"
    } else if state.can_reveal_selected() {
        "r reveal sensitive value for 10s"
    } else {
        ""
    };
    let prefix = if reveal.is_empty() {
        String::new()
    } else {
        format!("{reveal}   ")
    };
    let copy_controls = match (
        state.resource_copy_text.is_some(),
        state.plan_copy_text.is_some(),
    ) {
        (true, true) => "y resource / Y plan | ",
        (false, true) => "Y plan | ",
        (true, false) => "y resource | ",
        (false, false) => "",
    };
    let controls = if width < 110 {
        format!(
            "{copy_controls}Up/Down/j/k select | Enter expand | [ / ] prev/next | Esc back | q quit"
        )
    } else {
        format!(
            "{copy_controls}Up/Down/j/k select   Enter expand/collapse   PageUp/PageDown scroll   [ / ] prev/next   Esc back   q quit"
        )
    };
    format!("{prefix}{controls}")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::buffer::Buffer;
    use serde_json::{Value, json};

    use crate::app::attribution::{
        ResourceAddress, ResourceSourceLocation, SourceRange, SourceSide,
    };
    use crate::app::attribution::{SourceLineChange, attribute_changes};
    use crate::app::plan::{
        AttributeChangeKind, Plan, PlanAction, PlanSummary, PlanValue, ReplacePathSegment,
        ResourceChange, ResourceChangeKind, ResourceMode, format_attribute_path,
    };
    use crate::app::review::{
        PlanReview, ReviewComparison, ReviewComparisonBasis, ReviewComparisonStatus,
    };
    use crate::ui::test_support::{buffer_text, render_to_buffer};

    use super::super::*;

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

    fn change() -> ResourceChange {
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

    fn state() -> ResourceDetailState {
        state_with_changed_lines(&[])
    }

    fn state_with_changed_lines(changed_lines: &[SourceLineChange]) -> ResourceDetailState {
        state_for_change(change(), changed_lines)
    }

    fn state_for_change(
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

    fn filtered_review_list() -> PlanListState {
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

    fn navigation_list() -> PlanListState {
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

    fn expansion_change() -> ResourceChange {
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

    fn select_group(state: &mut ResourceDetailState, target: &AttributeGroup) {
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

    fn select_attribute(state: &mut ResourceDetailState, target: &str) {
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

    fn sensitive_sibling_state() -> ResourceDetailState {
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

    fn unknown_sensitive_state() -> ResourceDetailState {
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

    fn render(state: &ResourceDetailState, width: u16, height: u16) -> Buffer {
        let mut state = state.clone();
        render_to_buffer((width, height), |frame| {
            render_resource_detail(frame, &mut state);
        })
    }

    fn render_at(state: &mut ResourceDetailState, width: u16, height: u16, now: Instant) -> Buffer {
        render_to_buffer((width, height), |frame| {
            super::render_resource_detail_at(frame, state, now);
        })
    }

    #[test]
    fn renders_diff_evidence_masks_special_values_and_omits_future_controls() {
        let state = state();
        let text = buffer_text(&render(&state, 100, 40));

        assert!(text.contains("Terracotta / Resource 1/1"), "{text}");
        assert!(text.contains("Git: no match"), "{text}");
        assert!(
            text.contains("No direct match in analyzed sources."),
            "{text}"
        );
        assert!(text.contains("Analyzed sources:"), "{text}");
        assert!(text.contains("main.tf (after)"), "{text}");
        assert!(text.contains("instance_type"), "{text}");
        assert!(text.contains("t3.small"), "{text}");
        assert!(text.contains("t3.medium"), "{text}");
        assert!(text.contains("private_ip"), "{text}");
        assert!(text.contains("<unknown>"), "{text}");
        assert!(text.contains("password"), "{text}");
        assert!(text.contains("<sensitive>"), "{text}");
        assert!(text.contains("Replacement triggered by:"), "{text}");
        assert!(text.contains("  instance_type"), "{text}");
        assert!(
            text.contains("Replacement reason: replace_because_cannot_update"),
            "{text}"
        );
        assert!(text.contains("Up/Down/j/k select"), "{text}");
        assert!(text.contains("Esc back"), "{text}");
        assert!(text.contains("Enter expand"), "{text}");
        assert!(text.contains("[ / ] prev/next"), "{text}");
        assert!(!text.contains("reveal"), "{text}");
    }

    #[test]
    fn renders_quoted_replacement_paths_without_exposing_sensitive_values() {
        let mut change = change();
        change.replace_paths = Some(vec![vec![
            ReplacePathSegment::Attribute("tags".to_owned()),
            ReplacePathSegment::Attribute("service.name".to_owned()),
            ReplacePathSegment::Index(u64::MAX),
        ]]);

        let state = state_for_change(change, &[]);
        let text = buffer_text(&render(&state, 100, 40));

        assert!(
            text.contains("tags[\"service.name\"][18446744073709551615]"),
            "{text}"
        );
        assert!(text.contains("<sensitive>"), "{text}");
        assert!(!text.contains("old-secret"), "{text}");
        assert!(!text.contains("new-secret"), "{text}");
    }

    #[test]
    fn copying_revealed_resource_keeps_sensitive_values_masked() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();
        state.apply_at(DetailAction::Reveal, 96, 40, now);

        let effect = state
            .copy_effect(CopyTarget::Resource)
            .expect("resource copy should be available");
        let copied_text = effect.text().to_owned();

        assert_eq!(effect.target(), CopyTarget::Resource);
        assert!(copied_text.contains("Resource ~ aws_instance.api"));
        assert!(copied_text.contains("<sensitive>"));
        assert!(!copied_text.contains("old-secret"));
        assert!(!copied_text.contains("new-secret"));
    }

    #[test]
    fn plan_copy_notice_counts_resources_outside_search_scope() {
        let mut list = filtered_review_list();
        list.apply(PlanListAction::BeginSearch);
        list.apply(PlanListAction::SetSearch("api".to_owned()));
        list.apply(PlanListAction::ConfirmSearch);

        let detail = ResourceDetailState::from_list(&list).expect("filtered item should open");
        assert_eq!(detail.total_items(), 1);
        assert_eq!(
            detail
                .copy_effect(CopyTarget::Plan)
                .expect("plan copy should be available")
                .success_notice(),
            CopyNotice::Copied {
                target: CopyTarget::Plan,
                resource_count: 2,
            }
        );
    }

    #[test]
    fn copy_notice_gets_its_own_row_while_reveal_is_active() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();
        state.apply_at(DetailAction::Reveal, 96, 40, now);
        state.set_copy_notice(CopyNotice::Failed);

        let text = buffer_text(&render_at(&mut state, 48, 8, now));

        assert!(
            text.contains("Copy failed: clipboard unavailable."),
            "{text}"
        );
        assert!(text.contains("r mask now"), "{text}");
    }

    #[test]
    fn viewport_height_accounts_for_copy_notice_row() {
        let mut state = state();
        let now = Instant::now();
        let without_notice = state.viewport_height_at(20, now);

        state.set_copy_notice(CopyNotice::Failed);

        assert_eq!(state.viewport_height_at(20, now), without_notice - 1);
    }

    #[test]
    fn reveals_only_selected_known_sensitive_attribute_with_warning_and_expiry() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();

        let before_reveal = buffer_text(&render(&state, 100, 40));
        assert!(before_reveal.contains("r reveal sensitive value for 10s"));
        assert!(!before_reveal.contains("old-secret"));

        state.apply_at(DetailAction::Reveal, 96, 40, now);
        let revealed = buffer_text(&render_at(
            &mut state,
            100,
            40,
            now + Duration::from_secs(2),
        ));
        assert!(revealed.contains("old-secret"), "{revealed}");
        assert!(revealed.contains("new-secret"), "{revealed}");
        assert!(revealed.contains("<sensitive>"), "{revealed}");
        assert!(!revealed.contains("old-token"), "{revealed}");
        assert!(!revealed.contains("new-token"), "{revealed}");
        assert!(revealed.contains("Sensitive value revealed"), "{revealed}");
        assert!(revealed.contains("8s remaining"), "{revealed}");
        assert!(revealed.contains("r mask now"), "{revealed}");

        let expired = buffer_text(&render_at(&mut state, 100, 40, now + REVEAL_DURATION));
        assert!(expired.contains("<sensitive>"), "{expired}");
        assert!(!expired.contains("old-secret"), "{expired}");
        assert!(!expired.contains("Sensitive value revealed"), "{expired}");
        assert!(state.reveal.is_none());
    }

    #[test]
    fn pressing_reveal_again_masks_immediately_and_selection_masks_previous_value() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();
        state.apply_at(DetailAction::Reveal, 96, 40, now);
        assert!(buffer_text(&render_at(&mut state, 100, 40, now)).contains("old-secret"));

        state.apply_at(DetailAction::Reveal, 96, 40, now + Duration::from_secs(1));
        let remasked = buffer_text(&render_at(
            &mut state,
            100,
            40,
            now + Duration::from_secs(1),
        ));
        assert!(!remasked.contains("old-secret"), "{remasked}");
        assert!(state.reveal.is_none());

        state.apply_at(DetailAction::Reveal, 96, 40, now + Duration::from_secs(2));
        state.apply_at(
            DetailAction::SelectNext,
            96,
            40,
            now + Duration::from_secs(3),
        );
        assert!(state.reveal.is_none());
        let changed_selection = buffer_text(&render_at(
            &mut state,
            100,
            40,
            now + Duration::from_secs(3),
        ));
        assert!(
            !changed_selection.contains("old-secret"),
            "{changed_selection}"
        );
    }

    #[test]
    fn unknown_sensitive_attribute_cannot_start_reveal() {
        let mut state = unknown_sensitive_state();
        select_attribute(&mut state, "future_secret");
        let now = Instant::now();

        state.apply_at(DetailAction::Reveal, 96, 40, now);
        let text = buffer_text(&render_at(&mut state, 100, 40, now));
        assert!(text.contains("<sensitive>"), "{text}");
        assert!(!text.contains("not-known-yet"), "{text}");
        assert!(!text.contains("Sensitive value revealed"), "{text}");
        assert!(state.reveal.is_none());
    }

    #[test]
    fn small_terminal_masks_active_reveal_before_normal_rendering_resumes() {
        let mut state = state();
        select_attribute(&mut state, "password");
        let now = Instant::now();
        state.apply_at(DetailAction::Reveal, 96, 40, now);

        let small = buffer_text(&render_at(&mut state, 47, 7, now + Duration::from_secs(1)));
        assert!(small.contains("Terminal too small"), "{small}");
        assert!(state.reveal.is_none());

        let normal = buffer_text(&render_at(
            &mut state,
            100,
            40,
            now + Duration::from_secs(2),
        ));
        assert!(!normal.contains("old-secret"), "{normal}");
        assert!(normal.contains("<sensitive>"), "{normal}");
    }

    #[test]
    fn renders_direct_evidence_with_its_source_side_and_range() {
        let state = state_with_changed_lines(&[SourceLineChange::new(
            "main.tf",
            SourceSide::After,
            SourceRange::new(43, 44),
        )]);
        let text = buffer_text(&render(&state, 100, 24));

        assert!(text.contains("Git: direct"), "{text}");
        assert!(text.contains("main.tf:42-46 (after)"), "{text}");
        assert!(
            text.contains("Resource block overlaps changed lines."),
            "{text}"
        );
        assert!(!text.contains("No direct match"), "{text}");
    }

    #[test]
    fn selects_changed_attributes_and_scrolls_without_exposing_values() {
        let mut state = state();
        let initial_scroll = state.scroll();

        state.apply_at(DetailAction::SelectNext, 46, 4, Instant::now());
        assert!(state.scroll() > initial_scroll);
        state.apply_at(DetailAction::SelectNext, 46, 4, Instant::now());
        let text = buffer_text(&render(&state, 48, 12));
        assert!(text.contains("> password"), "{text}");

        state.apply_at(DetailAction::PageDown, 46, 4, Instant::now());
        let text = buffer_text(&render(&state, 48, 12));
        assert!(!text.contains("synthetic-secret"), "{text}");
    }

    #[test]
    fn resource_navigation_follows_search_order_and_resets_detail_state() {
        let mut list = navigation_list();
        list.apply(PlanListAction::BeginSearch);
        list.apply(PlanListAction::SetSearch("aws_instance".to_owned()));
        list.apply(PlanListAction::ConfirmSearch);

        let mut detail = ResourceDetailState::from_list(&list).expect("resource should open");
        select_attribute(&mut detail, "password");
        detail.apply_at(DetailAction::Reveal, 96, 40, Instant::now());
        assert!(detail.reveal.is_some());
        detail.selected = 1;
        detail.scroll = 3;
        detail.expanded_groups.push(AttributeGroup::Unchanged);

        detail.navigate(ResourceNavigation::Next, &mut list);

        assert_eq!(detail.item.address(), "aws_instance.worker");
        assert_eq!(detail.item_index(), 1);
        assert_eq!(detail.total_items(), 2);
        assert_eq!(detail.selected, 0);
        assert_eq!(detail.scroll(), 0);
        assert!(detail.expanded_groups.is_empty());
        assert!(detail.reveal.is_none());
        assert_eq!(list.selected(), Some(1));

        detail.navigate(ResourceNavigation::Next, &mut list);
        assert_eq!(detail.item.address(), "aws_instance.worker");
        assert_eq!(list.selected(), Some(1));

        detail.navigate(ResourceNavigation::Previous, &mut list);
        assert_eq!(detail.item.address(), "aws_instance.api");
        assert_eq!(list.selected(), Some(0));
    }

    #[test]
    fn resource_navigation_stops_when_search_has_one_item() {
        let mut list = navigation_list();
        list.apply(PlanListAction::BeginSearch);
        list.apply(PlanListAction::SetSearch("worker".to_owned()));
        list.apply(PlanListAction::ConfirmSearch);
        let mut detail = ResourceDetailState::from_list(&list).expect("resource should open");

        detail.navigate(ResourceNavigation::Previous, &mut list);
        detail.navigate(ResourceNavigation::Next, &mut list);

        assert_eq!(detail.item.address(), "aws_instance.worker");
        assert_eq!(detail.item_index(), 0);
        assert_eq!(detail.total_items(), 1);
        assert_eq!(list.selected(), Some(0));
    }

    #[test]
    fn expands_unchanged_group_and_keeps_selection_on_group_row() {
        let mut state = state_for_change(expansion_change(), &[]);
        let unchanged = AttributeGroup::Unchanged;
        select_group(&mut state, &unchanged);
        let group_index = state.selected;

        state.apply_at(DetailAction::ToggleExpansion, 96, 40, Instant::now());

        assert_eq!(state.selected, group_index);
        let text = buffer_text(&render(&state, 100, 60));
        assert!(
            text.contains("> [v] 2 unchanged attributes  [Enter collapse]"),
            "{text}"
        );
        assert!(text.contains("root_unchanged"), "{text}");

        state.apply_at(DetailAction::SelectNext, 96, 40, Instant::now());
        assert!(matches!(
            detail_rows(&state).get(state.selected),
            Some(DetailRow::Group {
                group: AttributeGroup::Nested {
                    kind: AttributeChangeKind::Unchanged,
                    ..
                },
                ..
            })
        ));

        state.apply_at(DetailAction::SelectPrevious, 96, 40, Instant::now());
        state.apply_at(DetailAction::ToggleExpansion, 96, 40, Instant::now());
        assert_eq!(state.selected, group_index);
        let text = buffer_text(&render(&state, 100, 60));
        assert!(
            text.contains("> [>] 2 unchanged attributes hidden  [Enter expand]"),
            "{text}"
        );
        assert!(!text.contains("root_unchanged"), "{text}");
    }

    #[test]
    fn expands_nested_group_and_masks_sensitive_children() {
        let mut state = state_for_change(expansion_change(), &[]);
        let group = AttributeGroup::Nested {
            kind: AttributeChangeKind::Changed,
            path: vec![AttributePathSegment::Key("group_b".to_owned())],
        };
        select_group(&mut state, &group);

        state.apply_at(DetailAction::ToggleExpansion, 96, 40, Instant::now());
        let text = buffer_text(&render(&state, 100, 60));

        assert!(
            text.contains("> [v] group_b: 1 changed attribute  [Enter collapse]"),
            "{text}"
        );
        assert!(text.contains("group_b.secret"), "{text}");
        assert!(text.contains("<sensitive>"), "{text}");
        assert!(!text.contains("old-secret"), "{text}");
        assert!(!text.contains("new-secret"), "{text}");
    }

    #[test]
    fn reveals_sensitive_children_when_nested_group_is_selected() {
        let mut state = state_for_change(expansion_change(), &[]);
        let group = AttributeGroup::Nested {
            kind: AttributeChangeKind::Changed,
            path: vec![AttributePathSegment::Key("group_b".to_owned())],
        };
        select_group(&mut state, &group);
        state.apply_at(DetailAction::ToggleExpansion, 96, 40, Instant::now());
        let now = Instant::now();

        assert!(buffer_text(&render_at(&mut state, 100, 60, now)).contains("r reveal"));
        state.apply_at(DetailAction::Reveal, 96, 40, now);
        let text = buffer_text(&render_at(&mut state, 100, 60, now));

        assert!(text.contains("old-secret"), "{text}");
        assert!(text.contains("new-secret"), "{text}");
        assert!(text.contains("Sensitive value revealed"), "{text}");
    }

    #[test]
    fn expanding_deep_group_keeps_child_selection_and_scrolls_to_it() {
        let mut state = state_for_change(expansion_change(), &[]);
        let group = AttributeGroup::Nested {
            kind: AttributeChangeKind::Changed,
            path: vec![AttributePathSegment::Key("group_a".to_owned())],
        };
        select_group(&mut state, &group);

        state.apply_at(DetailAction::ToggleExpansion, 36, 4, Instant::now());
        state.apply_at(DetailAction::SelectNext, 36, 4, Instant::now());
        state.apply_at(DetailAction::SelectNext, 36, 4, Instant::now());
        assert!(state.scroll() > 0);

        let nested_group = AttributeGroup::Nested {
            kind: AttributeChangeKind::Changed,
            path: vec![
                AttributePathSegment::Key("group_a".to_owned()),
                AttributePathSegment::Key("nested".to_owned()),
            ],
        };
        assert_eq!(
            detail_rows(&state)
                .get(state.selected)
                .and_then(DetailRow::group),
            Some(&nested_group)
        );
        state.apply_at(DetailAction::ToggleExpansion, 36, 4, Instant::now());
        state.apply_at(DetailAction::SelectNext, 36, 4, Instant::now());
        let text = buffer_text(&render(&state, 48, 12));
        assert!(text.contains("> group_a.nested.value"), "{text}");
    }

    #[test]
    fn page_down_stops_at_the_last_wrapped_line() {
        let mut state = state();

        for _ in 0..100 {
            state.apply_at(DetailAction::PageDown, 46, 4, Instant::now());
        }
        let last_scroll = state.scroll();
        state.apply_at(DetailAction::PageDown, 46, 4, Instant::now());

        assert_eq!(state.scroll(), last_scroll);
    }

    #[test]
    fn detail_wraps_long_values_and_paths_instead_of_truncating_them() {
        let state = state();
        let text = buffer_text(&render(&state, 60, 32));

        assert!(
            text.contains("another-value-that-is-long-enough-to-wrap"),
            "{text}"
        );
        assert!(!text.contains("..."), "{text}");
    }
}
