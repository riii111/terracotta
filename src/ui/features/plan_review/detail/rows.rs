use ratatui::text::{Line, Span};

use crate::app::attribution::{AttributionStatus, ResourceAttribution};
use crate::app::attribution::{SourceFileAnalysis, SourceSide};
use crate::app::plan::{
    AttributeChangeKind, AttributeDiff, AttributeDiffs, AttributePathSegment, AttributeValue,
    format_attribute_path, format_replace_path,
};
use crate::app::review::{AttributeGroup, DetailRow};
use crate::ui::theme;

use super::ResourceDetailState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetailContent {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) selected_line: Option<usize>,
}

pub(super) fn detail_content(
    state: &ResourceDetailState,
    now: std::time::Instant,
) -> DetailContent {
    let mut lines = Vec::new();
    let mut selected_line = None;

    lines.push(Line::from(vec![
        Span::styled(
            theme::action_symbol(state.item.kind()),
            theme::action_style(state.item.kind()),
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
                state.reveals_attribute(&state.attributes.attributes[*attribute_index], now),
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

pub(super) fn detail_rows(state: &ResourceDetailState) -> Vec<DetailRow> {
    let mut rows = Vec::new();
    append_attribute_rows(
        &mut rows,
        &state.attributes.attributes,
        AttributeChangeKind::Changed,
        &[],
        &state.expanded_groups,
    );

    let unchanged_count = state.attributes.unchanged_count;
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

fn append_attribute(
    lines: &mut Vec<Line<'static>>,
    attribute: &AttributeDiff,
    selected: bool,
    reveal: bool,
) {
    let marker = if selected { "> " } else { "  " };
    lines.push(Line::from(vec![
        Span::raw(marker),
        Span::raw(format_attribute_path(&attribute.path)),
    ]));
    lines.push(Line::from(format!(
        "    - {}",
        display_attribute_value(&attribute.before, reveal)
    )));
    lines.push(Line::from(format!(
        "    + {}",
        display_attribute_value(&attribute.after, reveal)
    )));
}

fn display_attribute_value(value: &AttributeValue, reveal: bool) -> String {
    if reveal {
        value.revealed_display().unwrap_or_else(|| value.display())
    } else {
        value.display()
    }
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
            format_attribute_path(path),
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
        lines.push(Line::from(format!("  {}", format_replace_path(path))));
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

const fn source_side_label(side: SourceSide) -> &'static str {
    match side {
        SourceSide::Before => "before",
        SourceSide::After => "after",
    }
}

const fn pluralize(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 { singular } else { plural }
}
