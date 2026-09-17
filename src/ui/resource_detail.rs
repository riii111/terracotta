use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::attribute_diff::{
    AttributeChangeKind, AttributeDiff, AttributeDiffs, AttributePathSegment,
};
use crate::app::attribution::{AttributionStatus, ResourceAttribution};
use crate::app::plan::{ReplacePathSegment, ResourceChangeKind};
use crate::app::plan_list::{PlanListContext, PlanListItem, PlanListState};
use crate::app::source_location::{SourceFileAnalysis, SourceSide};

const MIN_HEIGHT: u16 = 8;
const MIN_WIDTH: u16 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DetailAction {
    SelectPrevious,
    SelectNext,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DetailInput {
    Action(DetailAction),
    Back,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResourceDetailState {
    context: Option<PlanListContext>,
    comparison: String,
    item: PlanListItem,
    attributes: AttributeDiffs,
    source_files: Vec<SourceFileAnalysis>,
    index: usize,
    total: usize,
    selected: usize,
    scroll: u16,
}

impl ResourceDetailState {
    pub(super) fn from_list(state: &PlanListState) -> Option<Self> {
        let item = state.items().get(state.selected())?.clone();
        Some(Self {
            context: state.context().cloned(),
            comparison: state.comparison().to_owned(),
            attributes: item.attribute_diffs(),
            source_files: state.source_files().to_vec(),
            item,
            index: state.selected(),
            total: state.items().len(),
            selected: 0,
            scroll: 0,
        })
    }

    pub(super) fn apply(&mut self, action: DetailAction, viewport_height: u16) {
        let page = viewport_height.max(1);
        match action {
            DetailAction::SelectPrevious => {
                self.selected = self.selected.saturating_sub(1);
                self.ensure_selected_visible(page);
            }
            DetailAction::SelectNext => {
                if let Some(last) = self.attributes.changed_count.checked_sub(1) {
                    self.selected = (self.selected + 1).min(last);
                    self.ensure_selected_visible(page);
                }
            }
            DetailAction::PageUp => self.scroll = self.scroll.saturating_sub(page),
            DetailAction::PageDown => self.scroll = self.scroll.saturating_add(page),
        }
    }

    pub(super) const fn scroll(&self) -> u16 {
        self.scroll
    }

    pub(super) const fn item_index(&self) -> usize {
        self.index
    }

    pub(super) const fn total_items(&self) -> usize {
        self.total
    }

    pub(super) fn viewport_height(&self, total_height: u16) -> u16 {
        total_height.saturating_sub(5 + u16::from(self.context.is_some()) * 2)
    }

    fn ensure_selected_visible(&mut self, viewport_height: u16) {
        let Some(selected_line) = detail_content(self).selected_line else {
            return;
        };
        if selected_line < self.scroll {
            self.scroll = selected_line;
        } else if selected_line >= self.scroll.saturating_add(viewport_height) {
            self.scroll = selected_line.saturating_sub(viewport_height.saturating_sub(1));
        }
    }
}

pub(super) fn key_to_input(key: KeyEvent) -> Option<DetailInput> {
    if key.code == KeyCode::Esc {
        return Some(DetailInput::Back);
    }
    if matches!(key.code, KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return Some(DetailInput::Quit);
    }

    let action = match key.code {
        KeyCode::Up | KeyCode::Char('k') => DetailAction::SelectPrevious,
        KeyCode::Down | KeyCode::Char('j') => DetailAction::SelectNext,
        KeyCode::PageUp => DetailAction::PageUp,
        KeyCode::PageDown => DetailAction::PageDown,
        _ => return None,
    };
    Some(DetailInput::Action(action))
}

pub(super) fn render_resource_detail(frame: &mut Frame<'_>, state: &ResourceDetailState) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_terminal_too_small(frame, area);
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

    let context_height = u16::from(state.context.is_some()) * 2;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(context_height),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(content_area);

    if let Some(context) = &state.context {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("cwd {}", context.root().display())),
                Line::from(format!(
                    "workspace {}   git {}",
                    context.workspace(),
                    context.git()
                )),
            ]),
            chunks[0],
        );
    }
    frame.render_widget(
        Paragraph::new(format!("compare {}", state.comparison)),
        chunks[1],
    );
    frame.render_widget(separator(chunks[2].width), chunks[2]);

    let content = detail_content(state);
    frame.render_widget(
        Paragraph::new(content.lines)
            .scroll((state.scroll(), 0))
            .wrap(Wrap { trim: false }),
        chunks[3],
    );
    frame.render_widget(Paragraph::new(footer_line()), chunks[4]);
}

