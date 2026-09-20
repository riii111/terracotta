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
struct PreparedContent<'a> {
    lines: Vec<Line<'a>>,
    max_width: usize,
}

pub(crate) fn render_execution_with_view(
    frame: &mut Frame<'_>,
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
) {
    let area = frame.area();
    let content = prepare_content(state);
    let layout = execution_layout_with_content(area, state, &content);
    if area.width < MIN_WIDTH
        || area.height < MIN_HEIGHT
        || layout.body().width == 0
        || layout.body().height == 0
    {
        let finished_apply = matches!(
            state.stage(),
            ExecutionStage::ApplySucceeded
                | ExecutionStage::ApplyFailed
                | ExecutionStage::ApplyInterrupted
        );
        let message = if state.stage() == ExecutionStage::Failed || finished_apply {
            "Terminal too small. Resize or press q to quit."
        } else {
            "Terminal too small. Resize or press Ctrl-C to cancel."
        };
        terminal_notice::render_wrapped(frame, area, message);
        return;
    }

    header::render_execution(frame, layout.shell.header(), state.context());
    let content_area =
        shell_layout::render_content_block(frame, layout.shell.content(), state.stage().title());
    debug_assert_eq!(content_area, layout.shell.content_inner());
    frame.render_widget(
        Paragraph::new(status_lines(state, view, now)).style(theme::body_style()),
        layout.chunks[0],
    );

    let line_count = content.lines.len();
    let max_line_width = content.max_width;
    let max_vertical = layout.max_vertical();
    let max_horizontal = layout.max_horizontal();
    let scroll = view.vertical_offset(initial_scroll(state, max_vertical), max_vertical);
    let horizontal = view.horizontal().min(max_horizontal);
    let lines = if state.copy_flash_active(now) {
        flash_lines(content.lines)
    } else {
        content.lines
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .scroll((scroll, horizontal)),
        layout.chunks[1],
    );
    let body = layout.body();
    let scrollbar_area = Rect::new(
        body.x,
        body.y,
        body.width
            .saturating_add(u16::from(layout.vertical_scrollbar())),
        body.height
            .saturating_add(u16::from(layout.horizontal_scrollbar())),
    );
    if layout.vertical_scrollbar() {
        scrollbar::render_vertical(
            frame,
            scrollbar_area,
            line_count,
            usize::from(body.height),
            usize::from(scroll),
        );
    }
    if layout.horizontal_scrollbar() {
        scrollbar::render_horizontal(
            frame,
            scrollbar_area,
            max_line_width,
            usize::from(body.width),
            usize::from(horizontal),
        );
    }
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
    body: Rect,
    vertical_scrollbar: bool,
    horizontal_scrollbar: bool,
    max_vertical: u16,
    max_horizontal: u16,
}

impl ExecutionLayout {
    pub(crate) const fn body(&self) -> Rect {
        self.body
    }

    pub(crate) const fn vertical_scrollbar(&self) -> bool {
        self.vertical_scrollbar
    }

    pub(crate) const fn horizontal_scrollbar(&self) -> bool {
        self.horizontal_scrollbar
    }

    pub(crate) const fn max_vertical(&self) -> u16 {
        self.max_vertical
    }

    pub(crate) const fn max_horizontal(&self) -> u16 {
        self.max_horizontal
    }
}

pub(crate) fn execution_layout(area: Rect, state: &ExecutionState) -> ExecutionLayout {
    let content = prepare_content(state);
    execution_layout_with_content(area, state, &content)
}

fn execution_layout_with_content(
    area: Rect,
    state: &ExecutionState,
    content: &PreparedContent<'_>,
) -> ExecutionLayout {
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
    let available = chunks[1];
    let (vertical_scrollbar, horizontal_scrollbar) =
        scrollbar_reservations(content.lines.len(), content.max_width, available);
    let body = Rect::new(
        available.x,
        available.y,
        available
            .width
            .saturating_sub(u16::from(vertical_scrollbar)),
        available
            .height
            .saturating_sub(u16::from(horizontal_scrollbar)),
    );
    let (max_vertical, max_horizontal) =
        scroll_limits(content.lines.len(), content.max_width, body);
    ExecutionLayout {
        shell,
        chunks,
        body,
        vertical_scrollbar,
        horizontal_scrollbar,
        max_vertical,
        max_horizontal,
    }
}

