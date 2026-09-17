use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::app::execution::{ExecutionAction, ExecutionStage, ExecutionState};
use crate::app::progress::{
    Diagnostic, DiagnosticSeverity, ExecutionEvent, ExecutionEventKind, ResourceEvent,
    ResourceEventKind,
};

const MIN_HEIGHT: u16 = 11;
const MIN_WIDTH: u16 = 48;

pub(super) fn run_synthetic_execution() -> io::Result<()> {
    let started_at = Instant::now();
    let mut state = ExecutionState::new(started_at);
    state.record(ExecutionEvent {
        received_at: started_at,
        kind: ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::RefreshComplete,
        }),
    });
    state.record(ExecutionEvent {
        received_at: started_at + Duration::from_millis(400),
        kind: ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_instance.api".to_owned(),
            kind: ResourceEventKind::RefreshStart,
        }),
    });
    ratatui::run(|terminal| run_execution(terminal, &mut state))
}

fn run_execution(terminal: &mut DefaultTerminal, state: &mut ExecutionState) -> io::Result<()> {
    loop {
        terminal.draw(|frame| render_execution(frame, state, Instant::now()))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.is_press()
        {
            match execution_key_to_input(key, state.stage()) {
                Some(ExecutionInput::Quit) => return Ok(()),
                Some(ExecutionInput::Action(action)) => state.apply(action),
                None => {}
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecutionInput {
    Action(ExecutionAction),
    Quit,
}

fn execution_key_to_input(key: KeyEvent, stage: ExecutionStage) -> Option<ExecutionInput> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(if stage == ExecutionStage::Failed {
            ExecutionInput::Quit
        } else {
            ExecutionInput::Action(ExecutionAction::RequestCancellation)
        });
    }

    if stage == ExecutionStage::Failed && key.code == KeyCode::Char('q') {
        return Some(ExecutionInput::Quit);
    }

    let action = match key.code {
        KeyCode::Up | KeyCode::Char('k') => ExecutionAction::ScrollUp,
        KeyCode::Down | KeyCode::Char('j') => ExecutionAction::ScrollDown,
        KeyCode::PageUp => ExecutionAction::PageUp,
        KeyCode::PageDown => ExecutionAction::PageDown,
        KeyCode::End => ExecutionAction::End,
        _ => return None,
    };
    Some(ExecutionInput::Action(action))
}

fn render_execution(frame: &mut Frame<'_>, state: &ExecutionState, now: Instant) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_too_small(frame, area, state.stage());
        return;
    }

    let block = Block::new()
        .borders(Borders::ALL)
        .title(format!("Terracotta / {}", state.stage().title()));
    let content_area = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(content_area);

    frame.render_widget(Paragraph::new(status_lines(state, now)), chunks[0]);

    let lines = wrapped_lines(&execution_lines(state), chunks[1].width);
    let paragraph = Paragraph::new(lines.clone());
    let visible_height = usize::from(chunks[1].height);
    let max_scroll = lines.len().saturating_sub(visible_height);
    let max_scroll = u16::try_from(max_scroll).unwrap_or(u16::MAX);
    let scroll = if state.follows_latest() {
        max_scroll
    } else {
        state.scroll().min(max_scroll)
    };
    frame.render_widget(paragraph.scroll((scroll, 0)), chunks[1]);
    frame.render_widget(separator(chunks[2].width), chunks[2]);
    frame.render_widget(Paragraph::new(footer_line(state.stage())), chunks[3]);
}

fn render_too_small(frame: &mut Frame<'_>, area: Rect, stage: ExecutionStage) {
    let message = if stage == ExecutionStage::Failed {
        "Terminal too small. Resize or press q to quit."
    } else {
        "Terminal too small. Resize or press Ctrl-C to cancel."
    };
    frame.render_widget(
        Paragraph::new(message)
            .wrap(Wrap { trim: false })
            .style(Style::default().add_modifier(Modifier::BOLD)),
        area,
    );
}

fn status_lines(state: &ExecutionState, now: Instant) -> Vec<Line<'static>> {
    let status = if state.is_cancelling() {
        "Cancelling...".to_owned()
    } else {
        match state.stage() {
            ExecutionStage::Planning => {
                format!("{} Processing", spinner(state.elapsed_at(now)))
            }
            ExecutionStage::Reading => "Reading plan...".to_owned(),
            ExecutionStage::Matching => "Matching Git changes...".to_owned(),
            ExecutionStage::Failed => "Terraform plan failed.".to_owned(),
        }
    };
    let waiting = if state.stage() == ExecutionStage::Failed {
        String::new()
    } else {
        format!(
            "Waiting for Terraform... {}s",
            state.waiting_at(now).as_secs()
        )
    };
    vec![
        Line::from(vec![
            Span::raw(status),
            Span::raw("    Follow: "),
            Span::styled(
                if state.follows_latest() { "On" } else { "Off" },
                if state.follows_latest() {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Yellow)
                },
            ),
        ]),
        Line::from(waiting),
        Line::from(format!("Elapsed {}", format_elapsed(state.elapsed_at(now)))),
    ]
}

