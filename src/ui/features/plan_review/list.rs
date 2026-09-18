use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use crate::app::attribution::AttributionStatus;
use crate::app::copy::{CopyNotice, CopyTarget};
use crate::app::plan::ResourceChangeKind;
use crate::app::review::{PlanListFilter, PlanListItem, PlanListState, ReviewDiagnosticsState};
use crate::ui::primitives::atoms::separator;
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{footer, header, layout as shell_layout};
use crate::ui::theme;

use super::text::{display_width, truncate_end};

const MIN_HEIGHT: u16 = 12;
const MIN_CONTENT_HEIGHT: u16 = MIN_HEIGHT - 2;
const MIN_WIDTH: u16 = 48;
const ANALYSIS_PREFIX: &str = "Analysis incomplete: ";

pub(crate) fn render_plan_list_with_diagnostics(
    frame: &mut Frame<'_>,
    state: &PlanListState,
    diagnostics: &ReviewDiagnosticsState,
    copy_notice: Option<CopyNotice>,
    list_state: &mut ListState,
) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    let footer_lines = footer_lines(state, diagnostics, area.width);
    let required_footer_lines = required_footer_lines(state, area.width);
    let shell = shell_layout::layout(area, footer_lines, required_footer_lines, 6);
    if shell.content_inner().height == 0 {
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    header::render_review(frame, shell.header(), state.context(), state.comparison());
    let content_area = shell_layout::render_content_block(frame, shell.content(), "Plan");
    let has_search = state.searching() || !state.search().is_empty();
    let compact_layout = has_search && content_area.height <= MIN_CONTENT_HEIGHT;
    let notices = notice_lines(
        state,
        diagnostics,
        content_area.width as usize,
        compact_layout,
    );
    let legend = attention_legend(state);
    let summary = summary_lines(state, content_area.width as usize);
    let summary_height = u16::try_from(summary.len()).unwrap_or(u16::MAX);
    let search_height = u16::from(has_search);
    let legend_height = u16::from(legend.is_some());
    let notice_height = u16::try_from(notices.len()).unwrap_or(u16::MAX);
    let separator_height =
        u16::from(!compact_layout && notice_height < 2 && content_area.width > MIN_WIDTH);
    let copy_notice_height = u16::from(copy_notice.is_some());
    let chunks = list_layout(
        content_area,
        summary_height,
        search_height,
        legend_height,
        separator_height,
        notice_height,
        copy_notice_height,
    );

    if chunks[6].height == 0 {
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(summary).style(theme::body_style()),
        chunks[0],
    );
    if has_search {
        let search_line = if state.searching() {
            format!("/ {}_", state.search())
        } else {
            format!("Search: {}", state.search())
        };
        frame.render_widget(
            Paragraph::new(search_line).style(theme::body_style()),
            chunks[1],
        );
    }
    if chunks[2].height > 0 {
        frame.render_widget(separator::render(chunks[2].width), chunks[2]);
    }

    if let Some(legend) = legend {
        frame.render_widget(Paragraph::new(legend), chunks[3]);
    }

    if !notices.is_empty() {
        frame.render_widget(
            Paragraph::new(notices).style(theme::body_style()),
            chunks[4],
        );
    }

    if let Some(notice) = copy_notice {
        frame.render_widget(
            Paragraph::new(notice.message()).style(theme::body_style()),
            chunks[5],
        );
    }
    render_rows(frame, state, chunks[6], list_state);
    footer::render(frame, shell.footer(), shell.footer_lines().to_owned());
}

fn list_layout(
    content_area: Rect,
    summary_height: u16,
    search_height: u16,
    legend_height: u16,
    separator_height: u16,
    notice_height: u16,
    copy_notice_height: u16,
) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(summary_height),
            Constraint::Length(search_height),
            Constraint::Length(separator_height),
            Constraint::Length(legend_height),
            Constraint::Length(notice_height),
            Constraint::Length(copy_notice_height),
            Constraint::Min(1),
        ])
        .split(content_area)
        .to_vec()
}

fn notice_lines(
    state: &PlanListState,
    diagnostics: &ReviewDiagnosticsState,
    width: usize,
    compact: bool,
) -> Vec<Line<'static>> {
    let unsupported = state.unsupported_summary();
    let analysis = state.analysis_issues().first().map(|first| {
        let suffix = match state.analysis_issues().len() {
            0 | 1 => String::new(),
            count => format!(" (+{} more)", count - 1),
        };
        let issue_width = width
            .saturating_sub(display_width(ANALYSIS_PREFIX))
            .saturating_sub(display_width(suffix.as_str()));
        format!(
            "{ANALYSIS_PREFIX}{}{}",
            truncate_end(first, issue_width),
            suffix
        )
    });

    if compact {
        let message = match (unsupported.as_deref(), analysis.as_deref()) {
            (Some(summary), Some(_)) => format!(
                "Unshown: {}  Analysis incomplete",
                summary.strip_prefix("Unshown changes: ").unwrap_or(summary)
            ),
            (Some(summary), None) => summary.to_owned(),
            (None, Some(analysis)) => analysis.to_owned(),
            (None, None) => String::new(),
        };
        let mut lines = if message.is_empty() {
            Vec::new()
        } else {
            vec![Line::from(truncate_end(&message, width))]
        };
        if diagnostics.count() > 0 {
            lines.push(diagnostics_line(diagnostics, state.searching()));
        }
        return lines;
    }

    let mut lines = match (unsupported, analysis) {
        (Some(summary), Some(analysis)) => vec![
            Line::from(truncate_end(&summary, width)),
            Line::from(analysis),
        ],
        (Some(summary), None) => vec![Line::from(truncate_end(&summary, width))],
        (None, Some(analysis)) => vec![Line::from(analysis)],
        (None, None) => Vec::new(),
    };
    if diagnostics.count() > 0 {
        lines.push(diagnostics_line(diagnostics, state.searching()));
    }
    lines
}

fn diagnostics_line(diagnostics: &ReviewDiagnosticsState, searching: bool) -> Line<'static> {
    let suffix = if searching { "" } else { " (w)" };
    Line::from(Span::styled(
        format!("Diagnostics: {}{suffix}", diagnostics.count()),
        theme::warning_style(),
    ))
}

