use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use crate::app::attribution::{SourceLineChange as AttributionSourceLineChange, attribute_changes};
use crate::app::plan::{
    Plan, PlanAction, PlanSummary, PlanValue, ResourceChange, ResourceChangeKind, ResourceMode,
    UnsupportedChange, UnsupportedChangeKind, UnsupportedChangeScope,
};
use crate::app::plan_list::{PlanListAction, PlanListFilter, PlanListItem, PlanListState};
use crate::app::source_location::{
    ResourceAddress, ResourceSourceLocation, SourceFileAnalysis, SourceRange, SourceSide,
};

const MIN_HEIGHT: u16 = 11;
const MIN_CONTENT_HEIGHT: u16 = MIN_HEIGHT - 2;
const MIN_WIDTH: u16 = 48;
const ANALYSIS_PREFIX: &str = "Analysis incomplete: ";

pub(super) fn run_synthetic() -> io::Result<()> {
    let mut state = synthetic_state();
    ratatui::run(|terminal| run_plan_list(terminal, &mut state))
}

fn run_plan_list(terminal: &mut DefaultTerminal, state: &mut PlanListState) -> io::Result<()> {
    let mut list_view = ListState::default();
    let mut detail = None;
    loop {
        if let Some(detail_state) = detail.as_ref() {
            terminal.draw(|frame| {
                super::resource_detail::render_resource_detail(frame, detail_state);
            })?;
        } else {
            terminal.draw(|frame| render_plan_list_with_state(frame, state, &mut list_view))?;
        }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.is_press()
        {
            if let Some(detail_state) = detail.as_mut() {
                let size = terminal.size()?;
                let viewport_height = detail_state.viewport_height(size.height);
                match super::resource_detail::key_to_input(key) {
                    Some(super::resource_detail::DetailInput::Back) => {
                        state.apply(PlanListAction::SelectResource(detail_state.item_index()));
                        detail = None;
                    }
                    Some(super::resource_detail::DetailInput::Quit) => return Ok(()),
                    Some(super::resource_detail::DetailInput::Navigate(navigation)) => {
                        detail_state.navigate(navigation, state);
                    }
                    Some(super::resource_detail::DetailInput::Action(action)) => {
                        detail_state.apply(action, size.width.saturating_sub(2), viewport_height);
                    }
                    None => {}
                }
            } else if state.searching() {
                if handle_search_input(state, key) {
                    return Ok(());
                }
            } else {
                match key_to_action(key) {
                    Some(ListInput::Quit) => return Ok(()),
                    Some(ListInput::OpenDetail) => {
                        detail = super::resource_detail::ResourceDetailState::from_list(state);
                    }
                    Some(ListInput::StartSearch) => {
                        state.apply(PlanListAction::BeginSearch);
                    }
                    Some(ListInput::Selection(action)) => state.apply(action),
                    None => {}
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ListInput {
    Selection(PlanListAction),
    OpenDetail,
    StartSearch,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchInput {
    Insert(char),
    Delete,
    Confirm,
    Cancel,
    Quit,
}

pub(super) fn key_to_action(key: KeyEvent) -> Option<ListInput> {
    if matches!(key.code, KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return Some(ListInput::Quit);
    }

    match key.code {
        KeyCode::Char('f') => Some(ListInput::Selection(PlanListAction::ToggleFilter)),
        KeyCode::Char('/') => Some(ListInput::StartSearch),
        KeyCode::Up | KeyCode::Char('k') => {
            Some(ListInput::Selection(PlanListAction::SelectPrevious))
        }
        KeyCode::Down | KeyCode::Char('j') => {
            Some(ListInput::Selection(PlanListAction::SelectNext))
        }
        KeyCode::Enter => Some(ListInput::OpenDetail),
        _ => None,
    }
}

pub(super) fn search_key_to_input(key: KeyEvent) -> Option<SearchInput> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(SearchInput::Quit);
    }

    match key.code {
        KeyCode::Enter => Some(SearchInput::Confirm),
        KeyCode::Esc => Some(SearchInput::Cancel),
        KeyCode::Backspace => Some(SearchInput::Delete),
        KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(SearchInput::Insert(character))
        }
        _ => None,
    }
}

pub(super) fn handle_search_input(state: &mut PlanListState, key: KeyEvent) -> bool {
    match search_key_to_input(key) {
        Some(SearchInput::Quit) => true,
        Some(SearchInput::Confirm) => {
            state.apply(PlanListAction::ConfirmSearch);
            false
        }
        Some(SearchInput::Cancel) => {
            state.apply(PlanListAction::CancelSearch);
            false
        }
        Some(SearchInput::Delete) => {
            let mut search = state.search().to_owned();
            search.pop();
            state.apply(PlanListAction::SetSearch(search));
            false
        }
        Some(SearchInput::Insert(character)) => {
            let mut search = state.search().to_owned();
            search.push(character);
            state.apply(PlanListAction::SetSearch(search));
            false
        }
        None => false,
    }
}

pub(super) fn render_plan_list_with_state(
    frame: &mut Frame<'_>,
    state: &PlanListState,
    list_state: &mut ListState,
) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_terminal_too_small(frame, area);
        return;
    }

    let block = Block::new()
        .borders(Borders::ALL)
        .title("Terracotta / Plan");
    let content_area = block.inner(area);
    frame.render_widget(block, area);

    let has_search = state.searching() || !state.search().is_empty();
    let compact_layout = has_search && content_area.height <= MIN_CONTENT_HEIGHT;
    let notices = notice_lines(state, content_area.width as usize, compact_layout);
    let summary = summary_lines(state);
    let search_height = u16::from(has_search);
    let notice_height = match notices.len() {
        0 => 0,
        1 => 1,
        _ => 2,
    };
    let context_height = u16::from(state.context().is_some()) * 2;
    let separator_height = u16::from(!compact_layout && notice_height < 2);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(context_height),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(search_height),
            Constraint::Length(separator_height),
            Constraint::Length(notice_height),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(content_area);

    if let Some(context) = state.context() {
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
        Paragraph::new(Line::from(format!("compare {}", state.comparison()))),
        chunks[1],
    );
    frame.render_widget(Paragraph::new(summary), chunks[2]);
    if has_search {
        let search_line = if state.searching() {
            format!("/ {}_", state.search())
        } else {
            format!("Search: {}", state.search())
        };
        frame.render_widget(Paragraph::new(search_line), chunks[3]);
    }
    frame.render_widget(separator(chunks[4].width), chunks[4]);

    if !notices.is_empty() {
        frame.render_widget(Paragraph::new(notices), chunks[5]);
    }

    render_rows(frame, state, chunks[6], list_state);
    frame.render_widget(
        Paragraph::new(footer_line(state, content_area.width as usize)),
        chunks[7],
    );
}

fn notice_lines(state: &PlanListState, width: usize, compact: bool) -> Vec<Line<'static>> {
    let unsupported = state.unsupported_summary();
    let analysis = state.analysis_issues().first().map(|first| {
        let suffix = match state.analysis_issues().len() {
            0 | 1 => String::new(),
            count => format!(" (+{} more)", count - 1),
        };
        let issue_width = width
            .saturating_sub(ANALYSIS_PREFIX.len())
            .saturating_sub(suffix.chars().count());
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
            (None, None) => return Vec::new(),
        };
        return vec![Line::from(truncate_end(&message, width))];
    }

    match (unsupported, analysis) {
        (Some(summary), Some(analysis)) => vec![
            Line::from(truncate_end(&summary, width)),
            Line::from(analysis),
        ],
        (Some(summary), None) => vec![Line::from(truncate_end(&summary, width))],
        (None, Some(analysis)) => vec![Line::from(analysis)],
        (None, None) => Vec::new(),
    }
}

