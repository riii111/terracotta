use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::execution::{EventStream, ExecutionResult, ExecutionStage, ExecutionState};
use crate::ui::primitives::atoms::{scrollbar, separator};
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{footer, header, layout as shell_layout};
use crate::ui::theme;

use super::ExecutionViewState;

const MIN_HEIGHT: u16 = 9;
const MIN_WIDTH: u16 = 32;
const STATUS_HEIGHT: u16 = 3;

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

    header::render_execution(frame, layout.shell.header(), state.context());
    let content =
        shell_layout::render_content_block(frame, layout.shell.content(), state.stage().title());
    debug_assert_eq!(content, layout.shell.content_inner());
    frame.render_widget(
        Paragraph::new(status_lines(state, view, now)).style(theme::body_style()),
        layout.chunks[0],
    );

    let lines = execution_lines(state);
    let (max_vertical, max_horizontal) = scroll_limits(&lines, layout.body());
    let scroll = if view.follows_latest() {
        preferred_scroll(state, max_vertical)
    } else {
        view.scroll().min(max_vertical)
    };
    let horizontal = view.horizontal().min(max_horizontal);
    frame.render_widget(
        Paragraph::new(lines.clone())
            .style(theme::body_style())
            .scroll((scroll, horizontal)),
        layout.chunks[1],
    );
    scrollbar::render_vertical(
        frame,
        layout.chunks[1],
        lines.len(),
        usize::from(layout.body().height),
        usize::from(scroll),
    );
    scrollbar::render_horizontal(
        frame,
        layout.chunks[1],
        max_line_width(&lines),
        usize::from(layout.body().width),
        usize::from(horizontal),
    );
    frame.render_widget(separator::render(layout.chunks[2].width), layout.chunks[2]);
    if let Some(notice) = state.copy_notice() {
        frame.render_widget(
            Paragraph::new(notice.message()).style(theme::secondary_style()),
            layout.chunks[3],
        );
    }
    footer::render(
        frame,
        layout.shell.footer(),
        layout.shell.footer_lines().to_owned(),
    );
}

pub(crate) struct ExecutionLayout {
    shell: shell_layout::ShellLayout,
    chunks: Vec<Rect>,
}

impl ExecutionLayout {
    pub(crate) fn body(&self) -> Rect {
        let body = self.chunks[1];
        Rect::new(
            body.x,
            body.y,
            body.width.saturating_sub(1),
            body.height.saturating_sub(1),
        )
    }
}

pub(crate) fn execution_layout(area: Rect, state: &ExecutionState) -> ExecutionLayout {
    let shell_area = shell_layout::centered_area(area);
    let footer_lines = footer_lines(state, shell_area.width);
    let shell = shell_layout::layout(shell_area, footer_lines.clone(), footer_lines, 1);
    let notice_height = u16::from(state.copy_notice().is_some());
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(STATUS_HEIGHT),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(notice_height),
        ])
        .split(shell.content_inner())
        .to_vec();
    ExecutionLayout { shell, chunks }
}

pub(crate) fn execution_scroll_position_with_view(
    state: &ExecutionState,
    view: ExecutionViewState,
    body: Rect,
) -> (u16, u16) {
    let (max, _) = scroll_limits(&execution_lines(state), body);
    let current = if view.follows_latest() {
        preferred_scroll(state, max)
    } else {
        view.scroll().min(max)
    };
    (current, max)
}

pub(crate) fn execution_horizontal_scroll_position_with_view(
    state: &ExecutionState,
    view: ExecutionViewState,
    body: Rect,
) -> (u16, u16) {
    let (_, max) = scroll_limits(&execution_lines(state), body);
    (view.horizontal().min(max), max)
}