fn render_rows(
    frame: &mut Frame<'_>,
    state: &PlanListState,
    area: Rect,
    list_state: &mut ListState,
) {
    if state.visible_count() == 0 {
        *list_state.selected_mut() = None;
        *list_state.offset_mut() = 0;
        let message = if state.items().is_empty() {
            "No resource changes.".to_owned()
        } else {
            "No resources match the current filter/search.".to_owned()
        };
        frame.render_widget(Paragraph::new(message), area);
        return;
    }

    let width = area.width.saturating_sub(2) as usize;
    let wide_actions = frame.area().width >= 96;
    let action_column_width: usize = if wide_actions { 11 } else { 8 };
    let resource_width = width.saturating_sub(action_column_width + 5).max(1);
    let heading = if area.height >= 2 {
        format!(
            "     ACTION{}RESOURCE",
            " ".repeat(action_column_width - "ACTION".len())
        )
    } else {
        String::new()
    };
    let header = Line::from(Span::styled(
        heading,
        Style::default().add_modifier(Modifier::BOLD),
    ));
    let items = state
        .visible_items()
        .map(|item| list_item(item, resource_width, wide_actions))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(Block::new().title(header))
        .style(theme::body_style())
        .highlight_symbol("> ")
        .highlight_style(theme::selection_style());
    *list_state.selected_mut() = state.selected();
    frame.render_stateful_widget(list, area, list_state);
}

fn list_item(item: &PlanListItem, resource_width: usize, wide_actions: bool) -> ListItem<'static> {
    let marker = attention_marker(item);
    let action_style = theme::action_style(item.kind());
    let review_style = theme::review_style(!marker.trim().is_empty());
    let address = truncate_end(item.address(), resource_width);
    let action = action_label(item.kind(), wide_actions);
    let action_column_width: usize = if wide_actions { 11 } else { 8 };
    let action_padding = " ".repeat(action_column_width.saturating_sub(display_width(&action)));

    ListItem::new(Line::from(vec![
        Span::styled(marker.to_owned(), review_style),
        Span::raw(" "),
        Span::styled(action, action_style),
        Span::raw(action_padding),
        Span::raw(address),
    ]))
}

fn action_label(kind: ResourceChangeKind, wide: bool) -> String {
    if wide {
        format!("{} {}", theme::action_symbol(kind), kind_label(kind))
    } else {
        theme::action_symbol(kind).to_owned()
    }
}

const fn kind_label(kind: ResourceChangeKind) -> &'static str {
    match kind {
        ResourceChangeKind::Create => "create",
        ResourceChangeKind::Update => "update",
        ResourceChangeKind::Replace => "replace",
        ResourceChangeKind::Delete => "delete",
    }
}

const fn attention_marker(item: &PlanListItem) -> &'static str {
    match (
        matches!(item.attribution().status(), AttributionStatus::NoMatch),
        !item.attribution().analysis().is_complete(),
    ) {
        (true, true) => "!?",
        (true, false) => "! ",
        (false, true) => "? ",
        (false, false) => "  ",
    }
}

fn attention_legend(state: &PlanListState) -> Option<Line<'static>> {
    let has_no_match = state
        .items()
        .iter()
        .any(|item| matches!(item.attribution().status(), AttributionStatus::NoMatch));
    let has_incomplete = state
        .items()
        .iter()
        .any(|item| !item.attribution().analysis().is_complete());
    if !has_no_match && !has_incomplete {
        return None;
    }

    let mut spans = Vec::new();
    if has_no_match {
        spans.push(Span::styled("!", theme::review_style(true)));
        spans.push(Span::styled(" no direct match", theme::secondary_style()));
    }
    if has_incomplete {
        if has_no_match {
            spans.push(Span::styled(" | ", theme::secondary_style()));
        }
        spans.push(Span::styled("?", theme::review_style(true)));
        spans.push(Span::styled(
            " analysis incomplete",
            theme::secondary_style(),
        ));
    }
    Some(Line::from(spans))
}

fn summary_lines(state: &PlanListState, width: usize) -> Vec<Line<'static>> {
    let summary = state.summary();
    let review_text = format!(
        "Needs review: {}/{}",
        state.needs_review_count(),
        state.items().len()
    );
    let review_count = Span::styled(review_text, theme::body_style());

    let action_line = Line::from(vec![
        Span::styled(
            format!("+{} create", summary.creates),
            theme::action_style(ResourceChangeKind::Create),
        ),
        Span::raw("  "),
        Span::styled(
            format!("~{} update", summary.updates),
            theme::action_style(ResourceChangeKind::Update),
        ),
        Span::raw("  "),
        Span::styled(
            format!("R{} replace", summary.replaces),
            theme::action_style(ResourceChangeKind::Replace),
        ),
        Span::raw("  "),
        Span::styled(
            format!("-{} delete", summary.deletes),
            theme::action_style(ResourceChangeKind::Delete),
        ),
    ]);
    if state.filter() == PlanListFilter::All && !has_search(state) {
        return vec![
            action_line,
            Line::from(vec![
                review_count,
                Span::raw(format!("   Filter: {}", filter_label(state.filter()))),
            ]),
        ];
    }

    let filter_label = filter_label(state.filter());
    if has_search(state) && width < 60 {
        return vec![
            action_line,
            Line::from(vec![
                review_count,
                Span::raw(format!(
                    "  Showing: {}/{}",
                    state.visible_count(),
                    state.items().len()
                )),
            ]),
        ];
    }

    let filter_line = Line::from(vec![
        review_count.clone(),
        Span::raw(format!("  Filter: {filter_label}")),
    ]);
    let showing_line = Line::from(format!(
        "Showing: {}/{}",
        state.visible_count(),
        state.items().len()
    ));
    let combined = Line::from(vec![
        review_count,
        Span::raw(format!("  Filter: {filter_label}")),
        Span::raw(format!(
            "  Showing: {}/{}",
            state.visible_count(),
            state.items().len()
        )),
    ]);
    if combined.width() <= width {
        vec![action_line, combined]
    } else {
        vec![action_line, filter_line, showing_line]
    }
}

fn has_search(state: &PlanListState) -> bool {
    state.searching() || !state.search().is_empty()
}

const fn filter_label(filter: PlanListFilter) -> &'static str {
    match filter {
        PlanListFilter::All => "All (f)",
        PlanListFilter::NeedsReview => "Needs review (f)",
    }
}

