use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::execution::{Diagnostic, DiagnosticSeverity, ResourceEventKind, ResourceProgress};
use crate::app::execution::{ExecutionStage, ExecutionState};
use crate::ui::primitives::atoms::separator;
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{footer, header};

use super::ExecutionViewState;

const MIN_HEIGHT: u16 = 11;
const MIN_WIDTH: u16 = 48;
const STATUS_HEIGHT: u16 = 3;
const SEPARATOR_HEIGHT: u16 = 1;

#[cfg(test)]
pub(crate) fn render_execution(frame: &mut Frame<'_>, state: &ExecutionState, now: Instant) {
    let view = ExecutionViewState::from_state(state);
    render_execution_with_view(frame, state, view, now);
}

pub(crate) fn render_execution_with_view(
    frame: &mut Frame<'_>,
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
) {
    let area = frame.area();
    let layout = execution_layout(area, state);
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT || layout.body().height == 0 {
        let message = if state.stage() == ExecutionStage::Failed {
            "Terminal too small. Resize or press q to quit."
        } else {
            "Terminal too small. Resize or press Ctrl-C to cancel."
        };
        terminal_notice::render_wrapped(frame, area, message);
        return;
    }

    let block = Block::new()
        .borders(Borders::ALL)
        .title(format!("Terracotta / {}", state.stage().title()));
    frame.render_widget(block, area);

    let chunks = &layout.chunks;

    header::render(
        frame,
        chunks[0],
        wrapped_lines(&context_lines(state), chunks[0].width),
    );
    frame.render_widget(
        Paragraph::new(status_lines_with_view(state, view, now)),
        chunks[1],
    );

    let lines = wrapped_lines(&execution_lines(state), chunks[2].width);
    let paragraph = Paragraph::new(lines.clone());
    let visible_height = usize::from(chunks[2].height);
    let max_scroll = lines.len().saturating_sub(visible_height);
    let max_scroll = u16::try_from(max_scroll).unwrap_or(u16::MAX);
    let scroll = if view.follows_latest() {
        max_scroll
    } else {
        view.scroll().min(max_scroll)
    };
    frame.render_widget(paragraph.scroll((scroll, 0)), chunks[2]);
    frame.render_widget(separator::render(chunks[3].width), chunks[3]);
    if let Some(notice) = state.copy_notice() {
        frame.render_widget(Paragraph::new(notice.message()), chunks[4]);
    }
    footer::render(frame, chunks[5], layout.footer_lines);
}

pub(crate) struct ExecutionLayout {
    chunks: Vec<Rect>,
    footer_lines: Vec<Line<'static>>,
}

impl ExecutionLayout {
    pub(crate) fn body(&self) -> Rect {
        self.chunks[2]
    }
}

pub(crate) fn execution_layout(area: Rect, state: &ExecutionState) -> ExecutionLayout {
    let content_area = Block::new().borders(Borders::ALL).inner(area);
    let mut footer_lines = footer_lines(state, content_area.width);
    let context_height =
        u16::try_from(wrapped_lines(&context_lines(state), content_area.width).len())
            .unwrap_or(u16::MAX)
            .max(1);
    let copy_notice_height = u16::from(state.copy_notice().is_some());
    let required_height = usize::from(context_height)
        + usize::from(STATUS_HEIGHT)
        + 1
        + usize::from(SEPARATOR_HEIGHT)
        + usize::from(copy_notice_height);
    if required_height + footer_lines.len() > usize::from(content_area.height) {
        footer_lines = required_footer_lines(state, content_area.width);
    }
    let split = |footer_height: usize| {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(context_height),
                Constraint::Length(STATUS_HEIGHT),
                Constraint::Min(1),
                Constraint::Length(SEPARATOR_HEIGHT),
                Constraint::Length(copy_notice_height),
                Constraint::Length(u16::try_from(footer_height).unwrap_or(u16::MAX).max(1)),
            ])
            .split(content_area)
            .to_vec()
    };
    let mut chunks = split(footer_lines.len());
    if chunks[2].height == 0 {
        footer_lines = required_footer_lines(state, content_area.width);
        chunks = split(footer_lines.len());
    }
    ExecutionLayout {
        chunks,
        footer_lines,
    }
}

pub(crate) fn execution_chunks(area: Rect, state: &ExecutionState) -> Vec<Rect> {
    execution_layout(area, state).chunks
}

pub(crate) fn execution_scroll_position_with_view(
    state: &ExecutionState,
    view: ExecutionViewState,
    body: Rect,
) -> (u16, u16) {
    let lines = wrapped_lines(&execution_lines(state), body.width);
    let visible_height = usize::from(body.height);
    let max_scroll = u16::try_from(lines.len().saturating_sub(visible_height)).unwrap_or(u16::MAX);
    let current_scroll = if view.follows_latest() {
        max_scroll
    } else {
        view.scroll().min(max_scroll)
    };
    (current_scroll, max_scroll)
}