fn render_terminal_too_small(frame: &mut Frame<'_>, area: Rect) {
    let message = Paragraph::new("Terminal too small. Resize or press q to quit.")
        .style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(message, area);
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
        } else if !state.search().is_empty() {
            format!("No matching resources. Search: {}", state.search())
        } else {
            format!(
                "No items in this filter. Press f to show all {} changes.",
                state.items().len()
            )
        };
        frame.render_widget(Paragraph::new(message), area);
        return;
    }

    let header = Line::from(vec![
        Span::styled("ACTION", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  RESOURCE"),
        Span::raw("  GIT"),
    ]);
    let items = state
        .visible_items()
        .map(|item| list_item(item, area.width.saturating_sub(2) as usize))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(Block::new().title(header))
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::UNDERLINED),
        );
    *list_state.selected_mut() = state.selected();
    frame.render_stateful_widget(list, area, list_state);
}

fn list_item(item: &PlanListItem, width: usize) -> ListItem<'static> {
    let marker = if item.needs_review() { "!" } else { " " };
    let prefix = format!("{marker} {} ", action_symbol(item.kind()));
    let git = item.git_label();
    let inline_separator = "  ";
    let address_width = width
        .saturating_sub(prefix.chars().count())
        .saturating_sub(inline_separator.chars().count())
        .saturating_sub(git.chars().count());

    let action_style = action_style(item.kind());
    let review_style = if item.needs_review() {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let git_style = git_style(item);

    if address_width >= 12 && item.address().chars().count() <= address_width {
        return ListItem::new(Line::from(vec![
            Span::styled(marker.to_owned(), review_style),
            Span::raw(" "),
            Span::styled(action_symbol(item.kind()), action_style),
            Span::raw(" "),
            Span::raw(truncate_end(item.address(), address_width)),
            Span::raw(inline_separator),
            Span::styled(git, git_style),
        ]));
    }

    let address_width = width.saturating_sub(prefix.chars().count());
    let address = truncate_end(item.address(), address_width);
    let evidence_width = width.saturating_sub(4);
    ListItem::new(vec![
        Line::from(vec![
            Span::styled(marker.to_owned(), review_style),
            Span::raw(" "),
            Span::styled(action_symbol(item.kind()), action_style),
            Span::raw(" "),
            Span::raw(address),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled(truncate_end(&git, evidence_width), git_style),
        ]),
    ])
}

