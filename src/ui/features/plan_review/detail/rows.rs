use ratatui::text::{Line, Span};

use crate::app::attribution::SourceSide;
use crate::app::attribution::{AttributionEvidence, AttributionStatus, ResourceAttribution};
use crate::app::plan::{
    AttributeChangeKind, AttributeDiff, AttributeDiffs, AttributeValue, format_attribute_path,
    format_replace_path,
};
use crate::app::review::{
    AttributeGroup, DetailRow, PlanListContext, PlanListState, ReviewComparison,
    ReviewComparisonSource, ReviewDetailState,
};
use crate::ui::shell::context::{display_path, truncate_middle};
use crate::ui::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetailContent {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) selected_line: Option<usize>,
}

pub(super) fn detail_content(
    list: &PlanListState,
    detail: &ReviewDetailState,
    analysis_info_expanded: bool,
    width: u16,
    now: std::time::Instant,
) -> DetailContent {
    let item = list
        .selected_item()
        .expect("open detail should retain a selected resource");
    let mut lines = Vec::new();
    let mut selected_line = None;
    let repository_root = list.context().and_then(|context| context.repository_root());
    let execution_root = list.context().map(PlanListContext::root);

    lines.push(Line::from(vec![
        Span::styled(
            theme::action_symbol(item.kind()),
            theme::action_style(item.kind()),
        ),
        Span::raw(" "),
        Span::raw(item.address().to_owned()),
        Span::raw(format!(" (of {} total)", list.items().len())),
    ]));
    if !list.search().is_empty() {
        lines.push(Line::from(format!("Search: {}", list.search())));
    }
    lines.push(Line::default());
    append_attribution_summary(
        &mut lines,
        item.attribution(),
        list.comparison(),
        repository_root,
        execution_root,
        width,
    );
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
    append_evidence(
        &mut lines,
        item.attribution(),
        list.comparison(),
        repository_root,
        execution_root,
    );
    append_analysis_details(&mut lines, item.attribution());
    if analysis_info_expanded {
        append_analysis_info(&mut lines, list, repository_root, execution_root);
    }

    DetailContent {
        lines,
        selected_line,
    }
}

fn append_attribution_summary(
    lines: &mut Vec<Line<'static>>,
    attribution: &ResourceAttribution,
    comparison: &ReviewComparison,
    repository_root: Option<&std::path::Path>,
    execution_root: Option<&std::path::Path>,
    width: u16,
) {
    lines.push(Line::from(format!(
        "Git: {} ({} {})",
        attribution_label(attribution),
        attribution.evidence().len(),
        "evidence",
    )));

    if let Some(evidence) = attribution.evidence().first() {
        const PREFIX: &str = "  First evidence: ";
        let location = evidence_location(evidence, comparison, repository_root, execution_root);
        let available = usize::from(width).saturating_sub(PREFIX.len());
        lines.push(Line::from(format!(
            "{PREFIX}{}",
            truncate_middle(&location, available)
        )));
    }
    if attribution.status() == AttributionStatus::NoMatch {
        lines.push(Line::from(
            "No direct match within the analyzed scope; this does not establish safety.",
        ));
    }
    if !attribution.analysis().is_complete() {
        lines.push(Line::from(
            "Analysis incomplete; details follow after Git evidence.",
        ));
    }
}

fn append_evidence(
    lines: &mut Vec<Line<'static>>,
    attribution: &ResourceAttribution,
    comparison: &ReviewComparison,
    repository_root: Option<&std::path::Path>,
    execution_root: Option<&std::path::Path>,
) {
    lines.push(Line::default());
    lines.push(Line::from("Git evidence:"));
    if attribution.evidence().is_empty() {
        lines.push(Line::from("  None"));
    }
    for evidence in attribution.evidence() {
        lines.push(Line::from(detail_path(
            &evidence_location(evidence, comparison, repository_root, execution_root),
            "",
        )));
    }
}

fn append_analysis_details(lines: &mut Vec<Line<'static>>, attribution: &ResourceAttribution) {
    if attribution.analysis().is_complete() {
        return;
    }

    lines.push(Line::default());
    lines.push(Line::from("Analysis details:"));
    for issue in attribution.analysis().issues() {
        lines.push(Line::from(format!("  - {}", issue.message())));
    }
}