fn footer_lines(
    state: &PlanListState,
    diagnostics: &ReviewDiagnosticsState,
    width: u16,
) -> Vec<Line<'static>> {
    if state.searching() {
        return footer::layout(
            vec![
                footer::hint(&["Enter"], "confirm"),
                footer::hint(&["Esc"], "cancel"),
                footer::hint(&["Ctrl-C"], "quit"),
                Line::from("Type to search"),
            ],
            width,
        );
    }

    let has_visible_items = state.visible_count() > 0;
    let mut items = vec![footer::hint(&["q"], "quit")];
    if has_visible_items {
        items.push(footer::hint(&["Enter"], "details"));
        items.push(footer::hint(&["j", "k", "↑", "↓"], "select"));
    }
    if has_visible_items || (!state.items().is_empty() && state.filter() != PlanListFilter::All) {
        items.push(footer::hint(&["f"], "change filter"));
    }
    if has_visible_items || (!state.items().is_empty() && has_search(state)) {
        items.push(footer::hint(&["/"], "edit search"));
    }
    if diagnostics.count() > 0 {
        items.push(footer::hint(&["w"], "diagnostics"));
    }
    if has_visible_items && state.can_copy(CopyTarget::Resource) {
        items.push(footer::hint(&["y"], "copy resource"));
    }
    if state.can_copy(CopyTarget::Plan) {
        items.push(footer::hint(&["Y"], "copy plan"));
    }
    footer::layout(items, width)
}

