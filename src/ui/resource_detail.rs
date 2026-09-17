use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::buffer::CellWidth;
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
    ToggleExpansion,
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
enum AttributeGroup {
    Unchanged,
    Nested {
        kind: AttributeChangeKind,
        path: Vec<AttributePathSegment>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DetailRow {
    Attribute(usize),
    Group { group: AttributeGroup, count: usize },
}

impl DetailRow {
    const fn group(&self) -> Option<&AttributeGroup> {
        match self {
            Self::Attribute(_) => None,
            Self::Group { group, .. } => Some(group),
        }
    }
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
    expanded_groups: Vec<AttributeGroup>,
}

impl ResourceDetailState {
    pub(super) fn from_list(state: &PlanListState) -> Option<Self> {
        let item = state.selected_item()?.clone();
        Some(Self {
            context: state.context().cloned(),
            comparison: state.comparison().to_owned(),
            attributes: item.attribute_diffs(),
            source_files: state.source_files().to_vec(),
            item,
            index: state.selected()?,
            total: state.visible_count(),
            selected: 0,
            scroll: 0,
            expanded_groups: Vec::new(),
        })
    }

    pub(super) fn apply(
        &mut self,
        action: DetailAction,
        viewport_width: u16,
        viewport_height: u16,
    ) {
        let page = viewport_height.max(1);
        match action {
            DetailAction::SelectPrevious => {
                self.selected = self.selected.saturating_sub(1);
                self.ensure_selected_visible(viewport_width, page);
            }
            DetailAction::SelectNext => {
                if let Some(last) = detail_rows(self).len().checked_sub(1) {
                    self.selected = (self.selected + 1).min(last);
                    self.ensure_selected_visible(viewport_width, page);
                }
            }
            DetailAction::ToggleExpansion => {
                let group = detail_rows(self)
                    .get(self.selected)
                    .and_then(DetailRow::group)
                    .cloned();
                if let Some(group) = group {
                    if let Some(index) = self.expanded_groups.iter().position(|item| *item == group)
                    {
                        self.expanded_groups.remove(index);
                    } else {
                        self.expanded_groups.push(group);
                    }
                    self.ensure_selected_visible(viewport_width, page);
                }
            }
            DetailAction::PageUp => self.scroll = self.scroll.saturating_sub(page),
            DetailAction::PageDown => {
                self.scroll =
                    self.scroll
                        .saturating_add(page)
                        .min(max_scroll(self, viewport_width, page));
            }
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

    fn ensure_selected_visible(&mut self, viewport_width: u16, viewport_height: u16) {
        let content = detail_content(self);
        let Some(selected_line) = wrapped_selected_line(&content, viewport_width) else {
            return;
        };
        let selected_line = u16::try_from(selected_line).unwrap_or(u16::MAX);
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
        KeyCode::Enter => DetailAction::ToggleExpansion,
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
    let scroll = state
        .scroll()
        .min(max_scroll(state, chunks[3].width, chunks[3].height));
    frame.render_widget(
        Paragraph::new(content.lines)
            .scroll((scroll, 0))
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

    let has_changed_attributes = state
        .attributes
        .attributes
        .iter()
        .any(|attribute| attribute.kind == AttributeChangeKind::Changed);
    if !has_changed_attributes {
        lines.push(Line::from("  No changed attributes."));
    }

    let rows = detail_rows(state);
    for (row_index, row) in rows.iter().enumerate() {
        if row_index == state.selected {
            selected_line = Some(lines.len());
        }
        match row {
            DetailRow::Attribute(attribute_index) => append_attribute(
                &mut lines,
                &state.attributes.attributes[*attribute_index],
                row_index == state.selected,
            ),
            DetailRow::Group { group, count } => append_group(
                &mut lines,
                group,
                *count,
                state.expanded_groups.contains(group),
                row_index == state.selected,
            ),
        }
    }

    append_replacement(&mut lines, &state.attributes);

    DetailContent {
        lines,
        selected_line,
    }
}

fn detail_rows(state: &ResourceDetailState) -> Vec<DetailRow> {
    let mut rows = Vec::new();
    append_attribute_rows(
        &mut rows,
        &state.attributes.attributes,
        AttributeChangeKind::Changed,
        &[],
        &state.expanded_groups,
    );

    let unchanged_count = state
        .attributes
        .attributes
        .iter()
        .filter(|attribute| attribute.kind == AttributeChangeKind::Unchanged)
        .count();
    if unchanged_count > 0 {
        let group = AttributeGroup::Unchanged;
        rows.push(DetailRow::Group {
            group: group.clone(),
            count: unchanged_count,
        });
        if state.expanded_groups.contains(&group) {
            append_attribute_rows(
                &mut rows,
                &state.attributes.attributes,
                AttributeChangeKind::Unchanged,
                &[],
                &state.expanded_groups,
            );
        }
    }
    rows
}

fn append_attribute_rows(
    rows: &mut Vec<DetailRow>,
    attributes: &[AttributeDiff],
    kind: AttributeChangeKind,
    parent: &[AttributePathSegment],
    expanded_groups: &[AttributeGroup],
) {
    let mut items = Vec::new();
    for (index, attribute) in attributes.iter().enumerate() {
        if attribute.kind != kind
            || attribute.path.len() <= parent.len()
            || !attribute.path.starts_with(parent)
        {
            continue;
        }

        let segment = &attribute.path[parent.len()];
        let item = if attribute.path.len() == parent.len() + 1 {
            AttributeItem::Attribute(index)
        } else {
            let mut path = parent.to_vec();
            path.push(segment.clone());
            AttributeItem::Group(path)
        };
        if !items.contains(&item) {
            items.push(item);
        }
    }

    for item in items {
        match item {
            AttributeItem::Attribute(index) => rows.push(DetailRow::Attribute(index)),
            AttributeItem::Group(path) => {
                let group = AttributeGroup::Nested {
                    kind,
                    path: path.clone(),
                };
                let count = attributes
                    .iter()
                    .filter(|attribute| attribute.kind == kind && attribute.path.starts_with(&path))
                    .count();
                rows.push(DetailRow::Group {
                    group: group.clone(),
                    count,
                });
                if expanded_groups.contains(&group) {
                    append_attribute_rows(rows, attributes, kind, &path, expanded_groups);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AttributeItem {
    Attribute(usize),
    Group(Vec<AttributePathSegment>),
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

fn append_group(
    lines: &mut Vec<Line<'static>>,
    group: &AttributeGroup,
    count: usize,
    expanded: bool,
    selected: bool,
) {
    let marker = if selected { "> " } else { "  " };
    let toggle = if expanded { "[v]" } else { "[>]" };
    let action = if expanded { "collapse" } else { "expand" };
    let hidden = if expanded { "" } else { " hidden" };
    let label = match group {
        AttributeGroup::Unchanged => format!(
            "{count} unchanged {}{hidden}",
            pluralize(count, "attribute", "attributes")
        ),
        AttributeGroup::Nested { kind, path } => format!(
            "{}: {count} {}{hidden}",
            attribute_path(path),
            group_kind_label(*kind, count),
        ),
    };
    lines.push(Line::from(format!(
        "{marker}{toggle} {label}  [Enter {action}]"
    )));
}

const fn group_kind_label(kind: AttributeChangeKind, count: usize) -> &'static str {
    match kind {
        AttributeChangeKind::Changed => {
            if count == 1 {
                "changed attribute"
            } else {
                "changed attributes"
            }
        }
        AttributeChangeKind::Unchanged => {
            if count == 1 {
                "unchanged attribute"
            } else {
                "unchanged attributes"
            }
        }
    }
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
    "Up/Down/j/k select   Enter expand/collapse   PageUp/PageDown scroll   Esc back   q quit"
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetailContent {
    lines: Vec<Line<'static>>,
    selected_line: Option<usize>,
}

fn max_scroll(state: &ResourceDetailState, viewport_width: u16, viewport_height: u16) -> u16 {
    let content = detail_content(state);
    let max_scroll =
        wrapped_line_count(&content, viewport_width).saturating_sub(usize::from(viewport_height));
    u16::try_from(max_scroll).unwrap_or(u16::MAX)
}

fn wrapped_selected_line(content: &DetailContent, viewport_width: u16) -> Option<usize> {
    let selected_line = content.selected_line?;
    Some(
        content.lines[..selected_line]
            .iter()
            .map(|line| wrapped_line_count_for_line(line, viewport_width.max(1)))
            .sum(),
    )
}

fn wrapped_line_count(content: &DetailContent, viewport_width: u16) -> usize {
    content
        .lines
        .iter()
        .map(|line| wrapped_line_count_for_line(line, viewport_width.max(1)))
        .sum()
}

fn wrapped_line_count_for_line(line: &Line<'_>, max_width: u16) -> usize {
    let mut line_width: u16 = 0;
    let mut word_width: u16 = 0;
    let mut whitespace_width: u16 = 0;
    let mut whitespace = std::collections::VecDeque::new();
    let mut line_has_content = false;
    let mut word_has_content = false;
    let mut non_whitespace_previous = false;
    let mut count = 0;

    for grapheme in line.styled_graphemes(Style::default()) {
        let is_whitespace = grapheme.is_whitespace();
        let symbol_width = grapheme.symbol.cell_width();
        if symbol_width > max_width {
            continue;
        }

        let word_found = non_whitespace_previous && is_whitespace;
        let untrimmed_overflow = !line_has_content
            && word_width
                .saturating_add(whitespace_width)
                .saturating_add(symbol_width)
                > max_width;
        if word_found || untrimmed_overflow {
            if !whitespace.is_empty() {
                line_has_content = true;
            }
            if word_has_content {
                line_has_content = true;
            }
            line_width = line_width
                .saturating_add(whitespace_width)
                .saturating_add(word_width);
            whitespace.clear();
            whitespace_width = 0;
            word_width = 0;
            word_has_content = false;
        }

        let line_full = line_width >= max_width;
        let pending_word_overflow = symbol_width > 0
            && line_width
                .saturating_add(whitespace_width)
                .saturating_add(word_width)
                >= max_width;
        if line_full || pending_word_overflow {
            count += 1;
            let mut remaining_width = max_width.saturating_sub(line_width);
            while let Some(width) = whitespace.front().copied() {
                if width > remaining_width {
                    break;
                }
                whitespace.pop_front();
                whitespace_width = whitespace_width.saturating_sub(width);
                remaining_width = remaining_width.saturating_sub(width);
            }
            line_width = 0;
            line_has_content = false;
            if is_whitespace && whitespace.is_empty() {
                continue;
            }
        }

        if is_whitespace {
            whitespace_width = whitespace_width.saturating_add(symbol_width);
            whitespace.push_back(symbol_width);
        } else {
            word_width = word_width.saturating_add(symbol_width);
            word_has_content = true;
        }
        non_whitespace_previous = !is_whitespace;
    }

    count + 1
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
            state.apply(DetailAction::SelectNext, 96, 40);
        }
        panic!("group should be selectable");
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
        assert!(text.contains("Enter expand/collapse"), "{text}");
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
        state.apply(DetailAction::SelectNext, 46, 4);
        assert!(state.scroll() > initial_scroll);
        state.apply(DetailAction::SelectNext, 46, 4);
        let text = buffer_text(&render(&state, 48, 12));
        assert!(text.contains("> password"), "{text}");

        state.apply(DetailAction::PageDown, 46, 4);
        let text = buffer_text(&render(&state, 48, 12));
        assert!(!text.contains("synthetic-secret"), "{text}");
        assert_eq!(
            key_to_input(key(KeyCode::PageDown)),
            Some(DetailInput::Action(DetailAction::PageDown))
        );
        assert_eq!(key_to_input(key(KeyCode::Esc)), Some(DetailInput::Back));
        assert_eq!(
            key_to_input(key(KeyCode::Char('q'))),
            Some(DetailInput::Quit)
        );
        assert_eq!(
            key_to_input(key(KeyCode::Enter)),
            Some(DetailInput::Action(DetailAction::ToggleExpansion))
        );
    }

    #[test]
    fn expands_unchanged_group_and_keeps_selection_on_group_row() {
        let mut state = state_for_change(expansion_change(), &[]);
        let unchanged = AttributeGroup::Unchanged;
        select_group(&mut state, &unchanged);
        let group_index = state.selected;

        state.apply(DetailAction::ToggleExpansion, 96, 40);

        assert_eq!(state.selected, group_index);
        let text = buffer_text(&render(&state, 100, 60));
        assert!(
            text.contains("> [v] 2 unchanged attributes  [Enter collapse]"),
            "{text}"
        );
        assert!(text.contains("root_unchanged"), "{text}");

        state.apply(DetailAction::SelectNext, 96, 40);
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

        state.apply(DetailAction::SelectPrevious, 96, 40);
        state.apply(DetailAction::ToggleExpansion, 96, 40);
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

        state.apply(DetailAction::ToggleExpansion, 96, 40);
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
    fn expanding_deep_group_keeps_child_selection_and_scrolls_to_it() {
        let mut state = state_for_change(expansion_change(), &[]);
        let group = AttributeGroup::Nested {
            kind: AttributeChangeKind::Changed,
            path: vec![AttributePathSegment::Key("group_a".to_owned())],
        };
        select_group(&mut state, &group);

        state.apply(DetailAction::ToggleExpansion, 36, 4);
        state.apply(DetailAction::SelectNext, 36, 4);
        state.apply(DetailAction::SelectNext, 36, 4);
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
        state.apply(DetailAction::ToggleExpansion, 36, 4);
        state.apply(DetailAction::SelectNext, 36, 4);
        let text = buffer_text(&render(&state, 48, 12));
        assert!(text.contains("> group_a.nested.value"), "{text}");
    }

    #[test]
    fn page_down_stops_at_the_last_wrapped_line() {
        let mut state = state();

        for _ in 0..100 {
            state.apply(DetailAction::PageDown, 46, 4);
        }
        let last_scroll = state.scroll();
        state.apply(DetailAction::PageDown, 46, 4);

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