pub(crate) fn execution_scroll_position_with_view(
    state: &ExecutionState,
    view: ExecutionViewState,
    layout: &ExecutionLayout,
) -> (u16, u16) {
    let max = layout.max_vertical();
    let current = view.vertical_offset(initial_scroll(state, max), max);
    (current, max)
}

pub(crate) fn execution_horizontal_scroll_position_with_view(
    view: ExecutionViewState,
    layout: &ExecutionLayout,
) -> (u16, u16) {
    let max = layout.max_horizontal();
    (view.horizontal().min(max), max)
}

fn prepare_content(state: &ExecutionState) -> PreparedContent<'_> {
    let log = state.progress().log();
    let mut lines = Vec::new();
    for line in log {
        let style = if line.stream == EventStream::Stderr {
            theme::warning_style()
        } else {
            theme::body_style()
        };
        lines.extend(
            line.text
                .lines()
                .map(|text| Line::from(Span::styled(text, style))),
        );
    }
    let summary_is_in_log = state
        .result()
        .and_then(|result| result.summary_line())
        .is_some_and(|summary| {
            log.iter()
                .any(|line| line.text.lines().any(|text| text == summary))
        });
    if let Some(summary) = state.result().and_then(|result| result.summary_line())
        && !summary_is_in_log
    {
        lines.push(Line::default());
        lines.push(Line::from(summary));
    }
    if lines.is_empty() {
        lines.push(Line::from("Waiting for Terraform output..."));
    }
    let max_width = max_line_width(&lines);
    PreparedContent { lines, max_width }
}

fn status_lines(
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
) -> Vec<Line<'static>> {
    let status = if state.is_cancelling() {
        if state.is_apply() {
            "Stopping... Changes may already be applied.".to_owned()
        } else {
            "Stopping...".to_owned()
        }
    } else {
        match state.stage() {
            ExecutionStage::Initializing => "Initializing...".to_owned(),
            ExecutionStage::Planning => "Planning...".to_owned(),
            ExecutionStage::Reading => "Reading plan...".to_owned(),
            ExecutionStage::Applying => "Applying...".to_owned(),
            ExecutionStage::ApplySucceeded => "Apply complete".to_owned(),
            ExecutionStage::ApplyFailed => "Apply failed".to_owned(),
            ExecutionStage::ApplyInterrupted => "Apply interrupted".to_owned(),
            ExecutionStage::Failed => state.result().map_or_else(
                || "Terraform failed.".to_owned(),
                |result| format!("Terraform failed: {:?}", result.termination().status),
            ),
        }
    };
    let detail = if state.is_apply() {
        match state.stage() {
            ExecutionStage::ApplySucceeded => state
                .result()
                .and_then(|result| result.summary_line())
                .unwrap_or("Apply complete."),
            ExecutionStage::ApplyFailed | ExecutionStage::ApplyInterrupted => {
                "Changes may already be applied."
            }
            _ => "Applying...",
        }
        .to_owned()
    } else {
        format!(
            "Waiting {}s    Follow: {}",
            state.waiting_at(now).as_secs(),
            if view.follows_latest() { "On" } else { "Off" }
        )
    };
    vec![
        Line::from(status),
        Line::from(detail),
        Line::from(format!("Elapsed {}", format_elapsed(state.elapsed_at(now)))),
    ]
}