fn append_analysis_info(
    lines: &mut Vec<Line<'static>>,
    list: &PlanListState,
    repository_root: Option<&std::path::Path>,
    execution_root: Option<&std::path::Path>,
) {
    lines.push(Line::default());
    lines.push(Line::from("Analysis info (s hide):"));
    lines.push(Line::from(
        "Analysis scope (all analyzed files, not selected-resource evidence)",
    ));

    if let Some(context) = list.context() {
        lines.push(Line::from(format!(
            "Execution root: {}",
            context.root().display()
        )));
        lines.push(Line::from(format!(
            "Repository root: {}",
            context.repository_root().map_or_else(
                || "unavailable".to_owned(),
                |path| path.display().to_string(),
            )
        )));
        lines.push(Line::from(format!("Workspace: {}", context.workspace())));
        lines.push(Line::from(format!("Git branch: {}", context.git())));
    } else {
        lines.push(Line::from("Execution root: Target unavailable"));
        lines.push(Line::from("Repository root: unavailable"));
        lines.push(Line::from("Workspace: unavailable"));
        lines.push(Line::from("Git branch: unavailable"));
    }
    lines.push(Line::from(format!(
        "Comparison: {}",
        list.comparison().label()
    )));

    lines.push(Line::from("Analyzed files:"));
    if list.source_files().is_empty() {
        lines.push(Line::from("  No analyzed files"));
    }
    for source in list.source_files() {
        let suffix = if source.is_complete() {
            format!(
                " ({})",
                comparison_side_label(list.comparison(), source.side())
            )
        } else {
            format!(
                " ({}) [incomplete]",
                comparison_side_label(list.comparison(), source.side())
            )
        };
        lines.push(Line::from(detail_path(
            &display_path(source.path(), repository_root, execution_root),
            &suffix,
        )));
    }

    if !list.analysis_issues().is_empty() {
        lines.push(Line::from("Analysis issues:"));
        for issue in list.analysis_issues() {
            lines.push(Line::from(format!("  - {issue}")));
        }
    }
}

fn evidence_location(
    evidence: &AttributionEvidence,
    comparison: &ReviewComparison,
    repository_root: Option<&std::path::Path>,
    execution_root: Option<&std::path::Path>,
) -> String {
    let range = evidence.range();
    let line = if range.start_line() == range.end_line() {
        range.start_line().to_string()
    } else {
        format!("{}-{}", range.start_line(), range.end_line())
    };
    format!(
        "{}:{line} ({})",
        display_path(evidence.path(), repository_root, execution_root),
        comparison_side_label(comparison, evidence.side())
    )
}

fn detail_path(path: &str, suffix: &str) -> String {
    format!("  {path}{suffix}")
}

fn comparison_side_label(comparison: &ReviewComparison, side: SourceSide) -> String {
    match comparison.source_for(side) {
        ReviewComparisonSource::Head => "HEAD".to_owned(),
        ReviewComparisonSource::WorkingTree => "working tree".to_owned(),
        ReviewComparisonSource::MergeBase => comparison.compare_ref().map_or_else(
            || "merge-base".to_owned(),
            |name| format!("merge-base({name})"),
        ),
    }
}

const fn pluralize(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 { singular } else { plural }
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
    lines.push(Line::styled(
        format!(
            "    - {}",
            display_attribute_value(&attribute.before, reveal)
        ),
        theme::diff_style(attribute.kind, false),
    ));
    lines.push(Line::styled(
        format!(
            "    + {}",
            display_attribute_value(&attribute.after, reveal)
        ),
        theme::diff_style(attribute.kind, true),
    ));
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
    match attribution.status() {
        AttributionStatus::Direct => "direct",
        AttributionStatus::NoMatch => "no match",
    }
}

#[cfg(test)]
mod tests {
    use crate::app::attribution::{
        ResourceAddress, ResourceSourceLocation, SourceFileAnalysis, SourceLineChange, SourceRange,
    };
    use crate::app::plan::AttributePathSegment;
    use crate::app::review::{DetailAction, ReviewComparisonBasis, ReviewComparisonStatus};
    use crate::ui::test_support::buffer_text;

    use super::super::test_support::*;
    use super::super::*;
    use super::*;

    #[test]
    fn changed_values_use_red_and_green_while_unchanged_values_stay_dim() {
        use ratatui::style::{Color, Modifier};

        let state = state_for_change(expansion_change(), &[]);
        let attributes = state.detail.attributes();
        for attribute in &attributes.attributes {
            let mut lines = Vec::new();
            append_attribute(&mut lines, attribute, false, false);
            if attribute.kind == AttributeChangeKind::Changed {
                assert_eq!(lines[1].style.fg, Some(Color::Rgb(0xbf, 0x61, 0x6a)));
                assert_eq!(lines[2].style.fg, Some(Color::Rgb(0xa3, 0xbe, 0x8c)));
            } else {
                assert!(lines[1].style.add_modifier.contains(Modifier::DIM));
                assert!(lines[2].style.add_modifier.contains(Modifier::DIM));
            }
        }
    }

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

