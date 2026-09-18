use ratatui::text::{Line, Span};

use crate::app::attribution::{AttributionStatus, ResourceAttribution};
use crate::app::attribution::{SourceFileAnalysis, SourceSide};
use crate::app::plan::{
    AttributeChangeKind, AttributeDiff, AttributeDiffs, AttributeValue, format_attribute_path,
    format_replace_path,
};
use crate::app::review::{AttributeGroup, DetailRow, PlanListState, ReviewDetailState};
use crate::ui::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetailContent {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) selected_line: Option<usize>,
}

pub(super) fn detail_content(
    list: &PlanListState,
    detail: &ReviewDetailState,
    now: std::time::Instant,
) -> DetailContent {
    let item = list
        .selected_item()
        .expect("open detail should retain a selected resource");
    let mut lines = Vec::new();
    let mut selected_line = None;

    lines.push(Line::from(vec![
        Span::styled(
            theme::action_symbol(item.kind()),
            theme::action_style(item.kind()),
        ),
        Span::raw(" "),
        Span::raw(item.address().to_owned()),
    ]));
    lines.push(Line::default());
    append_attribution(&mut lines, item.attribution(), list.source_files());
    lines.push(Line::default());
    lines.push(Line::from("Diff:"));

    let attributes = detail.attributes();
    let has_changed_attributes = attributes
        .attributes
        .iter()
        .any(|attribute| attribute.kind == AttributeChangeKind::Changed);
    if !has_changed_attributes {
        lines.push(Line::from("  No changed attributes."));
    }

    let rows = detail.rows();
    for (row_index, row) in rows.iter().enumerate() {
        if row_index == detail.selected() {
            selected_line = Some(lines.len());
        }
        match row {
            DetailRow::Attribute(attribute_index) => append_attribute(
                &mut lines,
                &attributes.attributes[*attribute_index],
                row_index == detail.selected(),
                detail.reveals_attribute(&attributes.attributes[*attribute_index], now),
            ),
            DetailRow::Group { group, count } => append_group(
                &mut lines,
                group,
                *count,
                detail.expanded_groups().contains(group),
                row_index == detail.selected(),
            ),
        }
    }

    append_replacement(&mut lines, attributes);

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

#[cfg(test)]
mod tests {
    use crate::app::plan::AttributePathSegment;
    use crate::app::review::DetailAction;
    use crate::ui::test_support::buffer_text;

    use super::super::test_support::*;
    use super::super::*;
    use super::*;

    #[test]
    fn expands_unchanged_group_and_keeps_selection_on_group_row() {
        let mut state = state_for_change(expansion_change(), &[]);
        let unchanged = AttributeGroup::Unchanged;
        select_group(&mut state, &unchanged);
        let group_index = state.detail.selected();

        state
            .detail
            .apply(DetailAction::ToggleExpansion, Instant::now());

        assert_eq!(state.detail.selected(), group_index);
        let text = buffer_text(&render(&state, 100, 60));
        assert!(
            text.contains("> [v] 2 unchanged attributes  [Enter collapse]"),
            "{text}"
        );
        assert!(text.contains("root_unchanged"), "{text}");

        state.detail.apply(DetailAction::SelectNext, Instant::now());
        assert!(matches!(
            state.detail.rows().get(state.detail.selected()),
            Some(DetailRow::Group {
                group: AttributeGroup::Nested {
                    kind: AttributeChangeKind::Unchanged,
                    ..
                },
                ..
            })
        ));

        state
            .detail
            .apply(DetailAction::SelectPrevious, Instant::now());
        state
            .detail
            .apply(DetailAction::ToggleExpansion, Instant::now());
        assert_eq!(state.detail.selected(), group_index);
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

        state
            .detail
            .apply(DetailAction::ToggleExpansion, Instant::now());
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
}