fn footer_lines(state: &ExecutionState, width: u16) -> Vec<Line<'static>> {
    let finished_apply = matches!(
        state.stage(),
        ExecutionStage::ApplySucceeded
            | ExecutionStage::ApplyFailed
            | ExecutionStage::ApplyInterrupted
    );
    let items = if state.stage() == ExecutionStage::Failed || finished_apply {
        vec![
            footer::hint(&["q", "Ctrl-C"], "quit"),
            footer::hint(&["↑", "↓", "PgUp", "PgDn"], "scroll"),
            footer::hint(
                &["y"],
                if finished_apply {
                    "yank result"
                } else {
                    "copy diagnostic"
                },
            ),
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

fn scroll_limits(line_count: usize, line_width: usize, body: Rect) -> (u16, u16) {
    let vertical =
        u16::try_from(line_count.saturating_sub(usize::from(body.height))).unwrap_or(u16::MAX);
    let horizontal =
        u16::try_from(line_width.saturating_sub(usize::from(body.width))).unwrap_or(u16::MAX);
    (vertical, horizontal)
}

fn scrollbar_reservations(line_count: usize, line_width: usize, area: Rect) -> (bool, bool) {
    let mut vertical = false;
    let mut horizontal = false;
    loop {
        let next_vertical =
            line_count > usize::from(area.height.saturating_sub(u16::from(horizontal)));
        let next_horizontal =
            line_width > usize::from(area.width.saturating_sub(u16::from(vertical)));
        if next_vertical == vertical && next_horizontal == horizontal {
            return (vertical, horizontal);
        }
        vertical = next_vertical;
        horizontal = next_horizontal;
    }
}

fn max_line_width(lines: &[Line<'_>]) -> usize {
    lines.iter().map(Line::width).max().unwrap_or(0)
}

fn flash_lines(lines: Vec<Line<'_>>) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| Line::from(Span::styled(line.to_string(), theme::copy_flash_style())))
        .collect()
}

fn initial_scroll(state: &ExecutionState, max: u16) -> u16 {
    if !matches!(
        state.stage(),
        ExecutionStage::Failed | ExecutionStage::ApplyFailed
    ) {
        return max;
    }

    state
        .result()
        .and_then(ExecutionResult::first_error_line)
        .or_else(|| state.progress().first_error_line())
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
    use ratatui::{
        buffer::Buffer,
        style::{Color, Modifier},
    };

    use super::*;
    use crate::app::copy::{CopyResult, CopyTarget};
    use crate::app::execution::{
        ApplyStatus, ExecutionContext, ExecutionEvent, ExecutionEventKind, ExecutionLogLine,
    };
    use crate::app::session::{self, Action, SessionState};
    use crate::ui::test_support::{
        assert_shell_frame_and_footer, buffer_text, render_to_buffer, write_buffer_captures,
    };

    const SIZES: [(u16, u16); 3] = [(80, 24), (120, 40), (160, 60)];
    const APPLY_LOG: &[(&str, EventStream)] = &[
        ("terraform apply review.tfplan", EventStream::Stdout),
        (
            "terraform_data.api: Modifying... [id=api-20260920]",
            EventStream::Stdout,
        ),
        (
            "terraform_data.api: Modifications complete after 1s [id=api-20260920]",
            EventStream::Stdout,
        ),
        (
            "terraform_data.worker: Replacing... [id=worker-20260920]",
            EventStream::Stdout,
        ),
        (
            "terraform_data.worker: Destruction complete after 1s",
            EventStream::Stdout,
        ),
        (
            "terraform_data.worker: Creation complete after 1s [id=worker-20260920]",
            EventStream::Stdout,
        ),
        (
            "terraform_data.old: Destruction complete after 1s",
            EventStream::Stdout,
        ),
        (
            "terraform_data.new: Creation complete after 1s [id=new-20260920]",
            EventStream::Stdout,
        ),
        (
            "A deliberately long synthetic apply line keeps horizontal scrolling visible in the production renderer",
            EventStream::Stdout,
        ),
    ];
    const SUCCESS_LOG: &[(&str, EventStream)] = &[
        (
            "Warning: synthetic provider emitted a non-blocking diagnostic",
            EventStream::Stderr,
        ),
        ("Apply finished successfully.", EventStream::Stdout),
        ("Outputs: endpoint = synthetic", EventStream::Stdout),
        ("Apply log remains in receive order.", EventStream::Stdout),
    ];

    fn apply_state(status: ApplyStatus) -> (ExecutionState, Instant) {
        let started_at = Instant::now();
        let finished_at = started_at + Duration::from_secs(4);
        let mut state = ExecutionState::applying(
            started_at,
            ExecutionContext::loading("/repo/environments/production/main")
                .with_workspace("default"),
        );
        for (text, stream) in APPLY_LOG {
            state.record(ExecutionEvent {
                received_at: started_at,
                kind: ExecutionEventKind::Log(ExecutionLogLine {
                    stream: *stream,
                    text: (*text).to_owned(),
                }),
            });
        }
        if status == ApplyStatus::Succeeded {
            for (text, stream) in SUCCESS_LOG {
                state.record(ExecutionEvent {
                    received_at: started_at,
                    kind: ExecutionEventKind::Log(ExecutionLogLine {
                        stream: *stream,
                        text: (*text).to_owned(),
                    }),
                });
            }
        }
        state.finish_apply(
            status,
            (status == ApplyStatus::Succeeded)
                .then(|| "Resources: 2 added, 2 changed, 1 destroyed.".to_owned()),
            (status == ApplyStatus::Failed)
                .then(|| "AccessDenied: synthetic provider rejected the request".to_owned()),
            finished_at,
        );
        (state, finished_at)
    }

    fn long_apply_state(status: ApplyStatus) -> (ExecutionState, Instant) {
        let started_at = Instant::now();
        let finished_at = started_at + Duration::from_secs(4);
        let mut state = ExecutionState::applying(
            started_at,
            ExecutionContext::loading("/repo/environments/production/main")
                .with_workspace("default"),
        );
        for index in 0..40 {
            let text = match index {
                3 => "Error: initial failure".to_owned(),
                39 => "tail marker".to_owned(),
                _ => format!("log line {index}"),
            };
            state.record(ExecutionEvent {
                received_at: started_at,
                kind: ExecutionEventKind::Log(ExecutionLogLine {
                    stream: EventStream::Stdout,
                    text,
                }),
            });
        }
        state.finish_apply(
            status,
            None,
            (status == ApplyStatus::Failed).then(|| "apply failed".to_owned()),
            finished_at,
        );
        (state, finished_at)
    }

    fn snapshot(name: &str, buffer: &Buffer) {
        insta::assert_snapshot!(name.to_string(), buffer_text(buffer));
        write_buffer_captures(name, buffer);
    }

    fn assert_text_uses_style(buffer: &Buffer, text: &str, color: Color, modifier: Modifier) {
        let area = buffer.area();
        for y in area.y..area.bottom() {
            let symbols = (area.x..area.right())
                .map(|x| buffer.cell((x, y)).expect("execution cell").symbol())
                .collect::<Vec<_>>();
            let Some(start) = (0..symbols.len()).find(|&start| {
                symbols[start..]
                    .iter()
                    .copied()
                    .collect::<String>()
                    .starts_with(text)
            }) else {
                continue;
            };
            for offset in 0..text.chars().count() {
                let cell = buffer
                    .cell((
                        area.x + u16::try_from(start + offset).expect("execution offset"),
                        y,
                    ))
                    .expect("execution cell");
                assert_eq!(cell.fg, color, "{text}");
                assert!(cell.modifier.contains(modifier), "{text}");
            }
            return;
        }
        panic!("text should be visible: {text}");
    }

    #[test]
    fn renders_apply_success_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let (state, now) = apply_state(ApplyStatus::Succeeded);
            let buffer = render_to_buffer((width, height), |frame| {
                render_execution_with_view(frame, &state, ExecutionViewState::default(), now);
            });

            snapshot(&format!("preview_{width}x{height}_apply-success"), &buffer);
        }
    }

    #[test]
    fn renders_apply_failure_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let (state, now) = apply_state(ApplyStatus::Failed);
            let buffer = render_to_buffer((width, height), |frame| {
                render_execution_with_view(frame, &state, ExecutionViewState::default(), now);
            });

            snapshot(&format!("preview_{width}x{height}_apply-failure"), &buffer);
        }
    }

    #[test]
    fn initial_execution_position_depends_on_the_completed_result() {
        struct InitialPositionCase {
            name: &'static str,
            status: ApplyStatus,
            expected_marker: &'static str,
            tail_is_visible: bool,
        }

        for case in [
            InitialPositionCase {
                name: "success_follows_tail",
                status: ApplyStatus::Succeeded,
                expected_marker: "tail marker",
                tail_is_visible: true,
            },
            InitialPositionCase {
                name: "failure_starts_at_first_error",
                status: ApplyStatus::Failed,
                expected_marker: "Error: initial failure",
                tail_is_visible: false,
            },
            InitialPositionCase {
                name: "interrupted_follows_tail",
                status: ApplyStatus::Interrupted,
                expected_marker: "tail marker",
                tail_is_visible: true,
            },
        ] {
            let (state, now) = long_apply_state(case.status);
            let buffer = render_to_buffer((80, 24), |frame| {
                render_execution_with_view(frame, &state, ExecutionViewState::default(), now);
            });
            let text = buffer_text(&buffer);

            assert!(text.contains(case.expected_marker), "case: {}", case.name);
            assert_eq!(
                text.contains("tail marker"),
                case.tail_is_visible,
                "case: {}",
                case.name
            );
        }
    }

    #[test]
    fn end_uses_the_log_tail_after_a_failed_apply() {
        let (state, now) = long_apply_state(ApplyStatus::Failed);
        let mut view = ExecutionViewState::default();
        view.end();
        let buffer = render_to_buffer((80, 24), |frame| {
            render_execution_with_view(frame, &state, view, now);
        });

        assert!(buffer_text(&buffer).contains("tail marker"));
    }

    #[test]
    fn production_execution_render_draws_shell_scrollbars_and_stream_colors() {
        let (state, now) = apply_state(ApplyStatus::Succeeded);
        let area = Rect::new(0, 0, 80, 24);
        let layout = execution_layout(area, &state);
        let buffer = render_to_buffer((area.width, area.height), |frame| {
            render_execution_with_view(frame, &state, ExecutionViewState::default(), now);
        });

        assert_shell_frame_and_footer(
            &buffer,
            layout.shell.content(),
            layout.shell.footer(),
            "y yank result",
        );
        let text = buffer_text(&buffer);
        assert!(text.contains("Apply complete"));
        assert!(text.contains("Apply finished successfully."));
        assert!(layout.vertical_scrollbar());
        assert!(layout.horizontal_scrollbar());
        let body = layout.body();
        let vertical_x = body.x.saturating_add(body.width);
        let horizontal_y = body.y.saturating_add(body.height);
        let horizontal_end_x = vertical_x;
        assert_eq!(buffer[(vertical_x, body.y)].symbol(), "▲");
        assert_eq!(
            buffer[(vertical_x, body.y)].fg,
            Color::Rgb(0xc0, 0xb8, 0xb0)
        );
        assert_eq!(buffer[(body.x, horizontal_y)].symbol(), "◀︎");
        assert_eq!(
            buffer[(body.x, horizontal_y)].fg,
            Color::Rgb(0x50, 0x52, 0x5e)
        );
        assert_eq!(buffer[(horizontal_end_x, horizontal_y)].symbol(), "▶︎");
        assert_eq!(
            buffer[(horizontal_end_x, horizontal_y)].fg,
            Color::Rgb(0xc0, 0xb8, 0xb0)
        );
        assert_text_uses_style(
            &buffer,
            "Warning: synthetic provider emitted a non-blocking diagnostic",
            Color::Rgb(0xeb, 0xcb, 0x8b),
            Modifier::BOLD,
        );
    }

    #[test]
    fn production_execution_scrollbars_reach_offsets_after_resize_and_single_overflow() {
        let (state, now) = apply_state(ApplyStatus::Succeeded);
        let mut previous_body = None;
        for area in [Rect::new(0, 0, 80, 24), Rect::new(0, 0, 88, 24)] {
            let layout = execution_layout(area, &state);
            assert!(layout.vertical_scrollbar());
            assert!(layout.horizontal_scrollbar());
            assert!(layout.max_vertical() > 1);
            assert!(layout.max_horizontal() > 1);
            assert_ne!(previous_body, Some(layout.body()));
            previous_body = Some(layout.body());

            for (vertical, horizontal) in [
                (0, 0),
                (layout.max_vertical() / 2, layout.max_horizontal() / 2),
                (layout.max_vertical(), layout.max_horizontal()),
            ] {
                let (layout, buffer) = execution_buffer_at(area, &state, now, vertical, horizontal);
                assert_scrollbar_positions(&buffer, &layout, vertical, horizontal);
            }
        }

        let area = Rect::new(0, 0, 80, 24);
        let (base_state, _) = applying_state_with_content(1, 1);
        let available = execution_layout(area, &base_state).chunks[1];

        let (vertical_state, vertical_now) = applying_state_with_content(
            available.height.saturating_add(1),
            available.width.saturating_sub(1),
        );
        let (vertical_layout, vertical_buffer) =
            execution_buffer_at(area, &vertical_state, vertical_now, 1, 0);
        assert_eq!(vertical_layout.max_vertical(), 1);
        assert!(!vertical_layout.horizontal_scrollbar());
        assert_scrollbar_positions(&vertical_buffer, &vertical_layout, 1, 0);

        let (horizontal_state, horizontal_now) = applying_state_with_content(
            available.height.saturating_sub(1),
            available.width.saturating_add(1),
        );
        let (horizontal_layout, horizontal_buffer) =
            execution_buffer_at(area, &horizontal_state, horizontal_now, 0, 1);
        assert_eq!(horizontal_layout.max_horizontal(), 1);
        assert!(!horizontal_layout.vertical_scrollbar());
        assert_scrollbar_positions(&horizontal_buffer, &horizontal_layout, 0, 1);
    }

    #[test]
    fn production_execution_failure_render_draws_diagnostic_color() {
        let (state, now) = apply_state(ApplyStatus::Failed);
        let buffer = render_to_buffer((80, 24), |frame| {
            render_execution_with_view(frame, &state, ExecutionViewState::default(), now);
        });

        assert!(
            buffer_text(&buffer).contains("AccessDenied: synthetic provider rejected the request")
        );
        let diagnostic = "AccessDenied: synthetic provider rejected the request";
        let area = buffer.area();
        for y in area.y..area.bottom() {
            let symbols = (area.x..area.right())
                .map(|x| buffer.cell((x, y)).expect("diagnostic cell").symbol())
                .collect::<Vec<_>>();
            let Some(start) = (0..symbols.len()).find(|&start| {
                symbols[start..]
                    .iter()
                    .copied()
                    .collect::<String>()
                    .starts_with(diagnostic)
            }) else {
                continue;
            };
            for offset in 0..diagnostic.chars().count() {
                let cell = buffer
                    .cell((
                        area.x + u16::try_from(start + offset).expect("diagnostic offset"),
                        y,
                    ))
                    .expect("diagnostic cell");
                assert_eq!(cell.fg, Color::Rgb(0xeb, 0xcb, 0x8b));
                assert!(cell.modifier.contains(Modifier::BOLD));
            }
            return;
        }
        panic!("diagnostic row should be visible");
    }

    #[test]
    fn production_execution_copy_flash_uses_accent_background_then_restores_log_style() {
        let started_at = Instant::now();
        let mut state = ExecutionState::applying(started_at, ExecutionContext::loading("/repo"));
        state.record(ExecutionEvent {
            received_at: started_at,
            kind: ExecutionEventKind::Log(ExecutionLogLine {
                stream: EventStream::Stdout,
                text: "terraform apply review.tfplan".to_owned(),
            }),
        });
        state.record(ExecutionEvent {
            received_at: started_at,
            kind: ExecutionEventKind::Log(ExecutionLogLine {
                stream: EventStream::Stdout,
                text: "apply output".to_owned(),
            }),
        });
        let mut session = SessionState::new(state);
        let before = render_to_buffer((80, 24), |frame| {
            render_execution_with_view(
                frame,
                session.execution().expect("execution should be visible"),
                ExecutionViewState::default(),
                started_at,
            );
        });
        session::update(
            &mut session,
            Action::CopyCompleted {
                target: CopyTarget::Execution,
                result: CopyResult::Written,
            },
            started_at,
        );
        let state = session.execution().expect("execution should be visible");
        let flash = render_to_buffer((80, 24), |frame| {
            render_execution_with_view(frame, state, ExecutionViewState::default(), started_at);
        });
        let after = render_to_buffer((80, 24), |frame| {
            render_execution_with_view(
                frame,
                state,
                ExecutionViewState::default(),
                started_at + Duration::from_millis(201),
            );
        });

        let body = execution_layout(Rect::new(0, 0, 80, 24), state).body();
        let flash_cell = find_text_cell(&flash, body, "terraform apply review.tfplan");
        assert_eq!(flash_cell.fg, Color::Rgb(0x11, 0x14, 0x19));
        assert_eq!(flash_cell.bg, Color::Rgb(0xf4, 0x9e, 0x4c));
        let before_cell = find_text_cell(&before, body, "terraform apply review.tfplan");
        let after_cell = find_text_cell(&after, body, "terraform apply review.tfplan");
        assert_eq!(after_cell, before_cell);
    }

    fn find_text_cell<'a>(buffer: &'a Buffer, area: Rect, text: &str) -> &'a ratatui::buffer::Cell {
        for y in area.y..area.bottom() {
            let symbols = (area.x..area.right())
                .map(|x| buffer.cell((x, y)).expect("execution cell").symbol())
                .collect::<Vec<_>>();
            let Some(start) = (0..symbols.len()).find(|&start| {
                symbols[start..]
                    .iter()
                    .copied()
                    .collect::<String>()
                    .starts_with(text)
            }) else {
                continue;
            };
            return buffer
                .cell((area.x + u16::try_from(start).expect("execution offset"), y))
                .expect("execution cell");
        }
        panic!("text should be visible: {text}");
    }

    #[test]
    fn append_only_log_is_rendered_in_receive_order() {
        let now = Instant::now();
        let mut state = ExecutionState::with_context(now, ExecutionContext::loading("/project"));
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
            prepare_content(&state)
                .lines
                .iter()
                .map(Line::to_string)
                .collect::<Vec<_>>(),
            vec!["first", "second", "third"]
        );
    }

    #[test]
    fn completed_apply_statuses_keep_full_log_order_for_render_and_copy() {
        struct ApplyCase {
            name: &'static str,
            status: ApplyStatus,
            expected_copy: &'static str,
        }

        for case in [
            ApplyCase {
                name: "succeeded",
                status: ApplyStatus::Succeeded,
                expected_copy: "Apply complete.\nfirst\nsecond\nthird",
            },
            ApplyCase {
                name: "failed",
                status: ApplyStatus::Failed,
                expected_copy: "Apply failed.\nChanges may already be applied.\nfirst\nsecond\nthird",
            },
            ApplyCase {
                name: "interrupted",
                status: ApplyStatus::Interrupted,
                expected_copy: "Apply interrupted.\nChanges may already be applied.\nfirst\nsecond\nthird",
            },
        ] {
            let now = Instant::now();
            let mut state = ExecutionState::applying(now, ExecutionContext::loading("/project"));
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
            state.finish_apply(case.status, None, None, now + Duration::from_secs(1));

            assert_eq!(
                prepare_content(&state)
                    .lines
                    .iter()
                    .map(Line::to_string)
                    .collect::<Vec<_>>(),
                ["first", "second", "third"],
                "case: {}",
                case.name
            );
            assert_eq!(
                state
                    .copy_effect(CopyTarget::Execution)
                    .expect("completed apply should be copyable")
                    .text(),
                case.expected_copy,
                "case: {}",
                case.name
            );
        }
    }

    fn execution_buffer_at(
        area: Rect,
        state: &ExecutionState,
        now: Instant,
        vertical: u16,
        horizontal: u16,
    ) -> (ExecutionLayout, Buffer) {
        let layout = execution_layout(area, state);
        let mut view = ExecutionViewState::default();
        let mut current_vertical = 0;
        view.apply_scroll(
            super::super::ExecutionScroll::Top,
            current_vertical,
            layout.max_vertical(),
            layout.body().height,
        );
        for _ in 0..vertical {
            view.apply_scroll(
                super::super::ExecutionScroll::Down,
                current_vertical,
                layout.max_vertical(),
                layout.body().height,
            );
            current_vertical = current_vertical
                .saturating_add(1)
                .min(layout.max_vertical());
        }
        let mut current_horizontal = 0;
        view.apply_horizontal_scroll(
            super::super::ExecutionScroll::LeftEdge,
            current_horizontal,
            layout.max_horizontal(),
            current_vertical,
        );
        for _ in 0..horizontal {
            view.apply_horizontal_scroll(
                super::super::ExecutionScroll::Right,
                current_horizontal,
                layout.max_horizontal(),
                current_vertical,
            );
            current_horizontal = current_horizontal
                .saturating_add(1)
                .min(layout.max_horizontal());
        }
        let buffer = render_to_buffer((area.width, area.height), |frame| {
            render_execution_with_view(frame, state, view, now);
        });
        (layout, buffer)
    }

    fn applying_state_with_content(line_count: u16, line_width: u16) -> (ExecutionState, Instant) {
        let now = Instant::now();
        let mut state = ExecutionState::applying(now, ExecutionContext::loading("/repo"));
        let text = "x".repeat(usize::from(line_width));
        for _ in 0..line_count {
            state.record(ExecutionEvent {
                received_at: now,
                kind: ExecutionEventKind::Log(ExecutionLogLine {
                    stream: EventStream::Stdout,
                    text: text.clone(),
                }),
            });
        }
        (state, now)
    }

    fn assert_scrollbar_positions(
        buffer: &Buffer,
        layout: &ExecutionLayout,
        vertical: u16,
        horizontal: u16,
    ) {
        let body = layout.body();
        if layout.vertical_scrollbar() {
            let height = body.height + u16::from(layout.horizontal_scrollbar());
            let symbols = (body.y..body.y + height)
                .map(|y| {
                    buffer
                        .cell((body.x + body.width, y))
                        .expect("vertical cell")
                        .symbol()
                        .to_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(symbols.first().map(String::as_str), Some("▲"));
            assert_thumb_segments(
                &symbols[1..symbols.len() - 1],
                "│",
                "┃",
                usize::from(vertical),
                usize::from(layout.max_vertical()),
            );
        }
        if layout.horizontal_scrollbar() {
            let width = body.width + u16::from(layout.vertical_scrollbar());
            let symbols = (body.x..body.x + width)
                .map(|x| {
                    buffer
                        .cell((x, body.y + body.height))
                        .expect("horizontal cell")
                        .symbol()
                        .to_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(symbols.first().map(String::as_str), Some("◀︎"));
            assert_eq!(symbols.last().map(String::as_str), Some("▶︎"));
            assert_thumb_segments(
                &symbols[1..symbols.len() - 1],
                "─",
                "═",
                usize::from(horizontal),
                usize::from(layout.max_horizontal()),
            );
        }
    }

    fn assert_thumb_segments(
        track: &[String],
        track_symbol: &str,
        thumb_symbol: &str,
        position: usize,
        max_position: usize,
    ) {
        let thumb_start = track
            .iter()
            .position(|symbol| symbol == thumb_symbol)
            .expect("scrollbar should contain a thumb");
        let thumb_end = track
            .iter()
            .rposition(|symbol| symbol == thumb_symbol)
            .expect("scrollbar should contain a thumb");
        assert!(
            track[thumb_start..=thumb_end]
                .iter()
                .all(|symbol| symbol == thumb_symbol)
        );
        assert!(
            track[..thumb_start]
                .iter()
                .all(|symbol| symbol == track_symbol)
        );
        assert!(
            track[thumb_end + 1..]
                .iter()
                .all(|symbol| symbol == track_symbol)
        );
        if position == 0 {
            assert_eq!(thumb_start, 0);
        } else {
            assert!(thumb_start > 0);
        }
        if position == max_position {
            assert_eq!(thumb_end, track.len() - 1);
        } else {
            assert!(thumb_end < track.len() - 1);
        }
    }
}
