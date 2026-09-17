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
use crate::app::plan_list::{PlanListAction, PlanListItem, PlanListState};
use crate::app::source_location::{
    ResourceAddress, ResourceSourceLocation, SourceFileAnalysis, SourceRange, SourceSide,
};

const MIN_HEIGHT: u16 = 11;
const MIN_WIDTH: u16 = 48;

/// Runs the development-only plan list with synthetic plan and attribution data.
///
/// # Errors
///
/// Returns an I/O error when terminal drawing or input handling fails.
pub fn run_synthetic() -> io::Result<()> {
    let mut state = synthetic_state();
    ratatui::run(|terminal| run_plan_list(terminal, &mut state))
}

fn run_plan_list(terminal: &mut DefaultTerminal, state: &mut PlanListState) -> io::Result<()> {
    loop {
        terminal.draw(|frame| render_plan_list(frame, state))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.is_press()
        {
            match key_to_action(key) {
                Some(ListInput::Quit) => return Ok(()),
                Some(ListInput::Selection(action)) => state.apply(action),
                None => {}
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListInput {
    Selection(PlanListAction),
    Quit,
}

fn key_to_action(key: KeyEvent) -> Option<ListInput> {
    if matches!(key.code, KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return Some(ListInput::Quit);
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            Some(ListInput::Selection(PlanListAction::SelectPrevious))
        }
        KeyCode::Down | KeyCode::Char('j') => {
            Some(ListInput::Selection(PlanListAction::SelectNext))
        }
        _ => None,
    }
}

fn render_plan_list(frame: &mut Frame<'_>, state: &PlanListState) {
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

    let unsupported_summary = state.unsupported_summary();
    let unsupported_height = u16::from(unsupported_summary.is_some());
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(unsupported_height),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(content_area);

    frame.render_widget(
        Paragraph::new(Line::from(format!("compare {}", state.comparison()))),
        chunks[0],
    );
    frame.render_widget(Paragraph::new(summary_lines(state)), chunks[1]);
    frame.render_widget(separator(chunks[2].width), chunks[2]);

    if let Some(summary) = unsupported_summary {
        frame.render_widget(Paragraph::new(summary), chunks[3]);
    } else {
        frame.render_widget(Paragraph::new(""), chunks[3]);
    }

    render_rows(frame, state, chunks[4]);
    frame.render_widget(Paragraph::new(footer_line()), chunks[5]);
}

fn render_terminal_too_small(frame: &mut Frame<'_>, area: Rect) {
    let message = Paragraph::new("Terminal too small. Resize or press q to quit.")
        .style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(message, area);
}

fn render_rows(frame: &mut Frame<'_>, state: &PlanListState, area: Rect) {
    if state.items().is_empty() {
        frame.render_widget(Paragraph::new("No resource changes."), area);
        return;
    }

    let header = Line::from(vec![
        Span::styled("ACTION", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  RESOURCE"),
        Span::raw("  GIT"),
    ]);
    let items = state
        .items()
        .iter()
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
    let mut list_state = ListState::default().with_selected(Some(state.selected()));
    frame.render_stateful_widget(list, area, &mut list_state);
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
    vec![
        Line::from(vec![
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
        ]),
        Line::from(Span::styled(
            format!(
                "Needs review: {} / {}",
                state.needs_review_count(),
                state.items().len()
            ),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
    ]
}

fn footer_line() -> Line<'static> {
    Line::from("Up/Down/j/k select   q/Ctrl-C quit")
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
    use crossterm::event::{KeyEventKind, KeyEventState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};

    use super::*;

    fn render_to_buffer(state: &PlanListState, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| render_plan_list(frame, state))
            .expect("test frame should render");
        terminal.backend().buffer().clone()
    }

    fn buffer_text(buffer: &Buffer) -> String {
        let area = buffer.area();
        let mut lines = Vec::new();
        for y in area.y..area.bottom() {
            let line = (area.x..area.right())
                .filter_map(|x| buffer.cell((x, y)))
                .map(Cell::symbol)
                .collect::<String>();
            lines.push(line.trim_end().to_owned());
        }
        lines.join("\n")
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
        assert!(text.contains("Up/Down/j/k select   q/Ctrl-C quit"));
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
        assert_eq!(state.selected(), 1);
        assert_eq!(
            key_to_action(key(KeyCode::Up, KeyModifiers::NONE)),
            Some(ListInput::Selection(PlanListAction::SelectPrevious))
        );
        state.apply(PlanListAction::SelectPrevious);
        assert_eq!(state.selected(), 0);
        assert_eq!(
            key_to_action(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(ListInput::Quit)
        );
        assert_eq!(
            key_to_action(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(ListInput::Quit)
        );
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