fn summary_lines(state: &PlanListState) -> Vec<Line<'static>> {
    let summary = state.summary();
    let needs_review = Span::styled(
        format!(
            "Needs review: {} / {}",
            state.needs_review_count(),
            state.items().len()
        ),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let action_line = Line::from(vec![
        Span::styled(
            format!("+{} create", summary.creates),
            action_style(ResourceChangeKind::Create),
        ),
        Span::raw("  "),
        Span::styled(
            format!("~{} update", summary.updates),
            action_style(ResourceChangeKind::Update),
        ),
        Span::raw("  "),
        Span::styled(
            format!("R{} replace", summary.replaces),
            action_style(ResourceChangeKind::Replace),
        ),
        Span::raw("  "),
        Span::styled(
            format!("-{} delete", summary.deletes),
            action_style(ResourceChangeKind::Delete),
        ),
    ]);
    if state.filter() == PlanListFilter::All && !has_search(state) {
        return vec![
            action_line,
            Line::from(vec![needs_review, Span::raw("   Filter: All")]),
        ];
    }

    let showing = if state.filter() == PlanListFilter::All {
        format!(
            "Needs review: {} / {}",
            state.needs_review_count(),
            state.items().len()
        )
    } else {
        format!(
            "Review {}/{}",
            state.needs_review_count(),
            state.items().len()
        )
    };
    vec![
        action_line,
        Line::from(format!(
            "{}  Filter: {}  Showing {}/{}",
            showing,
            state.filter().label(),
            state.visible_count(),
            state.items().len()
        )),
    ]
}

fn has_search(state: &PlanListState) -> bool {
    state.searching() || !state.search().is_empty()
}