fn required_footer_lines(state: &PlanListState, width: u16) -> Vec<Line<'static>> {
    let items = if state.searching() {
        vec![
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "cancel"),
            footer::hint(&["Ctrl-C"], "quit"),
        ]
    } else {
        vec![footer::hint(&["q"], "quit")]
    };
    footer::layout(items, width)
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use crate::app::attribution::{
        ResourceAddress, ResourceSourceLocation, SourceFileAnalysis,
        SourceLineChange as AttributionSourceLineChange, SourceRange, SourceSide,
        attribute_changes, mark_analysis_incomplete,
    };
    use crate::app::plan::{
        Plan, PlanAction, PlanSummary, PlanValue, ResourceChange, ResourceMode, UnsupportedChange,
        UnsupportedChangeKind, UnsupportedChangeScope,
    };
    use crate::app::review::ReviewDetailState;
    use std::path::PathBuf;

    use ratatui::buffer::Buffer;

    use super::*;
    use crate::app::attribution::AnalysisIssue;
    use crate::app::execution::{Diagnostic, DiagnosticSeverity, DiagnosticSource};
    use crate::app::review::{
        PlanListAction, PlanReview, ReviewComparison, ReviewComparisonBasis, ReviewComparisonStatus,
    };
    use crate::ui::test_support::{
        REALISTIC_COMMON_SOURCE, REALISTIC_DEVELOPMENT_SOURCE, REALISTIC_EXECUTION_ROOT,
        REALISTIC_PRODUCTION_SOURCE, REALISTIC_REPOSITORY_ROOT, assert_shell_frame_and_footer,
        buffer_text, render_to_buffer as render_test_buffer,
    };

    include!("tests/render_snapshots.rs");

    fn render_plan_list_with_state(
        frame: &mut Frame<'_>,
        state: &PlanListState,
        list_state: &mut ListState,
    ) {
        render_plan_list_with_diagnostics(
            frame,
            state,
            &ReviewDiagnosticsState::default(),
            None,
            list_state,
        );
    }

    fn synthetic_state() -> PlanListState {
        let mut changes = synthetic_changes();
        changes[2].mode = ResourceMode::Data;
        let source_files = synthetic_source_files();
        let changed_lines = synthetic_changed_lines();
        let attributions = attribute_changes(&changes, &source_files, &changed_lines);
        PlanListState::from_plan(
            Plan {
                changes,
                summary: PlanSummary {
                    creates: 1,
                    updates: 1,
                    replaces: 1,
                    deletes: 1,
                },
                unsupported_changes: vec![UnsupportedChange {
                    scope: UnsupportedChangeScope::Output,
                    address: "output.synthetic".to_owned(),
                    actions: vec![PlanAction::Update],
                    kind: UnsupportedChangeKind::Output,
                    reason: None,
                    action_type: None,
                }],
            },
            attributions,
            ReviewComparison::working_tree(),
        )
        .expect("synthetic changes and attributions should align")
    }

    fn synthetic_changes() -> Vec<ResourceChange> {
        vec![
            synthetic_change(
                "aws_instance.api",
                ResourceChangeKind::Update,
                PlanAction::Update,
            ),
            synthetic_change(
                "aws_s3_bucket.logs_with_a_very_long_resource_address_that_needs_truncation_for_narrow_terminal",
                ResourceChangeKind::Create,
                PlanAction::Create,
            ),
            synthetic_change(
                "aws_instance.worker",
                ResourceChangeKind::Replace,
                PlanAction::Delete,
            ),
            synthetic_change(
                "aws_security_group.old",
                ResourceChangeKind::Delete,
                PlanAction::Delete,
            ),
        ]
    }

    fn synthetic_source_files() -> Vec<SourceFileAnalysis> {
        vec![
            SourceFileAnalysis::new(
                "main.tf".into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new("aws_instance", "api"),
                    "main.tf".into(),
                    SourceSide::After,
                    SourceRange::new(42, 46),
                )],
                Vec::new(),
            ),
            SourceFileAnalysis::new(
                "storage.tf".into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new(
                        "aws_s3_bucket",
                        "logs_with_a_very_long_resource_address_that_needs_truncation_for_narrow_terminal",
                    ),
                    "storage.tf".into(),
                    SourceSide::After,
                    SourceRange::new(8, 10),
                )],
                Vec::new(),
            ),
            SourceFileAnalysis::new(
                "worker.tf".into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new("aws_instance", "worker"),
                    "worker.tf".into(),
                    SourceSide::After,
                    SourceRange::new(12, 18),
                )],
                Vec::new(),
            ),
            SourceFileAnalysis::new(
                "old.tf".into(),
                SourceSide::Before,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new("aws_security_group", "old"),
                    "old.tf".into(),
                    SourceSide::Before,
                    SourceRange::new(20, 24),
                )],
                Vec::new(),
            ),
        ]
    }

    fn synthetic_changed_lines() -> Vec<AttributionSourceLineChange> {
        vec![
            AttributionSourceLineChange::new(
                "main.tf",
                SourceSide::After,
                SourceRange::new(42, 43),
            ),
            AttributionSourceLineChange::new(
                "storage.tf",
                SourceSide::After,
                SourceRange::new(8, 8),
            ),
            AttributionSourceLineChange::new(
                "worker.tf",
                SourceSide::After,
                SourceRange::new(14, 14),
            ),
        ]
    }

    fn synthetic_change(
        address: &str,
        kind: ResourceChangeKind,
        action: PlanAction,
    ) -> ResourceChange {
        ResourceChange {
            address: address.to_owned(),
            mode: ResourceMode::Managed,
            actions: vec![action],
            kind,
            before: Some(PlanValue::Null),
            after: Some(PlanValue::Null),
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
        }
    }

    fn render_to_buffer(state: &PlanListState, width: u16, height: u16) -> Buffer {
        let mut list_state = ListState::default();
        render_to_buffer_with_state(state, width, height, &mut list_state)
    }

    #[test]
    fn common_shell_geometry_survives_supported_sizes() {
        let state = empty_state();
        for (width, height) in [(120, 40), (80, 24), (48, 12)] {
            let layout = shell_layout::layout(
                Rect::new(0, 0, width, height),
                footer_lines(&state, &ReviewDiagnosticsState::default(), width),
                required_footer_lines(&state, width),
                6,
            );
            let buffer = render_to_buffer(&state, width, height);
            assert_shell_frame_and_footer(&buffer, layout.content(), layout.footer(), "q quit");
        }

        let small = buffer_text(&render_to_buffer(&state, 47, 11));
        assert!(small.contains("Terminal too small"), "{small}");
    }

    fn render_to_buffer_with_state(
        state: &PlanListState,
        width: u16,
        height: u16,
        list_state: &mut ListState,
    ) -> Buffer {
        render_test_buffer((width, height), |frame| {
            render_plan_list_with_state(frame, state, list_state);
        })
    }

    fn render_to_buffer_with_diagnostics(
        state: &PlanListState,
        diagnostics: &ReviewDiagnosticsState,
        width: u16,
        height: u16,
    ) -> Buffer {
        let mut list_state = ListState::default();
        render_test_buffer((width, height), |frame| {
            render_plan_list_with_diagnostics(frame, state, diagnostics, None, &mut list_state);
        })
    }

    fn render_to_buffer_with_notice(
        state: &PlanListState,
        copy_notice: Option<CopyNotice>,
        width: u16,
        height: u16,
    ) -> Buffer {
        let mut list_state = ListState::default();
        render_test_buffer((width, height), |frame| {
            render_plan_list_with_diagnostics(
                frame,
                state,
                &ReviewDiagnosticsState::default(),
                copy_notice,
                &mut list_state,
            );
        })
    }

    fn empty_state() -> PlanListState {
        PlanListState::from_plan(
            Plan {
                changes: Vec::new(),
                summary: PlanSummary::default(),
                unsupported_changes: Vec::new(),
            },
            Vec::new(),
            ReviewComparison::working_tree(),
        )
        .expect("empty plan should build a list")
    }

    fn warning_diagnostics() -> ReviewDiagnosticsState {
        ReviewDiagnosticsState::new(vec![Diagnostic {
            severity: DiagnosticSeverity::Warning,
            summary: "warning".to_owned(),
            detail: None,
            position: None,
            source: DiagnosticSource::Terraform,
        }])
    }

    fn connected_state() -> PlanListState {
        let change = synthetic_change(
            "aws_instance.api",
            ResourceChangeKind::Update,
            PlanAction::Update,
        );
        let attribution = attribute_changes(std::slice::from_ref(&change), &[], &[])
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
                unsupported_changes: vec![UnsupportedChange {
                    scope: UnsupportedChangeScope::Output,
                    address: "output.value".to_owned(),
                    actions: vec![PlanAction::Update],
                    kind: UnsupportedChangeKind::Output,
                    reason: None,
                    action_type: None,
                }],
            },
            Vec::new(),
            vec![attribution],
            ReviewComparison::new(
                ReviewComparisonBasis::WorkingTreeVsHead,
                None,
                ReviewComparisonStatus::Complete,
            ),
            vec![
                AnalysisIssue::git("first analysis issue"),
                AnalysisIssue::git("second analysis issue"),
                AnalysisIssue::git("third analysis issue"),
            ],
        )
        .with_git("feature/review".to_owned());
        PlanListState::from_review(review).expect("review data should build a list")
    }

    fn realistic_state() -> PlanListState {
        let change = synthetic_change(
            "terraform_data.api",
            ResourceChangeKind::Update,
            PlanAction::Update,
        );
        let source_files = vec![
            SourceFileAnalysis::new(
                REALISTIC_DEVELOPMENT_SOURCE.into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new("terraform_data", "api"),
                    REALISTIC_DEVELOPMENT_SOURCE.into(),
                    SourceSide::After,
                    SourceRange::new(12, 16),
                )],
                Vec::new(),
            ),
            SourceFileAnalysis::new(
                REALISTIC_PRODUCTION_SOURCE.into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new("terraform_data", "api"),
                    REALISTIC_PRODUCTION_SOURCE.into(),
                    SourceSide::After,
                    SourceRange::new(12, 16),
                )],
                Vec::new(),
            ),
            SourceFileAnalysis::new(
                REALISTIC_COMMON_SOURCE.into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new("terraform_data", "api"),
                    REALISTIC_COMMON_SOURCE.into(),
                    SourceSide::After,
                    SourceRange::new(4, 8),
                )],
                Vec::new(),
            ),
        ];
        let attributions = attribute_changes(
            std::slice::from_ref(&change),
            &source_files,
            &[AttributionSourceLineChange::new(
                REALISTIC_DEVELOPMENT_SOURCE,
                SourceSide::After,
                SourceRange::new(12, 13),
            )],
        );
        let review = PlanReview::new(
            PathBuf::from(REALISTIC_EXECUTION_ROOT),
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
            attributions,
            ReviewComparison::working_tree(),
            Vec::new(),
        )
        .with_git("feature/ui07".to_owned())
        .with_repository_root(Some(PathBuf::from(REALISTIC_REPOSITORY_ROOT)));
        PlanListState::from_review(review).expect("realistic review data should build a list")
    }

    #[test]
    fn renders_summary_selection_and_attention_legend() {
        let state = synthetic_state();
        let buffer = render_to_buffer(&state, 120, 20);
        let text = buffer_text(&buffer);

        assert!(text.contains("Git: working tree vs HEAD"));
        assert!(text.contains("+1 create  ~1 update  R1 replace  -1 delete"));
        assert!(text.contains("Needs review: 2/4"), "{text}");
        assert!(text.contains("! no direct match | ? analysis incomplete"));
        assert!(text.contains("!? R replace"));
        assert!(!text.contains("direct:"));
        assert!(!text.contains("main.tf:42-46"));
        assert!(text.contains("q quit"), "{text}");
        assert!(text.contains("j/k/↑/↓ select"), "{text}");
        assert!(text.contains("/ edit search"), "{text}");
        assert!(
            text.contains("aws_s3_bucket.logs_with_a_very_long_resource_address"),
            "{text}"
        );

        let delete_cell = buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == "-")
            .expect("delete symbol should be rendered");
        assert_eq!(delete_cell.fg, Color::Rgb(0xbf, 0x61, 0x6a));

        let selected_cell = buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == ">")
            .expect("selected row should be rendered");
        assert_eq!(selected_cell.bg, Color::Rgb(0x30, 0x32, 0x3b));
        assert!(!selected_cell.modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn attention_markers_stay_separate_from_actions_and_resources() {
        let state = synthetic_state();
        let text = buffer_text(&render_to_buffer(&state, 165, 30));
        let header = text.lines().find(|line| line.contains("ACTION")).unwrap();
        let resource_column = header.find("RESOURCE").unwrap();
        assert!(!header.contains("GIT"));

        for (address, marker) in [
            ("aws_instance.api", "  ~ update"),
            ("aws_instance.worker", "!? R replace"),
            ("aws_security_group.old", "!  - delete"),
        ] {
            let row = text.lines().find(|line| line.contains(address)).unwrap();
            assert_eq!(row.find(address), Some(resource_column), "{row}");
            assert!(row.contains(marker), "{row}");
        }
    }

    #[test]
    fn action_column_adds_change_names_at_the_96_cell_boundary() {
        let state = synthetic_state();
        let wide = buffer_text(&render_to_buffer(&state, 96, 20));
        let narrow = buffer_text(&render_to_buffer(&state, 95, 20));

        for label in ["+ create", "~ update", "R replace", "- delete"] {
            assert!(wide.contains(label), "wide list is missing {label}: {wide}");
        }
        assert!(
            !narrow.contains("~ update"),
            "narrow list used a wide action: {narrow}"
        );
    }

    #[test]
    fn list_hides_evidence_details_and_keeps_resource_rows_single_line() {
        let state = multiple_evidence_state();
        let text = buffer_text(&render_to_buffer(&state, 120, 16));

        assert!(text.lines().any(|line| line.contains("aws_instance.api")));
        assert!(!text.contains("direct:"), "{text}");
        assert!(!text.contains("[2 evidence]"), "{text}");
        assert!(!text.contains("/repo/environments/prod"), "{text}");
    }

    #[test]
    fn narrow_list_keeps_attention_marker_and_resource_on_one_line() {
        let state = long_evidence_incomplete_state();
        let text = buffer_text(&render_to_buffer(&state, 96, 20));
        let single_line = text.replace('\n', " ");

        assert!(single_line.contains("?  ~ update"), "{text}");
        assert!(!single_line.contains("direct:"), "{text}");
        assert!(!single_line.contains(":10-12 [2 evidence]"), "{text}");
    }

    #[test]
    fn empty_filter_uses_shared_message_and_keeps_only_filter_search_hints() {
        let mut state = direct_only_state();
        state.apply(PlanListAction::ToggleFilter);
        let text = buffer_text(&render_to_buffer(&state, 80, 12));

        assert!(
            text.contains("No resources match the current filter/search."),
            "{text}"
        );
        assert!(text.contains("f change filter"), "{text}");
        assert!(!text.contains("/ edit search"), "{text}");
        assert!(!text.contains("Enter details"), "{text}");
        assert!(!text.contains("y resource"), "{text}");
        assert!(!text.contains("j/k/↑/↓ select"), "{text}");
    }

    #[test]
    fn review_count_style_uses_full_plan_count_in_every_list_view() {
        let views = [
            ("normal", SummaryView::Normal),
            ("search_input", SummaryView::SearchInput),
            ("search_confirmed", SummaryView::SearchConfirmed),
            ("needs_review_filter", SummaryView::NeedsReviewFilter),
        ];

        for (state_name, has_review) in [("zero", false), ("nonzero", true)] {
            for (view_name, view) in views {
                let state = state_for_review_count(has_review, view);
                let buffer = render_to_buffer(&state, 100, 20);

                assert_review_count_style(&buffer, &state, &format!("{state_name}_{view_name}"));
            }
        }
    }

    #[test]
    fn empty_state_explains_that_there_are_no_resource_changes() {
        let state = empty_state();
        let text = buffer_text(&render_to_buffer(&state, 80, 12));

        assert!(text.contains("No resource changes."), "{text}");
        assert!(text.contains("Needs review: 0/0"));
        assert!(!text.contains("f change filter"), "{text}");
        assert!(!text.contains("/ edit search"), "{text}");
        assert!(!text.contains("j/k/↑/↓ select"), "{text}");
    }

    #[test]
    fn connected_list_keeps_context_and_summarizes_issues_at_minimum_size() {
        let state = connected_state();
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        let header = text.lines().next().expect("rendered header");
        assert!(header.starts_with("Terracotta | "), "{text}");
        assert!(header.contains("branch: feature/review"), "{text}");
        assert!(text.contains("Git: working tree vs HEAD"), "{text}");
        assert!(text.contains("Needs review: 1/1"), "{text}");
        assert!(text.contains("Analysis incomplete"), "{text}");
        assert!(text.contains("(+2 more)"), "{text}");
        assert!(text.contains("q quit"), "{text}");
    }

    #[test]
    fn connected_list_keeps_both_notices_at_minimum_width() {
        let state = connected_state();
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(text.contains("Unshown changes: output (1)"), "{text}");
        assert!(
            text.contains("Analysis incomplete: first analys..."),
            "{text}"
        );
        assert!(text.contains("(+2 more)"), "{text}");
        assert!(text.contains("Needs review: 1/1"), "{text}");
        assert!(text.contains("q quit"), "{text}");
    }

    #[test]
    fn diagnostics_notice_has_open_hint_and_keeps_count_during_empty_search() {
        let mut state = synthetic_state();
        let diagnostics = warning_diagnostics();
        let text = buffer_text(&render_to_buffer_with_diagnostics(
            &state,
            &diagnostics,
            80,
            16,
        ));

        assert!(text.contains("Diagnostics: 1 (w)"), "{text}");
        assert!(text.contains("w diagnostics"), "{text}");

        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("missing".to_owned()));
        let text = buffer_text(&render_to_buffer_with_diagnostics(
            &state,
            &diagnostics,
            80,
            16,
        ));

        assert!(text.contains("Diagnostics: 1"), "{text}");
        assert!(!text.contains("Diagnostics: 1 (w)"), "{text}");
        assert!(text.contains("Showing: 0/4"), "{text}");
        assert!(!text.contains("w diagnostics"), "{text}");
    }

    #[test]
    fn narrow_terminal_shows_resize_message() {
        let state = synthetic_state();
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH - 3, MIN_HEIGHT));

        assert!(text.contains("Terminal too small. Resize or press q to"));
        assert!(!text.contains("quit."));
    }

    #[test]
    fn narrow_list_keeps_resource_rows_on_one_line() {
        let state = synthetic_state();
        let text = buffer_text(&render_to_buffer(&state, 60, 16));

        assert!(!text.lines().any(|line| line.contains("direct:")), "{text}");
        assert!(
            text.contains("aws_s3_bucket.logs_with_a_very_long_reso..."),
            "{text}"
        );
    }

    #[test]
    fn unicode_rows_fit_the_allocated_cells_on_one_line() {
        let state = unicode_state();
        let wide_buffer = render_to_buffer(&state, 120, 16);
        let wide_text = buffer_text(&wide_buffer);
        let wide_lines = wide_text.lines().collect::<Vec<_>>();
        let wide_row_index = wide_lines
            .iter()
            .position(|line| line.contains("aws_instance."))
            .expect("wide resource row should be rendered");
        let wide_row = wide_lines[wide_row_index];
        assert!(!wide_row.contains("direct:"));
        assert_eq!(
            wide_buffer
                .cell((
                    wide_buffer.area().right() - 1,
                    u16::try_from(wide_row_index).expect("buffer row should fit in u16"),
                ))
                .expect("wide row should have a right border")
                .symbol(),
            "│"
        );

        let narrow_buffer = render_to_buffer(&state, 48, 16);
        let narrow_text = buffer_text(&narrow_buffer);
        let narrow_lines = narrow_text.lines().collect::<Vec<_>>();
        let address_line = narrow_lines
            .iter()
            .position(|line| line.contains("aws_instance."))
            .expect("narrow resource row should be rendered");
        let evidence_line = narrow_lines
            .iter()
            .position(|line| line.contains("direct:"));
        assert!(evidence_line.is_none());
        assert!(narrow_lines[address_line].contains("..."));
        assert_eq!(
            narrow_buffer
                .cell((
                    narrow_buffer.area().right() - 1,
                    u16::try_from(address_line).expect("buffer row should fit in u16"),
                ))
                .expect("narrow row should have a right border")
                .symbol(),
            "│"
        );
        assert_eq!(
            state.items()[0].address(),
            "aws_instance.東京_東京_東京_東京_東京_東京_東京"
        );
    }

    #[test]
    fn minimum_supported_width_keeps_summary_counts_visible() {
        let state = synthetic_state();
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(
            text.contains("+1 create  ~1 update  R1 replace  -1 delete"),
            "{text}"
        );
        assert!(text.contains("Needs review: 2/4"), "{text}");
    }

    #[test]
    fn minimum_supported_size_keeps_wrapped_item_and_unshown_summary_visible() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::SelectNext);
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(text.contains("Unshown changes: output (1)"), "{text}");
        assert!(text.contains("aws_s3_bucket.logs_with_a_ve..."), "{text}");
        assert!(
            text.contains("! no direct match | ? analysis incomplete"),
            "{text}"
        );
    }

    #[test]
    fn list_copy_notices_keep_required_footer_at_supported_widths() {
        let cases = [
            (
                CopyNotice::Copied {
                    target: CopyTarget::Resource,
                    resource_count: 1,
                },
                "Copied selected resource (redacted).",
            ),
            (CopyNotice::Failed, "Copy failed: clipboard unavailable."),
        ];

        for (notice, expected) in cases {
            for width in [48, 60, 80, 120] {
                let state = synthetic_state();
                let text = buffer_text(&render_to_buffer_with_notice(
                    &state,
                    Some(notice),
                    width,
                    MIN_HEIGHT,
                ));
                let notice_line = text
                    .lines()
                    .position(|line| line.contains(expected))
                    .expect("copy notice should be rendered");
                let footer_line = text
                    .lines()
                    .position(|line| line.contains("q quit"))
                    .expect("quit hint should be rendered");

                assert!(notice_line < footer_line, "width: {width}\n{text}");
            }
        }
    }

    #[test]
    fn selection_actions_select_adjacent_items() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::SelectNext);
        assert_eq!(state.selected(), Some(1));
        state.apply(PlanListAction::SelectPrevious);
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn empty_plan_disables_resource_copy() {
        let state = empty_state();

        assert!(!state.can_copy(CopyTarget::Resource));
    }

    #[test]
    fn plan_copy_availability_ignores_filter_and_search_scope() {
        let mut state = connected_state();
        assert!(state.can_copy(CopyTarget::Plan));

        state.apply(PlanListAction::ToggleFilter);
        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("not-present".to_owned()));

        assert!(state.can_copy(CopyTarget::Plan));
    }

    #[test]
    fn list_offset_survives_detail_return_until_selection_needs_visibility() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::SelectResource(2));
        let mut list_state = ListState::default();

        render_to_buffer_with_state(&state, 80, MIN_HEIGHT + 1, &mut list_state);
        let original_offset = list_state.offset();
        assert!(original_offset > 0);

        render_to_buffer_with_state(&state, 80, MIN_HEIGHT + 1, &mut list_state);
        assert_eq!(list_state.offset(), original_offset);

        state.apply(PlanListAction::SelectResource(0));
        render_to_buffer_with_state(&state, 80, MIN_HEIGHT + 1, &mut list_state);
        assert!(list_state.offset() < original_offset);
    }

    #[test]
    fn search_render_shows_query_and_explicitly_reports_no_matches() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("AWS_S3".to_owned()));
        let text = buffer_text(&render_to_buffer(&state, 100, 16));

        assert!(text.contains("Filter: All (f)  Showing: 1/4"), "{text}");
        assert!(text.contains("/ AWS_S3_"), "{text}");
        assert!(
            text.contains("Enter confirm | Esc cancel | Ctrl-C quit | Type to search"),
            "{text}"
        );
        assert!(text.contains("aws_s3_bucket"), "{text}");
        assert!(!text.contains("aws_instance.api"), "{text}");

        state.apply(PlanListAction::ConfirmSearch);
        let text = buffer_text(&render_to_buffer(&state, 100, 16));
        assert!(text.contains("Search: AWS_S3"), "{text}");

        state.apply(PlanListAction::SetSearch("missing".to_owned()));
        let text = buffer_text(&render_to_buffer(&state, 100, 16));
        assert!(
            text.contains("No resources match the current filter/search."),
            "{text}"
        );
        assert!(text.contains("/ edit search"), "{text}");
        assert!(!text.contains("f change filter"), "{text}");
    }

    #[test]
    fn search_at_minimum_width_keeps_summary_and_showing_counts() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("security".to_owned()));
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(text.contains("Needs review: 2/4"), "{text}");
        assert!(text.contains("Showing: 1/4"), "{text}");
        assert!(text.contains("/ security_"), "{text}");
    }

    #[test]
    fn connected_search_at_minimum_size_keeps_notices_counts_and_no_match_message() {
        let mut state = connected_state();
        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("missing".to_owned()));
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(text.contains("Unshown: output (1)"), "{text}");
        assert!(text.contains("Analysis incomplete"), "{text}");
        assert!(text.contains("Needs review: 1/1"), "{text}");
        assert!(text.contains("Showing: 0/1"), "{text}");
        assert!(
            text.contains("No resources match the current filter/search."),
            "{text}"
        );
    }

    #[test]
    fn search_and_filter_scope_detail_to_the_same_visible_resources() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::ToggleFilter);
        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("worker".to_owned()));
        state.apply(PlanListAction::ConfirmSearch);

        let detail = ReviewDetailState::from_list(&state)
            .expect("the filtered search result should open details");
        assert_eq!(detail.index(), 0);
        assert_eq!(detail.total(), 1);
        assert_eq!(state.filter(), PlanListFilter::NeedsReview);
        assert_eq!(state.search(), "worker");
    }

    #[test]
    fn filter_shows_only_review_items_and_keeps_plan_counts() {
        let mut state = synthetic_state();

        state.apply(PlanListAction::ToggleFilter);
        let text = buffer_text(&render_to_buffer(&state, 100, 16));

        assert!(text.contains("Needs review: 2/4"), "{text}");
        assert!(text.contains("Filter: Needs review"), "{text}");
        assert!(text.contains("Showing: 2/4"), "{text}");
        assert!(!text.contains("aws_instance.api"), "{text}");
        assert!(text.contains("aws_instance.worker"), "{text}");

        let detail =
            ReviewDetailState::from_list(&state).expect("filtered selection should open details");
        assert_eq!(detail.index(), 0);
        assert_eq!(detail.total(), 2);
        assert_eq!(state.filter(), PlanListFilter::NeedsReview);
    }

    #[test]
    fn empty_filter_uses_common_no_match_message() {
        let mut state = direct_only_state();
        state.apply(PlanListAction::ToggleFilter);
        let text = buffer_text(&render_to_buffer(&state, 80, 12));

        assert!(
            text.contains("No resources match the current filter/search."),
            "{text}"
        );
        assert!(text.contains("Showing: 0/1"), "{text}");
    }

    #[test]
    fn filtered_context_keeps_filter_summary_at_minimum_size() {
        let mut state = connected_state();
        state.apply(PlanListAction::ToggleFilter);
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(text.contains("Filter: Needs review"), "{text}");
        assert!(text.contains("Showing: 1/1"), "{text}");
        assert!(text.contains("q quit"), "{text}");
    }

    fn direct_only_state() -> PlanListState {
        let change = synthetic_change(
            "aws_instance.direct",
            ResourceChangeKind::Update,
            PlanAction::Update,
        );
        let source_files = vec![SourceFileAnalysis::new(
            "main.tf".into(),
            SourceSide::After,
            vec![ResourceSourceLocation::new(
                ResourceAddress::new("aws_instance", "direct"),
                "main.tf".into(),
                SourceSide::After,
                SourceRange::new(1, 4),
            )],
            Vec::new(),
        )];
        let attributions = attribute_changes(
            std::slice::from_ref(&change),
            &source_files,
            &[AttributionSourceLineChange::new(
                "main.tf",
                SourceSide::After,
                SourceRange::new(2, 2),
            )],
        );
        PlanListState::from_plan(
            Plan {
                changes: vec![change],
                summary: PlanSummary {
                    updates: 1,
                    ..PlanSummary::default()
                },
                unsupported_changes: Vec::new(),
            },
            attributions,
            ReviewComparison::working_tree(),
        )
        .expect("direct-only fixture should build a list")
    }

    fn multiple_evidence_state() -> PlanListState {
        let change = synthetic_change(
            "aws_instance.api",
            ResourceChangeKind::Update,
            PlanAction::Update,
        );
        let source_files = vec![
            SourceFileAnalysis::new(
                "/repo/environments/prod/main.tf".into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new("aws_instance", "api"),
                    "/repo/environments/prod/main.tf".into(),
                    SourceSide::After,
                    SourceRange::new(10, 12),
                )],
                Vec::new(),
            ),
            SourceFileAnalysis::new(
                "/repo/common/main.tf".into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new("aws_instance", "api"),
                    "/repo/common/main.tf".into(),
                    SourceSide::After,
                    SourceRange::new(20, 20),
                )],
                Vec::new(),
            ),
        ];
        let attributions = attribute_changes(
            std::slice::from_ref(&change),
            &source_files,
            &[
                AttributionSourceLineChange::new(
                    "/repo/environments/prod/main.tf",
                    SourceSide::After,
                    SourceRange::new(11, 11),
                ),
                AttributionSourceLineChange::new(
                    "/repo/common/main.tf",
                    SourceSide::After,
                    SourceRange::new(20, 20),
                ),
            ],
        );
        let review = PlanReview::new(
            PathBuf::from("/repo/environments/prod"),
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
            attributions,
            ReviewComparison::working_tree(),
            Vec::new(),
        )
        .with_repository_root(Some(PathBuf::from("/repo")));
        PlanListState::from_review(review).expect("multiple evidence fixture should build a list")
    }

    fn long_evidence_incomplete_state() -> PlanListState {
        let address = "aws_instance.resource_with_a_long_name_that_forces_a_narrow_git_column";
        let change = synthetic_change(address, ResourceChangeKind::Update, PlanAction::Update);
        let first_path = "/repo/environments/production/very_long_configuration/main.tf";
        let second_path = "/repo/environments/shared/another_configuration/main.tf";
        let source_files = vec![
            SourceFileAnalysis::new(
                first_path.into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new(
                        "aws_instance",
                        "resource_with_a_long_name_that_forces_a_narrow_git_column",
                    ),
                    first_path.into(),
                    SourceSide::After,
                    SourceRange::new(10, 12),
                )],
                Vec::new(),
            ),
            SourceFileAnalysis::new(
                second_path.into(),
                SourceSide::After,
                vec![ResourceSourceLocation::new(
                    ResourceAddress::new(
                        "aws_instance",
                        "resource_with_a_long_name_that_forces_a_narrow_git_column",
                    ),
                    second_path.into(),
                    SourceSide::After,
                    SourceRange::new(30, 31),
                )],
                Vec::new(),
            ),
        ];
        let mut attributions = attribute_changes(
            std::slice::from_ref(&change),
            &source_files,
            &[
                AttributionSourceLineChange::new(
                    first_path,
                    SourceSide::After,
                    SourceRange::new(10, 10),
                ),
                AttributionSourceLineChange::new(
                    second_path,
                    SourceSide::After,
                    SourceRange::new(30, 30),
                ),
            ],
        );
        mark_analysis_incomplete(&mut attributions, &[AnalysisIssue::git("incomplete")]);
        let review = PlanReview::new(
            PathBuf::from("/repo/environments/production"),
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
            attributions,
            ReviewComparison::working_tree(),
            Vec::new(),
        )
        .with_repository_root(Some(PathBuf::from("/repo")));
        PlanListState::from_review(review)
            .expect("long evidence incomplete fixture should build a list")
    }

    fn unicode_state() -> PlanListState {
        let address = "aws_instance.東京_東京_東京_東京_東京_東京_東京";
        let change = synthetic_change(address, ResourceChangeKind::Update, PlanAction::Update);
        let source_files = vec![SourceFileAnalysis::new(
            "証拠/東京/very_long_source_file.tf".into(),
            SourceSide::After,
            vec![ResourceSourceLocation::new(
                ResourceAddress::new("aws_instance", "東京_東京_東京_東京_東京_東京_東京"),
                "証拠/東京/very_long_source_file.tf".into(),
                SourceSide::After,
                SourceRange::new(12, 18),
            )],
            Vec::new(),
        )];
        let attributions = attribute_changes(
            std::slice::from_ref(&change),
            &source_files,
            &[AttributionSourceLineChange::new(
                "証拠/東京/very_long_source_file.tf",
                SourceSide::After,
                SourceRange::new(12, 12),
            )],
        );

        PlanListState::from_plan(
            Plan {
                changes: vec![change],
                summary: PlanSummary {
                    updates: 1,
                    ..PlanSummary::default()
                },
                unsupported_changes: Vec::new(),
            },
            attributions,
            ReviewComparison::working_tree(),
        )
        .expect("Unicode synthetic change should produce a list")
    }

    #[derive(Clone, Copy)]
    enum SummaryView {
        Normal,
        SearchInput,
        SearchConfirmed,
        NeedsReviewFilter,
    }

    fn state_for_review_count(has_review: bool, view: SummaryView) -> PlanListState {
        let mut state = if has_review {
            synthetic_state()
        } else {
            direct_only_state()
        };

        match view {
            SummaryView::Normal => {}
            SummaryView::SearchInput => {
                state.apply(PlanListAction::BeginSearch);
                state.apply(PlanListAction::SetSearch("aws".to_owned()));
            }
            SummaryView::SearchConfirmed => {
                state.apply(PlanListAction::BeginSearch);
                state.apply(PlanListAction::SetSearch("aws".to_owned()));
                state.apply(PlanListAction::ConfirmSearch);
            }
            SummaryView::NeedsReviewFilter => state.apply(PlanListAction::ToggleFilter),
        }

        state
    }

    fn assert_review_count_style(buffer: &Buffer, state: &PlanListState, case: &str) {
        let label = format!(
            "Needs review: {}/{}",
            state.needs_review_count(),
            state.items().len()
        );
        let area = buffer.area();
        let (x, y) = (area.y..area.bottom())
            .find_map(|y| {
                let line = (area.x..area.right())
                    .filter_map(|x| buffer.cell((x, y)))
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>();
                line.find(&label).map(|start| {
                    (
                        area.x + u16::try_from(start).expect("summary offset fits in terminal"),
                        y,
                    )
                })
            })
            .unwrap_or_else(|| panic!("summary label not found for {case}: {label}"));
        for offset in 0..label.chars().count() {
            let cell = buffer
                .cell((
                    x + u16::try_from(offset).expect("summary width fits in terminal"),
                    y,
                ))
                .expect("summary cell should exist");
            if cell.symbol() == " " {
                continue;
            }
            assert_eq!(
                cell.fg,
                Color::Rgb(0xe9, 0xdb, 0xdb),
                "foreground for {case} at {offset}"
            );
            assert!(!cell.modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn renders_unshown_change_types_when_plan_has_unsupported_changes() {
        let state = PlanListState::from_plan(
            Plan {
                changes: Vec::new(),
                summary: PlanSummary::default(),
                unsupported_changes: vec![UnsupportedChange {
                    scope: UnsupportedChangeScope::Output,
                    address: "output.value".to_owned(),
                    actions: vec![PlanAction::Update],
                    kind: UnsupportedChangeKind::Output,
                    reason: None,
                    action_type: None,
                }],
            },
            Vec::new(),
            ReviewComparison::working_tree(),
        )
        .expect("unsupported changes do not require list rows");
        let text = buffer_text(&render_to_buffer(&state, 80, 12));

        assert!(text.contains("Unshown changes: output (1)"));
    }
}