    #[test]
    fn summarizes_match_status_and_expands_analysis_info_with_side_names() {
        let empty = state_for_change_with_sources(change(), &[], Vec::new());
        let empty_text = buffer_text(&render(&empty, 100, 60));
        assert!(
            empty_text.contains(
                "No direct match within the analyzed scope; this does not establish safety."
            ),
            "{empty_text}"
        );
        assert!(empty_text.contains("Git evidence:"), "{empty_text}");
        assert!(empty_text.contains("  None"), "{empty_text}");
        assert!(!empty_text.contains("Analyzed files:"), "{empty_text}");

        let long_path =
            "modules/production/services/networking/terraform/main/region/ap-northeast-1/main.tf";
        let mut state = state_for_change_with_sources(
            change(),
            &[],
            vec![
                source_file(long_path, SourceSide::Before, Vec::new()),
                incomplete_source_file(long_path),
            ],
        );
        let collapsed = buffer_text(&render(&state, 120, 60));
        assert!(
            collapsed.contains("Analysis incomplete; details follow after Git evidence."),
            "{collapsed}"
        );
        assert!(!collapsed.contains("(before)"), "{collapsed}");

        state.toggle_analysis_info(120, 60, Instant::now());
        let expanded = buffer_text(&render(&state, 120, 60));
        assert!(expanded.contains("Analysis info (s hide):"), "{expanded}");
        assert!(
            expanded
                .contains("Analysis scope (all analyzed files, not selected-resource evidence)"),
            "{expanded}"
        );
        let content = detail_content(&state.list, &state.detail, true, 120, Instant::now());
        assert!(
            content
                .lines
                .iter()
                .any(|line| { line.to_string() == format!("  {long_path} (HEAD)") })
        );
        assert!(content.lines.iter().any(|line| {
            line.to_string() == format!("  {long_path} (working tree) [incomplete]")
        }));
    }

    #[test]
    fn analysis_info_opens_without_context_or_analyzed_files() {
        let state = state_without_context();
        let content = detail_content(&state.list, &state.detail, true, 120, Instant::now());
        let text = content
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            text.contains("Execution root: Target unavailable"),
            "{text}"
        );
        assert!(text.contains("Repository root: unavailable"), "{text}");
        assert!(text.contains("No analyzed files"), "{text}");
    }

    #[test]
    fn comparison_side_labels_keep_the_requested_ref_name() {
        let comparison = ReviewComparison::new(
            ReviewComparisonBasis::HeadVsMergeBase,
            Some("main".to_owned()),
            ReviewComparisonStatus::Complete,
        );

        assert_eq!(
            comparison_side_label(&comparison, SourceSide::Before),
            "merge-base(main)"
        );
        assert_eq!(
            comparison_side_label(&comparison, SourceSide::After),
            "HEAD"
        );
    }

    #[test]
    fn puts_analysis_info_after_diff_and_replacement_reason() {
        let mut state = state_for_change(change(), &[]);
        let collapsed = detail_content(&state.list, &state.detail, false, 120, Instant::now());
        state.toggle_analysis_info(120, 60, Instant::now());
        let expanded = detail_content(&state.list, &state.detail, true, 120, Instant::now());

        let collapsed_diff = collapsed
            .lines
            .iter()
            .position(|line| line.to_string() == "Diff:")
            .expect("diff heading should be present");
        let expanded_diff = expanded
            .lines
            .iter()
            .position(|line| line.to_string() == "Diff:")
            .expect("diff heading should be present");
        assert_eq!(collapsed_diff, expanded_diff);

        let replacement = expanded
            .lines
            .iter()
            .position(|line| {
                line.to_string() == "Replacement reason: replace_because_cannot_update"
            })
            .expect("replacement reason should be present");
        let evidence = expanded
            .lines
            .iter()
            .position(|line| line.to_string() == "Git evidence:")
            .expect("evidence heading should be present");
        let analysis = expanded
            .lines
            .iter()
            .position(|line| line.to_string() == "Analysis info (s hide):")
            .expect("analysis info heading should be present");
        assert!(evidence > replacement);
        assert!(analysis > evidence);
    }

    #[test]
    fn full_path_keeps_filename_and_location_suffix() {
        let line = detail_path(
            "modules/production/services/networking/main.tf",
            ":42-46 (after)",
        );

        assert!(line.contains("modules/production/services/networking/main.tf"));
        assert!(line.ends_with("main.tf:42-46 (after)"));
    }

    #[test]
    fn first_evidence_summary_stays_on_one_line_at_minimum_width() {
        let long_path =
            "modules/production/services/networking/terraform/main/region/ap-northeast-1/main.tf";
        let source = SourceFileAnalysis::new(
            long_path.into(),
            SourceSide::After,
            vec![ResourceSourceLocation::new(
                ResourceAddress::new("aws_instance", "api"),
                long_path.into(),
                SourceSide::After,
                SourceRange::new(42, 46),
            )],
            Vec::new(),
        );
        let changed_line =
            SourceLineChange::new(long_path, SourceSide::After, SourceRange::new(42, 46));
        let state = state_for_change_with_sources(change(), &[changed_line], vec![source]);
        let text = buffer_text(&render(&state, 48, 30));
        let lines = text.lines().collect::<Vec<_>>();
        let first = lines
            .iter()
            .position(|line| line.contains("First evidence:"))
            .unwrap_or_else(|| panic!("first evidence summary should be rendered: {text}"));

        assert!(lines[first].contains("..."), "{text}");
        assert!(!lines[first + 1].contains("First evidence:"), "{text}");
        assert!(lines[first + 2].contains("Diff:"), "{text}");
    }
}