fn detail_content(state: &ResourceDetailState) -> DetailContent {
    let mut lines = Vec::new();
    let mut selected_line = None;

    lines.push(Line::from(vec![
        Span::styled(
            action_symbol(state.item.kind()),
            action_style(state.item.kind()),
        ),
        Span::raw(" "),
        Span::raw(state.item.address().to_owned()),
    ]));
    lines.push(Line::default());
    append_attribution(&mut lines, state.item.attribution(), &state.source_files);
    lines.push(Line::default());
    lines.push(Line::from("Diff:"));

    let mut changed_index = 0;
    for attribute in &state.attributes.attributes {
        if attribute.kind != AttributeChangeKind::Changed {
            continue;
        }
        if changed_index == state.selected {
            selected_line = Some(u16::try_from(lines.len()).unwrap_or(u16::MAX));
        }
        append_attribute(&mut lines, attribute, changed_index == state.selected);
        changed_index += 1;
    }
    if changed_index == 0 {
        lines.push(Line::from("  No changed attributes."));
    }

    append_replacement(&mut lines, &state.attributes);
    if state.attributes.unchanged_count > 0 {
        lines.push(Line::default());
        lines.push(Line::from(format!(
            "[>] {} unchanged {} hidden",
            state.attributes.unchanged_count,
            pluralize(state.attributes.unchanged_count, "attribute", "attributes")
        )));
    }

    DetailContent {
        lines,
        selected_line,
    }
}

fn append_attribution(
    lines: &mut Vec<Line<'static>>,
    attribution: &ResourceAttribution,
    source_files: &[SourceFileAnalysis],
) {
    lines.push(Line::from(format!(
        "Git: {}",
        attribution_label(attribution)
    )));

    if attribution.status() == AttributionStatus::NoMatch {
        lines.push(Line::from("  No direct match in analyzed sources."));
    }
    for evidence in attribution.evidence() {
        let range = evidence.range();
        let location = if range.start_line() == range.end_line() {
            format!("{}:{}", evidence.path().display(), range.start_line())
        } else {
            format!(
                "{}:{}-{}",
                evidence.path().display(),
                range.start_line(),
                range.end_line()
            )
        };
        lines.push(Line::from(format!(
            "  {location} ({})",
            source_side_label(evidence.side())
        )));
    }
    if attribution.status() == AttributionStatus::Direct && !attribution.evidence().is_empty() {
        lines.push(Line::from("  Resource block overlaps changed lines."));
    }
    if !attribution.analysis().is_complete() {
        lines.push(Line::from("  Analysis incomplete:"));
        for issue in attribution.analysis().issues() {
            lines.push(Line::from(format!("    {}", issue.message())));
        }
    }

    lines.push(Line::from("  Analyzed sources:"));
    if source_files.is_empty() {
        lines.push(Line::from("    none"));
    } else {
        for source in source_files {
            let suffix = if source.is_complete() {
                String::new()
            } else {
                " [incomplete]".to_owned()
            };
            lines.push(Line::from(format!(
                "    {} ({}){suffix}",
                source.path().display(),
                source_side_label(source.side())
            )));
        }
    }
}

fn append_attribute(lines: &mut Vec<Line<'static>>, attribute: &AttributeDiff, selected: bool) {
    let marker = if selected { "> " } else { "  " };
    lines.push(Line::from(vec![
        Span::raw(marker),
        Span::raw(attribute_path(&attribute.path)),
    ]));
    lines.push(Line::from(format!("    - {}", attribute.before.display())));
    lines.push(Line::from(format!("    + {}", attribute.after.display())));
}

fn append_replacement(lines: &mut Vec<Line<'static>>, attributes: &AttributeDiffs) {
    let paths = attributes.replace_paths.as_deref().unwrap_or_default();
    if paths.is_empty() && attributes.action_reason.is_none() {
        return;
    }

    lines.push(Line::default());
    lines.push(Line::from("Replacement triggered by:"));
    for path in paths {
        lines.push(Line::from(format!("  {}", replacement_path(path))));
    }
    if let Some(reason) = &attributes.action_reason {
        lines.push(Line::from(format!("Replacement reason: {reason}")));
    }
}

const fn attribution_label(attribution: &ResourceAttribution) -> &'static str {
    if attribution.analysis().is_complete() {
        match attribution.status() {
            AttributionStatus::Direct => "direct",
            AttributionStatus::NoMatch => "no match",
        }
    } else {
        "incomplete"
    }
}