fn execution_lines(state: &ExecutionState) -> Vec<String> {
    let mut lines = resource_lines(state.progress().events());
    if lines.is_empty() {
        lines.push("  No Terraform events yet.".to_owned());
    }

    let diagnostics = state.progress().diagnostics();
    if !diagnostics.is_empty() {
        lines.push(String::new());
        lines.push(format!("  Diagnostics ({})", diagnostics.len()));
        for diagnostic in diagnostics {
            append_diagnostic(&mut lines, diagnostic);
        }
    }

    if state.stage() == ExecutionStage::Failed {
        lines.push(String::new());
        lines.push("  Review result is unavailable.".to_owned());
    }
    lines
}

fn resource_lines(events: &[ExecutionEvent]) -> Vec<String> {
    let mut resources = Vec::new();
    for event in events {
        let ExecutionEventKind::Resource(resource) = &event.kind else {
            continue;
        };
        if let Some((_, kind)) = resources
            .iter_mut()
            .find(|(address, _)| address == &resource.address)
        {
            *kind = resource.kind;
        } else {
            resources.push((resource.address.clone(), resource.kind));
        }
    }
    resources
        .into_iter()
        .map(|(address, kind)| {
            format!(
                "  [{}] {:<36} {}",
                resource_status(kind),
                address,
                resource_label(kind)
            )
        })
        .collect()
}

fn append_diagnostic(lines: &mut Vec<String>, diagnostic: &Diagnostic) {
    lines.push(format!(
        "    {}: {}",
        diagnostic_severity(diagnostic.severity),
        diagnostic.summary
    ));
    if let Some(detail) = &diagnostic.detail {
        for line in detail.lines() {
            lines.push(format!("      {line}"));
        }
    }
    if let Some(position) = &diagnostic.position {
        lines.push(format!(
            "      at {}:{}:{}",
            position.filename, position.start.line, position.start.column
        ));
    }
}

fn wrapped_lines(lines: &[String], width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .flat_map(|line| {
            if line.is_empty() {
                return vec![Line::from("")];
            }
            let chars = line.chars().collect::<Vec<_>>();
            chars
                .chunks(width)
                .map(|chunk| Line::from(chunk.iter().collect::<String>()))
                .collect::<Vec<_>>()
        })
        .collect()
}

const fn resource_status(kind: ResourceEventKind) -> &'static str {
    match kind {
        ResourceEventKind::ApplyErrored
        | ResourceEventKind::ProvisionErrored
        | ResourceEventKind::EphemeralErrored => "failed",
        kind if kind.is_complete() => "done",
        ResourceEventKind::ResourceDrift | ResourceEventKind::PlannedChange => "info",
        _ => "run",
    }
}

const fn resource_label(kind: ResourceEventKind) -> &'static str {
    match kind {
        ResourceEventKind::RefreshStart => "Refreshing",
        ResourceEventKind::RefreshComplete => "Refresh complete",
        ResourceEventKind::ApplyStart | ResourceEventKind::ApplyProgress => "Applying",
        ResourceEventKind::ApplyComplete => "Apply complete",
        ResourceEventKind::ApplyErrored => "Apply failed",
        ResourceEventKind::ProvisionStart | ResourceEventKind::ProvisionProgress => "Provisioning",
        ResourceEventKind::ProvisionComplete => "Provision complete",
        ResourceEventKind::ProvisionErrored => "Provision failed",
        ResourceEventKind::ImportStart => "Importing",
        ResourceEventKind::ImportComplete => "Import complete",
        ResourceEventKind::EphemeralStart | ResourceEventKind::EphemeralProgress => {
            "Running ephemeral operation"
        }
        ResourceEventKind::EphemeralComplete => "Ephemeral operation complete",
        ResourceEventKind::EphemeralErrored => "Ephemeral operation failed",
        ResourceEventKind::ResourceDrift => "Drift detected",
        ResourceEventKind::PlannedChange => "Planned change",
    }
}