fn footer_line(state: &PlanListState, width: usize) -> Line<'static> {
    if state.searching() {
        let footer = if width < 55 {
            "Type to search  Enter  Esc cancel  Ctrl-C"
        } else {
            "Type to search  Enter confirm  Esc cancel  Ctrl-C quit"
        };
        return Line::from(footer);
    }

    let footer = if width < 55 {
        "j/k/↑↓ select  f filter  / search  q/Ctrl-C"
    } else {
        "j/k/↑↓ select  Enter  f filter  / search  q/Ctrl-C quit"
    };
    Line::from(footer)
}

fn separator(width: u16) -> Paragraph<'static> {
    Paragraph::new("─".repeat(width as usize)).style(Style::default().fg(Color::DarkGray))
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

fn git_style(item: &PlanListItem) -> Style {
    if item.needs_review() {
        return Style::default();
    }

    Style::default().fg(Color::Cyan)
}

fn truncate_end(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    format!(
        "{}...",
        value.chars().take(max_chars - 3).collect::<String>()
    )
}

fn synthetic_state() -> PlanListState {
    let mut changes = vec![
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
    ];
    let source_files = vec![
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
    ];
    changes[2].mode = ResourceMode::Data;
    let changed_lines = vec![
        AttributionSourceLineChange::new("main.tf", SourceSide::After, SourceRange::new(42, 43)),
        AttributionSourceLineChange::new("storage.tf", SourceSide::After, SourceRange::new(8, 8)),
        AttributionSourceLineChange::new("worker.tf", SourceSide::After, SourceRange::new(14, 14)),
    ];
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
        "working tree vs HEAD",
    )
    .expect("synthetic changes and attributions should align")
}