fn attribute_path(path: &[AttributePathSegment]) -> String {
    let mut result = String::new();
    for segment in path {
        match segment {
            AttributePathSegment::Key(key) => {
                if !result.is_empty() {
                    result.push('.');
                }
                result.push_str(key);
            }
            AttributePathSegment::Index(index) => {
                result.push('[');
                result.push_str(&index.to_string());
                result.push(']');
            }
        }
    }
    if result.is_empty() {
        "<resource>".to_owned()
    } else {
        result
    }
}

fn replacement_path(path: &[ReplacePathSegment]) -> String {
    let mut result = String::new();
    for segment in path {
        match segment {
            ReplacePathSegment::Attribute(attribute) => {
                if !result.is_empty() {
                    result.push('.');
                }
                result.push_str(attribute);
            }
            ReplacePathSegment::Index(index) => {
                result.push('[');
                result.push_str(&index.to_string());
                result.push(']');
            }
        }
    }
    if result.is_empty() {
        "<resource>".to_owned()
    } else {
        result
    }
}

const fn source_side_label(side: SourceSide) -> &'static str {
    match side {
        SourceSide::Before => "before",
        SourceSide::After => "after",
    }
}

const fn action_symbol(kind: ResourceChangeKind) -> &'static str {
    match kind {
        ResourceChangeKind::Create => "+",
        ResourceChangeKind::Update => "~",
        ResourceChangeKind::Replace => "R",
        ResourceChangeKind::Delete => "-",
    }
}

fn action_style(kind: ResourceChangeKind) -> Style {
    let color = match kind {
        ResourceChangeKind::Create => Color::Green,
        ResourceChangeKind::Update => Color::Yellow,
        ResourceChangeKind::Replace => Color::Magenta,
        ResourceChangeKind::Delete => Color::Red,
    };
    Style::default().fg(color)
}

const fn pluralize(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 { singular } else { plural }
}

fn render_terminal_too_small(frame: &mut Frame<'_>, area: Rect) {
    let message = Paragraph::new("Terminal too small. Resize or press q to quit.")
        .style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(message, area);
}

fn separator(width: u16) -> Paragraph<'static> {
    Paragraph::new("─".repeat(width as usize)).style(Style::default().fg(Color::DarkGray))
}

const fn footer_line() -> &'static str {
    "Up/Down/j/k select   PageUp/PageDown scroll   Esc back   q quit"
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetailContent {
    lines: Vec<Line<'static>>,
    selected_line: Option<u16>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossterm::event::{KeyEventKind, KeyEventState};
    use ratatui::buffer::Buffer;
    use serde_json::{Value, json};

    use crate::app::attribution::{SourceLineChange, attribute_changes};
    use crate::app::plan::{
        Plan, PlanAction, PlanSummary, PlanValue, ResourceChange, ResourceMode,
    };
    use crate::app::review::{
        PlanReview, ReviewComparison, ReviewComparisonBasis, ReviewComparisonStatus,
    };
    use crate::app::source_location::{ResourceAddress, ResourceSourceLocation, SourceRange};
    use crate::ui::test_support::{buffer_text, render_to_buffer};

    use super::*;

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
        let change = change();
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

    fn render(state: &ResourceDetailState, width: u16, height: u16) -> Buffer {
        render_to_buffer((width, height), |frame| {
            render_resource_detail(frame, state);
        })
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
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
        assert!(
            text.contains("Replacement reason: replace_because_cannot_update"),
            "{text}"
        );
        assert!(text.contains("Up/Down/j/k select"), "{text}");
        assert!(text.contains("Esc back"), "{text}");
        assert!(!text.contains("expand"), "{text}");
        assert!(!text.contains("reveal"), "{text}");
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

        assert_eq!(
            key_to_input(key(KeyCode::Down)),
            Some(DetailInput::Action(DetailAction::SelectNext))
        );
        state.apply(DetailAction::SelectNext, 4);
        assert!(state.scroll() > initial_scroll);

        state.apply(DetailAction::PageDown, 4);
        let text = buffer_text(&render(&state, 48, 12));
        assert!(!text.contains("synthetic-secret"), "{text}");
        assert!(text.contains("PageUp/PageDown scroll"), "{text}");
        assert_eq!(key_to_input(key(KeyCode::Esc)), Some(DetailInput::Back));
        assert_eq!(
            key_to_input(key(KeyCode::Char('q'))),
            Some(DetailInput::Quit)
        );
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