fn status_lines_with_view(
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
) -> Vec<Line<'static>> {
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
                if view.follows_latest() { "On" } else { "Off" },
                if view.follows_latest() {
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

fn context_lines(state: &ExecutionState) -> Vec<String> {
    vec![
        format!(
            "cwd {}   workspace {}",
            state.context().cwd().as_str(),
            state.context().workspace().as_str(),
        ),
        format!(
            "git {}   compare {}",
            state.context().git().as_str(),
            state.context().comparison().as_str(),
        ),
    ]
}

fn execution_lines(state: &ExecutionState) -> Vec<String> {
    let mut lines = resource_lines(state.progress().resources());
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

fn resource_lines<'a>(resources: impl Iterator<Item = (&'a str, ResourceProgress)>) -> Vec<String> {
    resources
        .map(|(address, progress)| {
            format!(
                "  [{}] {:<36} {}",
                resource_status(progress.kind),
                address,
                resource_label(progress.kind)
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
            let source = Line::from(line.as_str());
            let mut wrapped = Vec::new();
            let mut current = String::new();
            let mut current_width = 0;
            for grapheme in source.styled_graphemes(Style::default()) {
                let grapheme_width = Line::from(grapheme.symbol).width();
                if grapheme_width > 0 && current_width > 0 && current_width + grapheme_width > width
                {
                    wrapped.push(Line::from(std::mem::take(&mut current)));
                    current_width = 0;
                }
                current.push_str(grapheme.symbol);
                current_width += grapheme_width;
            }
            if current.is_empty() {
                wrapped.push(Line::from(""));
            } else {
                wrapped.push(Line::from(current));
            }
            wrapped
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

fn footer_lines(state: &ExecutionState, width: u16) -> Vec<Line<'static>> {
    let items = if state.stage() == ExecutionStage::Failed {
        vec![
            Line::from("q quit"),
            Line::from("↑/↓ PgUp/PgDn scroll"),
            Line::from("y diagnostic"),
            Line::from("Y result"),
        ]
    } else {
        vec![
            Line::from("Ctrl-C cancel"),
            Line::from("↑/↓ PgUp/PgDn scroll"),
            Line::from("End follow latest"),
        ]
    };
    footer::layout(items, width)
}

fn required_footer_lines(state: &ExecutionState, width: u16) -> Vec<Line<'static>> {
    let items = if state.stage() == ExecutionStage::Failed {
        vec![Line::from("q quit"), Line::from("↑/↓ PgUp/PgDn scroll")]
    } else {
        vec![
            Line::from("Ctrl-C cancel"),
            Line::from("↑/↓ PgUp/PgDn scroll"),
        ]
    };
    footer::layout(items, width)
}

#[cfg(test)]
mod tests {
    use crate::ui::test_support::buffer_text;
    use crate::ui::test_support::render_to_buffer as render_test_buffer;
    use ratatui::buffer::Buffer;

    use crate::app::copy::{CopyNotice, CopyTarget};
    use crate::app::execution::{
        ExecutionAction, ExecutionContext, ExecutionEvent, ExecutionEventKind, ResourceEvent,
    };
    use crate::ui::features::execution::ExecutionScroll;

    use super::*;
    use crate::app::execution::{
        DiagnosticPoint, DiagnosticPosition, DiagnosticSource, ExecutionPhase, ProcessExitStatus,
        ProcessTermination,
    };

    include!("tests/render_snapshots.rs");

    fn event(received_at: Instant, kind: ExecutionEventKind) -> ExecutionEvent {
        ExecutionEvent { received_at, kind }
    }

    fn render_to_buffer(state: &ExecutionState, now: Instant, width: u16, height: u16) -> Buffer {
        render_test_buffer((width, height), |frame| render_execution(frame, state, now))
    }

    fn render_to_buffer_with_view(
        state: &ExecutionState,
        view: ExecutionViewState,
        now: Instant,
        width: u16,
        height: u16,
    ) -> Buffer {
        render_test_buffer((width, height), |frame| {
            render_execution_with_view(frame, state, view, now);
        })
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
    fn renders_execution_context_for_known_and_unavailable_values() {
        let started_at = Instant::now();
        let state = ExecutionState::with_context(
            started_at,
            ExecutionContext::known(
                "infra/prod",
                "default",
                "feature/plan-ui",
                "working tree vs HEAD",
            ),
        );
        let text = buffer_text(&render_to_buffer(&state, started_at, 80, 16));

        assert!(text.contains("cwd infra/prod"), "{text}");
        assert!(text.contains("workspace default"), "{text}");
        assert!(text.contains("git feature/plan-ui"), "{text}");
        assert!(text.contains("compare working tree vs HEAD"), "{text}");

        let long_context = ExecutionState::with_context(
            started_at,
            ExecutionContext::known(
                "/Users/example/terraform/infrastructure/production/networking",
                "workspace-with-a-long-name",
                "feature/long-running-execution-screen",
                "release/2026-09-17 vs working tree",
            ),
        );
        let long_context_text = buffer_text(&render_to_buffer(&long_context, started_at, 80, 30));
        assert!(
            long_context_text.contains("cwd /Users/example/terraform"),
            "{long_context_text}"
        );
        assert!(
            long_context_text.contains("workspace-with-a-long-name"),
            "{long_context_text}"
        );
        assert!(
            long_context_text.contains("feature/long-running-execution-screen"),
            "{long_context_text}"
        );
        assert!(
            long_context_text.contains("compare release/2026-09-17 vs work"),
            "{long_context_text}"
        );
        assert!(
            long_context_text.contains("ing tree"),
            "{long_context_text}"
        );
        assert!(
            long_context_text.contains("Elapsed 0.0s"),
            "{long_context_text}"
        );
        assert!(
            long_context_text.contains("Ctrl-C cancel"),
            "{long_context_text}"
        );

        let unavailable = ExecutionState::with_context(started_at, ExecutionContext::unavailable());
        let unavailable_text = buffer_text(&render_to_buffer(&unavailable, started_at, 80, 16));
        assert!(
            unavailable_text.contains("cwd unavailable"),
            "{unavailable_text}"
        );
        assert!(
            unavailable_text.contains("compare unavailable"),
            "{unavailable_text}"
        );
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
    fn renders_copy_notices_on_their_own_row_before_running_footer() {
        let cases = [
            (
                "success",
                CopyNotice::Copied {
                    target: CopyTarget::Result,
                    resource_count: 1,
                },
                "Copied result (redacted).",
            ),
            (
                "failure",
                CopyNotice::Failed,
                "Copy failed: clipboard unavailable.",
            ),
        ];

        for (name, notice, expected) in cases {
            let started_at = Instant::now();
            let mut state = ExecutionState::new(started_at);
            state.set_copy_notice(notice);
            let text = buffer_text(&render_to_buffer(&state, started_at, 48, 12));

            assert!(text.contains(expected), "case: {name}\n{text}");
            assert!(text.contains("Ctrl-C cancel"), "case: {name}\n{text}");
            assert!(
                text.contains("↑/↓ PgUp/PgDn scroll"),
                "case: {name}\n{text}"
            );
        }
    }

    #[test]
    fn execution_widths_keep_active_exit_and_page_scroll_hints() {
        for width in [48, 60, 80, 120] {
            let started_at = Instant::now();
            let state = ExecutionState::new(started_at);
            let text = buffer_text(&render_to_buffer(&state, started_at, width, 20));

            assert!(text.contains("Ctrl-C cancel"), "width: {width}\n{text}");
            assert!(
                text.contains("↑/↓ PgUp/PgDn scroll"),
                "width: {width}\n{text}"
            );
        }
    }

    #[test]
    fn execution_layout_reserves_copy_notice_row_for_scroll_body() {
        let started_at = Instant::now();
        let area = Rect::new(0, 0, 48, 12);
        let mut state = ExecutionState::new(started_at);
        for index in 0..40 {
            state.record(resource_event(
                started_at,
                &format!("aws_instance.item[{index}]"),
                ResourceEventKind::RefreshStart,
            ));
        }

        let without_notice = execution_layout(area, &state).body();
        state.set_copy_notice(CopyNotice::Failed);
        let with_notice = execution_layout(area, &state).body();
        assert_eq!(without_notice.height, with_notice.height + 1);

        let mut view = ExecutionViewState::from_state(&state);
        let (current, max) = execution_scroll_position_with_view(&state, view, with_notice);
        assert_eq!(current, max);
        view.apply_scroll(ExecutionScroll::PageUp, current, max, with_notice.height);
        assert_eq!(view.scroll(), current.saturating_sub(with_notice.height));
        view.apply_scroll(
            ExecutionScroll::PageDown,
            view.scroll(),
            max,
            with_notice.height,
        );
        assert_eq!(
            view.scroll(),
            (current.saturating_sub(with_notice.height))
                .saturating_add(with_notice.height)
                .min(max)
        );
    }

    #[test]
    fn renders_failed_stage_with_long_diagnostic_and_quit_footer() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);
        let diagnostic_detail = "日本語の診断文と絵文字🙂を含む長い内容。".repeat(8);
        state.record(event(
            started_at + Duration::from_secs(1),
            ExecutionEventKind::Diagnostic(Diagnostic {
                severity: DiagnosticSeverity::Error,
                summary: "Terraform initialization required".to_owned(),
                detail: Some(diagnostic_detail.clone()),
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
        state.record(event(
            started_at + Duration::from_secs(2),
            ExecutionEventKind::Terminated(ProcessTermination {
                status: ProcessExitStatus::Exited(1),
                interrupted: false,
            }),
        ));
        assert_eq!(state.progress().diagnostics().len(), 1);
        assert_eq!(
            state.progress().diagnostics()[0].summary,
            "Terraform initialization required"
        );
        assert_eq!(
            state.progress().diagnostics()[0].detail.as_deref(),
            Some(diagnostic_detail.as_str())
        );
        let text = buffer_text(&render_to_buffer(
            &state,
            started_at + Duration::from_secs(2),
            60,
            30,
        ));

        assert!(text.contains("Terracotta / Failed"), "{text}");
        assert!(text.contains("Terraform plan failed."), "{text}");
        assert!(text.contains("Terraform initialization required"), "{text}");
        let compact = text.replace(' ', "");
        assert!(compact.contains("日本語の診断文"), "{text}");
        assert!(compact.contains("絵文字🙂"), "{text}");
        assert!(text.contains("at infra/prod/main.tf:12:3"), "{text}");
        assert!(text.contains("q quit"), "{text}");
    }

    #[test]
    fn renders_reading_matching_and_cancelling_statuses() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);

        state.record(event(
            started_at,
            ExecutionEventKind::Phase(ExecutionPhase::Reading),
        ));
        let reading = buffer_text(&render_to_buffer(&state, started_at, 80, 16));
        assert!(reading.contains("Reading plan..."), "{reading}");

        state.record(event(
            started_at,
            ExecutionEventKind::Phase(ExecutionPhase::Matching),
        ));
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

        let mut view = ExecutionViewState::from_state(&state);
        view.apply_scroll(ExecutionScroll::Down, 0, 1, 1);
        let stopped = buffer_text(&render_to_buffer_with_view(
            &state, view, started_at, 80, 16,
        ));
        assert!(stopped.contains("Follow: Off"), "{stopped}");

        view.end();
        let resumed = buffer_text(&render_to_buffer_with_view(
            &state, view, started_at, 80, 16,
        ));
        assert!(resumed.contains("Follow: On"), "{resumed}");
    }

    #[test]
    fn scrolling_from_latest_uses_the_rendered_bottom_and_stays_put_on_new_events() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);
        for index in 0..30 {
            state.record(resource_event(
                started_at,
                &format!("aws_instance.item[{index}]"),
                ResourceEventKind::RefreshStart,
            ));
        }

        let area = Rect::new(0, 0, 80, 16);
        let body = execution_chunks(area, &state)[2];
        let mut view = ExecutionViewState::from_state(&state);
        let (current_offset, max_offset) = execution_scroll_position_with_view(&state, view, body);
        assert!(current_offset > 0);
        assert_eq!(current_offset, max_offset);

        view.apply_scroll(ExecutionScroll::Up, current_offset, max_offset, body.height);
        assert!(!view.follows_latest());
        let stopped_offset = view.scroll();
        assert_eq!(stopped_offset, current_offset - 1);

        state.record(resource_event(
            started_at,
            "aws_instance.new",
            ResourceEventKind::RefreshStart,
        ));
        let body = execution_chunks(area, &state)[2];
        let (new_offset, new_max_offset) = execution_scroll_position_with_view(&state, view, body);
        assert_eq!(new_offset, stopped_offset);
        assert!(new_max_offset > max_offset);

        view.end();
        let body = execution_chunks(area, &state)[2];
        let (follow_offset, follow_max_offset) =
            execution_scroll_position_with_view(&state, view, body);
        assert!(view.follows_latest());
        assert_eq!(follow_offset, follow_max_offset);
    }

    #[test]
    fn narrow_running_and_failed_screens_show_their_allowed_exit_guidance() {
        let started_at = Instant::now();
        let state = ExecutionState::new(started_at);
        let minimum = buffer_text(&render_to_buffer(&state, started_at, MIN_WIDTH, MIN_HEIGHT));
        assert!(minimum.contains("Ctrl-C cancel"), "{minimum}");

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
        failed.record(event(
            started_at,
            ExecutionEventKind::Terminated(ProcessTermination {
                status: ProcessExitStatus::Exited(1),
                interrupted: false,
            }),
        ));
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
}