fn synthetic_change(address: &str, kind: ResourceChangeKind, action: PlanAction) -> ResourceChange {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossterm::event::{KeyEventKind, KeyEventState};
    use ratatui::buffer::Buffer;

    use crate::app::attribution::AnalysisIssue;
    use crate::app::review::{
        PlanReview, ReviewComparison, ReviewComparisonBasis, ReviewComparisonStatus,
    };
    use crate::ui::test_support::{buffer_text, render_to_buffer as render_test_buffer};

    use super::*;

    mod render_snapshots;

    fn render_to_buffer(state: &PlanListState, width: u16, height: u16) -> Buffer {
        let mut list_state = ListState::default();
        render_to_buffer_with_state(state, width, height, &mut list_state)
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
                None,
                None,
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
        PlanListState::from_review(&review).expect("review data should build a list")
    }

    #[test]
    fn renders_summary_selection_and_git_evidence() {
        let state = synthetic_state();
        let buffer = render_to_buffer(&state, 120, 20);
        let text = buffer_text(&buffer);

        assert!(text.contains("compare working tree vs HEAD"));
        assert!(text.contains("+1 create  ~1 update  R1 replace  -1 delete"));
        assert!(text.contains("Needs review: 2 / 4"), "{text}");
        assert!(text.contains("main.tf:42-46"));
        assert!(text.contains("incomplete"));
        assert!(text.contains("no match"));
        assert!(text.contains("j/k/↑↓ select  Enter  f filter  / search  q/Ctrl-C quit"));
        assert!(
            text.contains(
                "aws_s3_bucket.logs_with_a_very_long_resource_address_that_needs_truncation_for_narrow_terminal",
            ),
            "{text}"
        );

        let delete_cell = buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == "-")
            .expect("delete symbol should be rendered");
        assert_eq!(delete_cell.fg, Color::Red);
    }

    #[test]
    fn empty_state_explains_that_there_are_no_resource_changes() {
        let state = PlanListState::empty("working tree vs HEAD");
        let text = buffer_text(&render_to_buffer(&state, 80, 12));

        assert!(text.contains("No resource changes."), "{text}");
        assert!(text.contains("Needs review: 0 / 0"));
    }

    #[test]
    fn connected_list_keeps_context_and_summarizes_issues_at_minimum_size() {
        let state = connected_state();
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(text.contains("cwd /infra/prod"), "{text}");
        assert!(text.contains("workspace default"), "{text}");
        assert!(text.contains("git feature/review"), "{text}");
        assert!(text.contains("Needs review: 1 / 1"), "{text}");
        assert!(text.contains("Analysis incomplete"), "{text}");
        assert!(text.contains("(+2 more)"), "{text}");
        assert!(
            text.contains("j/k/↑↓ select  f filter  / search  q/Ctrl-C"),
            "{text}"
        );
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
        assert!(text.contains("Needs review: 1 / 1"), "{text}");
        assert!(
            text.contains("j/k/↑↓ select  f filter  / search  q/Ctrl-C"),
            "{text}"
        );
    }

    #[test]
    fn narrow_terminal_shows_resize_message() {
        let state = synthetic_state();
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH - 1, MIN_HEIGHT));

        assert!(text.contains("Terminal too small. Resize or press q to quit."));
    }

    #[test]
    fn narrow_list_moves_git_evidence_to_the_next_line() {
        let state = synthetic_state();
        let text = buffer_text(&render_to_buffer(&state, 60, 16));

        assert!(
            text.lines()
                .any(|line| line.contains("      direct: storage.tf:8-10")),
            "{text}"
        );
        assert!(text.contains("aws_s3_bucket.logs_with_a_very_long_resource_addr..."));
    }

    #[test]
    fn minimum_supported_width_keeps_summary_counts_visible() {
        let state = synthetic_state();
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, 12));

        assert!(
            text.contains("+1 create  ~1 update  R1 replace  -1 delete"),
            "{text}"
        );
        assert!(text.contains("Needs review: 2 / 4"), "{text}");
    }

    #[test]
    fn minimum_supported_size_keeps_wrapped_item_and_unshown_summary_visible() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::SelectNext);
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(text.contains("Unshown changes: output (1)"), "{text}");
        assert!(
            text.contains("aws_s3_bucket.logs_with_a_very_long_r..."),
            "{text}"
        );
        assert!(text.contains("direct: storage.tf:8-10"), "{text}");
    }

    #[test]
    fn navigation_keys_select_adjacent_items_and_quit_keys_exit() {
        let mut state = synthetic_state();
        let key = |code, modifiers| KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        assert_eq!(
            key_to_action(key(KeyCode::Char('j'), KeyModifiers::NONE)),
            Some(ListInput::Selection(PlanListAction::SelectNext))
        );
        state.apply(PlanListAction::SelectNext);
        assert_eq!(state.selected(), Some(1));
        assert_eq!(
            key_to_action(key(KeyCode::Up, KeyModifiers::NONE)),
            Some(ListInput::Selection(PlanListAction::SelectPrevious))
        );
        state.apply(PlanListAction::SelectPrevious);
        assert_eq!(state.selected(), Some(0));
        assert_eq!(
            key_to_action(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(ListInput::Quit)
        );
        assert_eq!(
            key_to_action(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(ListInput::Quit)
        );
        assert_eq!(
            key_to_action(key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(ListInput::OpenDetail)
        );
        assert_eq!(
            key_to_action(key(KeyCode::Char('f'), KeyModifiers::NONE)),
            Some(ListInput::Selection(PlanListAction::ToggleFilter))
        );
        assert_eq!(
            key_to_action(key(KeyCode::Char('/'), KeyModifiers::NONE)),
            Some(ListInput::StartSearch)
        );
    }

    #[test]
    fn list_offset_survives_detail_return_until_selection_needs_visibility() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::SelectResource(2));
        let mut list_state = ListState::default();

        render_to_buffer_with_state(&state, 80, 11, &mut list_state);
        let original_offset = list_state.offset();
        assert!(original_offset > 0);

        state.apply(PlanListAction::SelectResource(3));
        render_to_buffer_with_state(&state, 80, 11, &mut list_state);
        assert_eq!(list_state.offset(), original_offset);

        state.apply(PlanListAction::SelectResource(0));
        render_to_buffer_with_state(&state, 80, 11, &mut list_state);
        assert!(list_state.offset() < original_offset);
    }

    #[test]
    fn search_input_treats_regular_shortcut_keys_as_search_text() {
        let key = |code, modifiers| KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        assert_eq!(
            search_key_to_input(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(SearchInput::Insert('q'))
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Char('f'), KeyModifiers::NONE)),
            Some(SearchInput::Insert('f'))
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Char('y'), KeyModifiers::NONE)),
            Some(SearchInput::Insert('y'))
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(SearchInput::Quit)
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(SearchInput::Delete)
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(SearchInput::Confirm)
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(SearchInput::Cancel)
        );
    }

    #[test]
    fn search_render_shows_query_and_explicitly_reports_no_matches() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("AWS_S3".to_owned()));
        let text = buffer_text(&render_to_buffer(&state, 100, 16));

        assert!(text.contains("Filter: All  Showing 1/4"), "{text}");
        assert!(text.contains("/ AWS_S3_"), "{text}");
        assert!(
            text.contains("Type to search  Enter confirm  Esc cancel  Ctrl-C quit"),
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
            text.contains("No matching resources. Search: missing"),
            "{text}"
        );
    }

    #[test]
    fn search_at_minimum_width_keeps_summary_and_showing_counts() {
        let mut state = synthetic_state();
        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("security".to_owned()));
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(text.contains("Needs review: 2 / 4"), "{text}");
        assert!(text.contains("Showing 1/4"), "{text}");
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
        assert!(text.contains("Needs review: 1 / 1"), "{text}");
        assert!(text.contains("Showing 0/1"), "{text}");
        assert!(
            text.contains("No matching resources. Search: missing"),
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

        let detail = super::super::resource_detail::ResourceDetailState::from_list(&state)
            .expect("the filtered search result should open details");
        assert_eq!(detail.item_index(), 0);
        assert_eq!(detail.total_items(), 1);
        assert_eq!(state.filter(), PlanListFilter::NeedsReview);
        assert_eq!(state.search(), "worker");
    }

    #[test]
    fn filter_shows_only_review_items_and_keeps_plan_counts() {
        let mut state = synthetic_state();

        state.apply(PlanListAction::ToggleFilter);
        let text = buffer_text(&render_to_buffer(&state, 100, 16));

        assert!(text.contains("Review 2/4"), "{text}");
        assert!(text.contains("Filter: Needs review"), "{text}");
        assert!(text.contains("Showing 2/4"), "{text}");
        assert!(!text.contains("aws_instance.api"), "{text}");
        assert!(text.contains("aws_instance.worker"), "{text}");

        let detail = super::super::resource_detail::ResourceDetailState::from_list(&state)
            .expect("filtered selection should open details");
        assert_eq!(detail.item_index(), 0);
        assert_eq!(detail.total_items(), 2);
        assert_eq!(state.filter(), PlanListFilter::NeedsReview);
    }

    #[test]
    fn empty_filter_explains_how_to_restore_nonempty_plan() {
        let mut state = direct_only_state();
        state.apply(PlanListAction::ToggleFilter);
        let text = buffer_text(&render_to_buffer(&state, 80, 12));

        assert!(
            text.contains("No items in this filter. Press f to show all 1 changes."),
            "{text}"
        );
        assert!(text.contains("Showing 0/1"), "{text}");
    }

    #[test]
    fn filtered_context_keeps_filter_summary_at_minimum_size() {
        let mut state = connected_state();
        state.apply(PlanListAction::ToggleFilter);
        let text = buffer_text(&render_to_buffer(&state, MIN_WIDTH, MIN_HEIGHT));

        assert!(text.contains("Filter: Needs review"), "{text}");
        assert!(text.contains("Showing 1/1"), "{text}");
        assert!(
            text.contains("j/k/↑↓ select  f filter  / search  q/Ctrl-C"),
            "{text}"
        );
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
            "working tree vs HEAD",
        )
        .expect("direct-only fixture should build a list")
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
            "working tree vs HEAD",
        )
        .expect("unsupported changes do not require list rows");
        let text = buffer_text(&render_to_buffer(&state, 80, 12));

        assert!(text.contains("Unshown changes: output (1)"));
    }
}