const fn diagnostic_severity(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Unknown => "diagnostic",
    }
}

fn spinner(elapsed: Duration) -> char {
    let index = usize::try_from((elapsed.as_millis() / 250) % 4).unwrap_or(0);
    ['|', '/', '-', '\\'][index]
}

fn format_elapsed(elapsed: Duration) -> String {
    format!(
        "{}.{:01}s",
        elapsed.as_secs(),
        elapsed.subsec_millis() / 100
    )
}

fn separator(width: u16) -> Paragraph<'static> {
    Paragraph::new("─".repeat(usize::from(width))).style(Style::default().fg(Color::DarkGray))
}

fn footer_line(stage: ExecutionStage) -> &'static str {
    if stage == ExecutionStage::Failed {
        "Up/Down/PageUp/PageDown scroll   q/Ctrl-C quit"
    } else {
        "Up/Down/PageUp/PageDown scroll   End follow latest   Ctrl-C cancel"
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};

    use super::super::super::app::progress::{
        DiagnosticPoint, DiagnosticPosition, DiagnosticSource,
    };
    use super::*;

    fn event(received_at: Instant, kind: ExecutionEventKind) -> ExecutionEvent {
        ExecutionEvent { received_at, kind }
    }

    fn render_to_buffer(state: &ExecutionState, now: Instant, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal should be created");
        terminal
            .draw(|frame| render_execution(frame, state, now))
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

    fn resource_event(at: Instant, address: &str, kind: ResourceEventKind) -> ExecutionEvent {
        event(
            at,
            ExecutionEventKind::Resource(ResourceEvent {
                address: address.to_owned(),
                kind,
            }),
        )
    }

    #[test]
    fn renders_spinner_elapsed_waiting_and_interleaved_resource_progress() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);
        state.record(resource_event(
            started_at + Duration::from_secs(1),
            "aws_vpc.main",
            ResourceEventKind::RefreshStart,
        ));
        state.record(resource_event(
            started_at + Duration::from_secs(2),
            "aws_instance.api",
            ResourceEventKind::RefreshStart,
        ));
        state.record(resource_event(
            started_at + Duration::from_secs(3),
            "aws_vpc.main",
            ResourceEventKind::RefreshComplete,
        ));

        let text = buffer_text(&render_to_buffer(
            &state,
            started_at + Duration::from_secs(5),
            100,
            20,
        ));

        assert!(text.contains("Terracotta / Planning"), "{text}");
        assert!(text.contains("| Processing"), "{text}");
        assert!(text.contains("Waiting for Terraform... 2s"), "{text}");
        assert!(text.contains("Elapsed 5.0s"), "{text}");
        assert!(text.contains("[done] aws_vpc.main"), "{text}");
        assert!(text.contains("[run] aws_instance.api"), "{text}");
        assert!(text.contains("Follow: On"), "{text}");
    }

    #[test]
    fn renders_no_event_state_and_keeps_copy_out_of_running_footer() {
        let started_at = Instant::now();
        let state = ExecutionState::new(started_at);
        let text = buffer_text(&render_to_buffer(
            &state,
            started_at + Duration::from_secs(2),
            80,
            16,
        ));

        assert!(text.contains("No Terraform events yet."), "{text}");
        assert!(text.contains("Waiting for Terraform... 2s"), "{text}");
        assert!(!text.to_ascii_lowercase().contains("copy"), "{text}");
    }

    #[test]
    fn renders_failed_stage_with_long_diagnostic_and_quit_footer() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);
        state.apply(ExecutionAction::SetStage(ExecutionStage::Failed));
        state.record(event(
            started_at + Duration::from_secs(1),
            ExecutionEventKind::Diagnostic(Diagnostic {
                severity: DiagnosticSeverity::Error,
                summary: "Terraform initialization required".to_owned(),
                detail: Some("detail ".repeat(40)),
                position: Some(DiagnosticPosition {
                    filename: "infra/prod/main.tf".to_owned(),
                    start: DiagnosticPoint {
                        line: 12,
                        column: 3,
                        byte: Some(100),
                    },
                    end: DiagnosticPoint {
                        line: 12,
                        column: 9,
                        byte: Some(106),
                    },
                }),
                source: DiagnosticSource::Terraform,
                raw: None,
            }),
        ));
        let text = buffer_text(&render_to_buffer(
            &state,
            started_at + Duration::from_secs(2),
            60,
            16,
        ));

        assert!(text.contains("Terracotta / Failed"), "{text}");
        assert!(text.contains("Terraform plan failed."), "{text}");
        assert!(text.contains("Terraform initialization required"), "{text}");
        assert!(text.contains("at infra/prod/main.tf:12:3"), "{text}");
        assert!(text.contains("q/Ctrl-C quit"), "{text}");
    }

    #[test]
    fn renders_reading_matching_and_cancelling_statuses() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);

        state.apply(ExecutionAction::SetStage(ExecutionStage::Reading));
        let reading = buffer_text(&render_to_buffer(&state, started_at, 80, 16));
        assert!(reading.contains("Reading plan..."), "{reading}");

        state.apply(ExecutionAction::SetStage(ExecutionStage::Matching));
        let matching = buffer_text(&render_to_buffer(&state, started_at, 80, 16));
        assert!(matching.contains("Matching Git changes..."), "{matching}");

        state.apply(ExecutionAction::RequestCancellation);
        let cancelling = buffer_text(&render_to_buffer(&state, started_at, 80, 16));
        assert!(cancelling.contains("Cancelling..."), "{cancelling}");
    }

    #[test]
    fn end_restores_following_and_scrolling_turns_it_off() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);
        state.record(resource_event(
            started_at,
            "aws_vpc.main",
            ResourceEventKind::RefreshComplete,
        ));

        state.apply(ExecutionAction::ScrollDown);
        let stopped = buffer_text(&render_to_buffer(&state, started_at, 80, 16));
        assert!(stopped.contains("Follow: Off"), "{stopped}");

        state.apply(ExecutionAction::End);
        let resumed = buffer_text(&render_to_buffer(&state, started_at, 80, 16));
        assert!(resumed.contains("Follow: On"), "{resumed}");
    }

    #[test]
    fn narrow_running_and_failed_screens_show_their_allowed_exit_guidance() {
        let started_at = Instant::now();
        let state = ExecutionState::new(started_at);
        let running = buffer_text(&render_to_buffer(
            &state,
            started_at,
            MIN_WIDTH - 1,
            MIN_HEIGHT,
        ));
        assert!(running.contains("Resize or press Ctrl-C to"), "{running}");
        assert!(running.contains("cancel."), "{running}");
        assert!(!running.contains("press q to quit"), "{running}");

        let mut failed = ExecutionState::new(started_at);
        failed.apply(ExecutionAction::SetStage(ExecutionStage::Failed));
        let failed_text = buffer_text(&render_to_buffer(
            &failed,
            started_at,
            MIN_WIDTH - 1,
            MIN_HEIGHT,
        ));
        assert!(
            failed_text.contains("Resize or press q to quit."),
            "{failed_text}"
        );
    }

    #[test]
    fn key_mapping_cancels_running_but_quits_failed() {
        use crossterm::event::{KeyEventKind, KeyEventState};

        let key = |code, modifiers| KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        assert_eq!(
            execution_key_to_input(
                key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                ExecutionStage::Planning,
            ),
            Some(ExecutionInput::Action(ExecutionAction::RequestCancellation))
        );
        assert_eq!(
            execution_key_to_input(
                key(KeyCode::Char('q'), KeyModifiers::NONE),
                ExecutionStage::Planning,
            ),
            None
        );
        assert_eq!(
            execution_key_to_input(
                key(KeyCode::Char('q'), KeyModifiers::NONE),
                ExecutionStage::Failed,
            ),
            Some(ExecutionInput::Quit)
        );
        assert_eq!(
            execution_key_to_input(
                key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                ExecutionStage::Failed,
            ),
            Some(ExecutionInput::Quit)
        );
    }
}
