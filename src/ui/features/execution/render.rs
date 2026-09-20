use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
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
const FLASH_BACKGROUND: Color = Color::Rgb(0xf4, 0x9e, 0x4c);
const FLASH_FOREGROUND: Color = Color::Rgb(0x11, 0x14, 0x19);

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
    let scroll = if view.follows_latest() {
        preferred_scroll(state, max_vertical)
    } else {
        view.scroll().min(max_vertical)
    };
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
    let current = if view.follows_latest() {
        preferred_scroll(state, max)
    } else {
        view.scroll().min(max)
    };
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
    let style = Style::default().fg(FLASH_FOREGROUND).bg(FLASH_BACKGROUND);
    lines
        .into_iter()
        .map(|line| Line::from(Span::styled(line.to_string(), style)))
        .collect()
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
    use ratatui::{buffer::Buffer, style::Modifier};

    use super::*;
    use crate::app::copy::CopyTarget;
    use crate::app::execution::{
        ApplyStatus, ExecutionContext, ExecutionEvent, ExecutionEventKind, ExecutionLogLine,
    };
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
        assert_eq!(buffer[(vertical_x, body.y)].symbol(), "↑");
        assert_eq!(
            buffer[(vertical_x, body.y)].fg,
            Color::Rgb(0xc0, 0xb8, 0xb0)
        );
        assert_eq!(buffer[(body.x, horizontal_y)].symbol(), "←");
        assert_eq!(
            buffer[(body.x, horizontal_y)].fg,
            Color::Rgb(0x50, 0x52, 0x5e)
        );
        assert_eq!(buffer[(horizontal_end_x, horizontal_y)].symbol(), "→");
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
}