fn execution_lines(state: &ExecutionState) -> Vec<Line<'static>> {
    let log = state
        .result()
        .map_or_else(|| state.progress().log(), |result| result.log());
    let mut lines = log
        .iter()
        .flat_map(|line| {
            line.text
                .lines()
                .map(|text| {
                    let style = if line.stream == EventStream::Stderr {
                        theme::warning_style()
                    } else {
                        theme::body_style()
                    };
                    Line::from(Span::styled(text.to_owned(), style))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if let Some(summary) = state.result().and_then(|result| result.summary_line()) {
        lines.push(Line::default());
        lines.push(Line::from(summary.to_owned()));
    }
    if lines.is_empty() {
        vec![Line::from("Waiting for Terraform output...")]
    } else {
        lines
    }
}

fn status_lines(
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
) -> Vec<Line<'static>> {
    let status = if state.is_cancelling() {
        "Stopping...".to_owned()
    } else {
        match state.stage() {
            ExecutionStage::Initializing => "Initializing...".to_owned(),
            ExecutionStage::Planning => "Planning...".to_owned(),
            ExecutionStage::Reading => "Reading plan...".to_owned(),
            #[cfg(test)]
            ExecutionStage::Matching => "Matching Git...".to_owned(),
            ExecutionStage::Failed => state.result().map_or_else(
                || "Terraform failed.".to_owned(),
                |result| format!("Terraform failed: {:?}", result.termination().status),
            ),
        }
    };
    vec![
        Line::from(status),
        Line::from(vec![
            Span::raw(format!("Waiting {}s", state.waiting_at(now).as_secs())),
            Span::raw("    Follow: "),
            Span::styled(
                if view.follows_latest() { "On" } else { "Off" },
                theme::secondary_style(),
            ),
        ]),
        Line::from(format!("Elapsed {}", format_elapsed(state.elapsed_at(now)))),
    ]
}

fn footer_lines(state: &ExecutionState, width: u16) -> Vec<Line<'static>> {
    let items = if state.stage() == ExecutionStage::Failed {
        vec![
            footer::hint(&["q"], "quit"),
            footer::hint(&["↑", "↓", "PgUp", "PgDn"], "scroll"),
            footer::hint(&["y"], "copy diagnostic"),
        ]
    } else {
        vec![
            footer::hint(&["Ctrl-C"], "cancel"),
            footer::hint(&["↑", "↓", "PgUp", "PgDn"], "scroll"),
            footer::hint(&["End"], "follow latest"),
        ]
    };
    footer::layout(items, width)
}

fn scroll_limits(lines: &[Line<'static>], body: Rect) -> (u16, u16) {
    let vertical =
        u16::try_from(lines.len().saturating_sub(usize::from(body.height))).unwrap_or(u16::MAX);
    let horizontal = u16::try_from(max_line_width(lines).saturating_sub(usize::from(body.width)))
        .unwrap_or(u16::MAX);
    (vertical, horizontal)
}

fn max_line_width(lines: &[Line<'static>]) -> usize {
    lines.iter().map(Line::width).max().unwrap_or(0)
}

fn preferred_scroll(state: &ExecutionState, max: u16) -> u16 {
    state
        .result()
        .and_then(ExecutionResult::first_error_line)
        .and_then(|line| u16::try_from(line).ok())
        .unwrap_or(max)
        .min(max)
}

fn format_elapsed(elapsed: Duration) -> String {
    format!(
        "{}.{:01}s",
        elapsed.as_secs(),
        elapsed.subsec_millis() / 100
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::execution::{
        ExecutionContext, ExecutionEvent, ExecutionEventKind, ExecutionLogLine,
    };

    #[test]
    fn append_only_log_is_rendered_in_receive_order() {
        let now = Instant::now();
        let mut state = ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project", "Git comparison paused"),
        );
        for (stream, text) in [
            (EventStream::Stdout, "first"),
            (EventStream::Stderr, "second"),
            (EventStream::Stdout, "third"),
        ] {
            state.record(ExecutionEvent {
                received_at: now,
                kind: ExecutionEventKind::Log(ExecutionLogLine {
                    stream,
                    text: text.to_owned(),
                }),
            });
        }

        assert_eq!(
            execution_lines(&state)
                .iter()
                .map(Line::to_string)
                .collect::<Vec<_>>(),
            vec!["first", "second", "third"]
        );
    }
}
