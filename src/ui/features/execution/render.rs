use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{
    copy::CopyNotice,
    execution::{
        EventStream, ExecutionLogLine, ExecutionResult, ExecutionStage, ExecutionState,
        ExecutionTargetStatus,
    },
    plan::PlanAction,
};
use crate::ui::primitives::atoms::{scrollbar, separator};
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{context::truncate_middle, footer, header, layout as shell_layout};
use crate::ui::theme;

use super::ExecutionViewState;

const MIN_HEIGHT: u16 = 9;
const MIN_WIDTH: u16 = 32;
const STATUS_HEIGHT: u16 = 3;
const COMPACT_STATUS_HEIGHT: u16 = 4;
const APPLY_STATUS_HEIGHT: u16 = 2;
const TARGET_ADDRESS_WIDTH: usize = 24;
struct PreparedContent<'a> {
    lines: Vec<Line<'a>>,
    max_width: usize,
}

pub(crate) fn render_execution_with_quit_confirmation(
    frame: &mut Frame<'_>,
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
    quit_confirmation: bool,
) {
    let area = frame.area();
    if state.is_apply() {
        render_apply_execution(frame, state, view, now, quit_confirmation);
        return;
    }
    let content = prepare_content(state);
    let status = status_lines(state, view, now);
    let notice = state.copy_feedback().notice_at(now);
    let layout = execution_layout_with_content(
        area,
        state,
        view,
        &content,
        &status,
        notice.map(CopyNotice::message),
        quit_confirmation,
    );
    if area.width < MIN_WIDTH
        || area.height < MIN_HEIGHT
        || (!compact_apply(state, view) && (layout.body().width == 0 || layout.body().height == 0))
    {
        let finished_apply = matches!(
            state.stage(),
            ExecutionStage::ApplySucceeded
                | ExecutionStage::ApplyFailed
                | ExecutionStage::ApplyInterrupted
        );
        let message = if quit_confirmation {
            "Quit? Enter exit / Esc cancel"
        } else if state.stage() == ExecutionStage::Failed || finished_apply {
            "Terminal too small. Resize or press q to quit."
        } else {
            "Terminal too small. Resize or press Ctrl-C to cancel."
        };
        terminal_notice::render_wrapped(frame, area, message);
        return;
    }

    header::render_execution(frame, layout.shell.header(), state.context());
    let title = if finished_apply(state) {
        "Apply result"
    } else {
        state.stage().title()
    };
    let content_area = shell_layout::render_content_block(frame, layout.shell.content(), title);
    debug_assert_eq!(content_area, layout.shell.content_inner());
    if compact_apply(state, view) {
        render_compact_execution(frame, &layout, status, notice);
        return;
    }
    render_log_view(frame, &layout, state, view, now, content, notice);
}

fn render_apply_execution(
    frame: &mut Frame<'_>,
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
    quit_confirmation: bool,
) {
    let area = frame.area();
    let content = prepare_selected_content(state, view);
    let status = apply_status_lines(state, now);
    let notice = state.copy_feedback().notice_at(now);
    let layout = execution_layout_with_content(
        area,
        state,
        view,
        &content,
        &status,
        notice.map(CopyNotice::message),
        quit_confirmation,
    );
    if area.width < MIN_WIDTH
        || area.height < MIN_HEIGHT
        || layout.target_body().height == 0
        || layout.body().height == 0
    {
        let message = if quit_confirmation {
            "Quit? Enter exit / Esc cancel"
        } else if finished_apply(state) {
            "Terminal too small. Resize or press q to quit."
        } else {
            "Terminal too small. Resize or press Ctrl-C to cancel."
        };
        terminal_notice::render_wrapped(frame, area, message);
        return;
    }

    header::render_execution(frame, layout.shell.header(), state.context());
    let title = if finished_apply(state) {
        "Apply result"
    } else {
        "Applying"
    };
    let content_area = shell_layout::render_content_block(frame, layout.shell.content(), title);
    debug_assert_eq!(content_area, layout.shell.content_inner());
    frame.render_widget(status_paragraph(status, true), layout.status());
    render_target_panel(
        frame,
        layout.target_panel(),
        layout.target_body(),
        state,
        view,
        now,
    );
    render_log_panel(
        frame,
        layout.log_panel(),
        layout.body(),
        state,
        view,
        content,
        now,
    );
    render_footer(
        frame,
        layout.shell.footer(),
        layout.shell.footer_lines(),
        notice,
    );
}

fn render_target_panel(
    frame: &mut Frame<'_>,
    panel: Rect,
    body: Rect,
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
) {
    let title = if view.logs_open() {
        "Targets"
    } else {
        "Targets *"
    };
    frame.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .border_style(theme::frame_style())
            .title(title),
        panel,
    );
    let all_logs_style = if view.selected_target().is_none() {
        theme::accent_style().add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        theme::secondary_style()
    };
    let show_previous = state.progress().has_previous();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            if view.selected_target().is_none() {
                "> All logs"
            } else {
                "  All logs"
            },
            all_logs_style,
        )))
        .style(theme::body_style()),
        Rect::new(body.x, body.y.saturating_sub(2), body.width, 1),
    );
    frame.render_widget(
        Paragraph::new(if show_previous {
            format!(
                "  {:<width$}  {:<10} {:<10} {:>7}  {:>7}",
                "Resource",
                "Status",
                "Action",
                "Elapsed",
                "Previous",
                width = TARGET_ADDRESS_WIDTH,
            )
        } else {
            format!(
                "  {:<width$}  {:<10} {:<10} {:>7}",
                "Resource",
                "Status",
                "Action",
                "Elapsed",
                width = TARGET_ADDRESS_WIDTH,
            )
        })
        .style(theme::secondary_style()),
        Rect::new(body.x, body.y.saturating_sub(1), body.width, 1),
    );

    let finished = state.result().is_some();
    let indices = state.progress().display_target_indices(finished);
    let offset = view.target_vertical_offset(0, layout_target_max(indices.len(), body.height));
    let lines = indices
        .iter()
        .map(|index| target_line(state, *index, view.selected_target(), now, show_previous))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .scroll((offset, 0)),
        body,
    );
    let max = layout_target_max(indices.len(), body.height);
    if max > 0 {
        scrollbar::render_vertical(
            frame,
            Rect::new(body.x, body.y, body.width.saturating_add(1), body.height),
            indices.len(),
            usize::from(body.height),
            usize::from(offset),
        );
    }
}

fn render_log_panel(
    frame: &mut Frame<'_>,
    panel: Rect,
    body: Rect,
    state: &ExecutionState,
    view: ExecutionViewState,
    content: PreparedContent<'_>,
    now: Instant,
) {
    let title = view
        .selected_target()
        .and_then(|index| state.progress().targets().get(index))
        .map_or_else(
            || "Logs: All logs".to_owned(),
            |target| format!("Logs: {}", target.address()),
        );
    frame.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .border_style(theme::frame_style())
            .title(title),
        panel,
    );
    let line_count = content.lines.len();
    let max_line_width = content.max_width;
    let (max_vertical, max_horizontal) = scroll_limits(line_count, max_line_width, body);
    let scroll = view.vertical_offset(initial_scroll(state, max_vertical), max_vertical);
    let horizontal = view.horizontal().min(max_horizontal);
    let lines = if state.copy_feedback().flash_active(now) {
        flash_lines(content.lines)
    } else {
        content.lines
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .scroll((scroll, horizontal)),
        body,
    );
    let (vertical, horizontal_scrollbar) = scrollbar_reservations(line_count, max_line_width, body);
    let scrollbar_area = Rect::new(
        body.x,
        body.y,
        body.width.saturating_add(u16::from(vertical)),
        body.height.saturating_add(u16::from(horizontal_scrollbar)),
    );
    if vertical {
        scrollbar::render_vertical(
            frame,
            scrollbar_area,
            line_count,
            usize::from(body.height),
            usize::from(scroll),
        );
    }
    if horizontal_scrollbar {
        scrollbar::render_horizontal(
            frame,
            scrollbar_area,
            max_line_width,
            usize::from(body.width),
            usize::from(horizontal),
        );
    }
}

fn render_log_view(
    frame: &mut Frame<'_>,
    layout: &ExecutionLayout,
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
    content: PreparedContent<'_>,
    notice: Option<CopyNotice>,
) {
    let status = status_lines(state, view, now);
    frame.render_widget(
        status_paragraph(status, finished_apply(state)),
        layout.status(),
    );

    let line_count = content.lines.len();
    let max_line_width = content.max_width;
    let max_vertical = layout.max_vertical();
    let max_horizontal = layout.max_horizontal();
    let scroll = view.vertical_offset(initial_scroll(state, max_vertical), max_vertical);
    let horizontal = view.horizontal().min(max_horizontal);
    let lines = if state.copy_feedback().flash_active(now) {
        flash_lines(content.lines)
    } else {
        content.lines
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .scroll((scroll, horizontal)),
        layout.log_area(),
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
    frame.render_widget(
        separator::render(layout.separator().width),
        layout.separator(),
    );
    render_footer(
        frame,
        layout.shell.footer(),
        layout.shell.footer_lines(),
        notice,
    );
}

fn render_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    lines: &[Line<'static>],
    notice: Option<CopyNotice>,
) {
    footer::render(
        frame,
        area,
        lines,
        notice.map(|notice| {
            (
                notice.message(),
                if matches!(notice, CopyNotice::Failed) {
                    theme::error_style()
                } else {
                    theme::accent_style()
                },
            )
        }),
    );
}

fn render_compact_execution(
    frame: &mut Frame<'_>,
    layout: &ExecutionLayout,
    status: Vec<Line<'static>>,
    notice: Option<CopyNotice>,
) {
    frame.render_widget(status_paragraph(status, true), layout.status());
    render_footer(
        frame,
        layout.shell.footer(),
        layout.shell.footer_lines(),
        notice,
    );
}

pub(crate) struct ExecutionLayout {
    shell: shell_layout::ShellLayout,
    status: Rect,
    target_panel: Rect,
    target_body: Rect,
    log_area: Rect,
    log_panel: Rect,
    separator: Rect,
    body: Rect,
    target_max_vertical: u16,
    vertical_scrollbar: bool,
    horizontal_scrollbar: bool,
    max_vertical: u16,
    max_horizontal: u16,
}

impl ExecutionLayout {
    pub(crate) const fn status(&self) -> Rect {
        self.status
    }

    pub(crate) const fn log_area(&self) -> Rect {
        self.log_area
    }

    pub(crate) const fn target_panel(&self) -> Rect {
        self.target_panel
    }

    pub(crate) const fn target_body(&self) -> Rect {
        self.target_body
    }

    pub(crate) const fn log_panel(&self) -> Rect {
        self.log_panel
    }

    pub(crate) const fn separator(&self) -> Rect {
        self.separator
    }

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

    pub(crate) const fn target_max_vertical(&self) -> u16 {
        self.target_max_vertical
    }

    pub(crate) const fn max_horizontal(&self) -> u16 {
        self.max_horizontal
    }
}

pub(crate) fn execution_layout_with_view(
    area: Rect,
    state: &ExecutionState,
    view: ExecutionViewState,
) -> ExecutionLayout {
    execution_layout_with_quit_confirmation_and_view(area, state, view, false)
}

fn execution_layout_with_quit_confirmation_and_view(
    area: Rect,
    state: &ExecutionState,
    view: ExecutionViewState,
    quit_confirmation: bool,
) -> ExecutionLayout {
    let content = if state.is_apply() {
        prepare_selected_content(state, view)
    } else {
        prepare_content(state)
    };
    let status = if state.is_apply() {
        apply_status_lines(state, Instant::now())
    } else {
        status_lines(state, view, Instant::now())
    };
    execution_layout_with_content(
        area,
        state,
        view,
        &content,
        &status,
        state.copy_feedback().notice().map(CopyNotice::message),
        quit_confirmation,
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "the non-apply and apply layout branches share one public layout entry point"
)]
fn execution_layout_with_content(
    area: Rect,
    state: &ExecutionState,
    view: ExecutionViewState,
    content: &PreparedContent<'_>,
    status: &[Line<'static>],
    notice: Option<&str>,
    quit_confirmation: bool,
) -> ExecutionLayout {
    if state.is_apply() {
        return applying_layout(
            area,
            state,
            view,
            content,
            status,
            notice,
            quit_confirmation,
        );
    }
    let panel_width = shell_layout::centered_width(area);
    let compact = compact_apply(state, view);
    let normal_footer_lines = footer_lines(state, view, panel_width, notice);
    let normal_required_footer_lines = required_footer_lines(state, panel_width, notice);
    let status_height = status_height(state, status, panel_width.saturating_sub(2), compact);
    let requested_height = execution_requested_height(
        area,
        state,
        content,
        status_height,
        compact,
        (&normal_footer_lines, &normal_required_footer_lines),
    );
    let shell_area = if compact {
        compact_shell_area(area, requested_height)
    } else {
        shell_layout::centered_area(area, requested_height)
    };
    let footer_lines = if quit_confirmation {
        footer::pad_lines(
            footer::quit_confirmation_lines(panel_width, notice),
            normal_footer_lines.len(),
        )
    } else {
        normal_footer_lines
    };
    let required_footer_lines = if quit_confirmation {
        footer::pad_lines(
            footer::quit_confirmation_lines(panel_width, notice),
            normal_required_footer_lines.len(),
        )
    } else {
        normal_required_footer_lines
    };
    let shell = shell_layout::layout(shell_area, footer_lines, required_footer_lines, 1);
    if compact {
        let status = shell.content_inner();
        return ExecutionLayout {
            shell,
            status,
            target_panel: Rect::default(),
            target_body: Rect::default(),
            log_area: Rect::default(),
            log_panel: Rect::default(),
            separator: Rect::default(),
            body: Rect::default(),
            target_max_vertical: 0,
            vertical_scrollbar: false,
            horizontal_scrollbar: false,
            max_vertical: 0,
            max_horizontal: 0,
        };
    }
    let constraints = if finished_apply(state) {
        [
            Constraint::Length(status_height),
            Constraint::Length(1),
            Constraint::Min(1),
        ]
    } else {
        [
            Constraint::Length(status_height),
            Constraint::Min(1),
            Constraint::Length(1),
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(shell.content_inner())
        .to_vec();
    let (status_area, separator_area, available) = if finished_apply(state) {
        (chunks[0], chunks[1], chunks[2])
    } else {
        (chunks[0], chunks[2], chunks[1])
    };
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
        status: status_area,
        target_panel: Rect::default(),
        target_body: Rect::default(),
        log_area: available,
        log_panel: Rect::default(),
        separator: separator_area,
        body,
        target_max_vertical: 0,
        vertical_scrollbar,
        horizontal_scrollbar,
        max_vertical,
        max_horizontal,
    }
}

fn applying_layout(
    area: Rect,
    state: &ExecutionState,
    view: ExecutionViewState,
    content: &PreparedContent<'_>,
    status: &[Line<'static>],
    notice: Option<&str>,
    quit_confirmation: bool,
) -> ExecutionLayout {
    let panel_width = shell_layout::centered_width(area);
    let normal_footer_lines = apply_footer_lines(state, view, panel_width, notice);
    let normal_required_footer_lines = apply_required_footer_lines(state, panel_width, notice);
    let footer_lines = if quit_confirmation {
        footer::pad_lines(
            footer::quit_confirmation_lines(panel_width, notice),
            normal_footer_lines.len(),
        )
    } else {
        normal_footer_lines
    };
    let required_footer_lines = if quit_confirmation {
        footer::pad_lines(
            footer::quit_confirmation_lines(panel_width, notice),
            normal_required_footer_lines.len(),
        )
    } else {
        normal_required_footer_lines
    };
    let shell_area = shell_layout::max_centered_area(area);
    let shell = shell_layout::layout(shell_area, footer_lines, required_footer_lines, 4);
    let status_height =
        status_line_count(status, panel_width.saturating_sub(2)).max(APPLY_STATUS_HEIGHT);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(status_height), Constraint::Min(1)])
        .split(shell.content_inner())
        .to_vec();
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1])
        .to_vec();
    let target_panel = panels[0];
    let log_panel = panels[1];
    let target_inner = Block::new().borders(Borders::ALL).inner(target_panel);
    let target_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(target_inner)
        .to_vec();
    let target_body = target_rows[2];
    let target_count = state.progress().targets().len();
    let target_max_vertical = layout_target_max(target_count, target_body.height);
    let log_inner = Block::new().borders(Borders::ALL).inner(log_panel);
    let (vertical_scrollbar, horizontal_scrollbar) =
        scrollbar_reservations(content.lines.len(), content.max_width, log_inner);
    let body = Rect::new(
        log_inner.x,
        log_inner.y,
        log_inner
            .width
            .saturating_sub(u16::from(vertical_scrollbar)),
        log_inner
            .height
            .saturating_sub(u16::from(horizontal_scrollbar)),
    );
    let (max_vertical, max_horizontal) =
        scroll_limits(content.lines.len(), content.max_width, body);
    ExecutionLayout {
        shell,
        status: chunks[0],
        target_panel,
        target_body,
        log_area: log_inner,
        log_panel,
        separator: Rect::default(),
        body,
        target_max_vertical,
        vertical_scrollbar,
        horizontal_scrollbar,
        max_vertical,
        max_horizontal,
    }
}

fn execution_requested_height(
    area: Rect,
    state: &ExecutionState,
    content: &PreparedContent<'_>,
    status_height: u16,
    compact: bool,
    footer_lines: (&[Line<'static>], &[Line<'static>]),
) -> u16 {
    if compact {
        return shell_layout::required_height(
            status_height.saturating_add(2),
            footer_lines.0,
            footer_lines.1,
        );
    }
    if result_screen(state) {
        let body_height = shell_layout::required_body_height(
            content.lines.len(),
            content.max_width,
            shell_layout::centered_width(area).saturating_sub(2),
        );
        let content_height = status_height
            .saturating_add(1)
            .saturating_add(body_height)
            .saturating_add(2);
        return shell_layout::required_height(content_height, footer_lines.0, footer_lines.1);
    }
    shell_layout::max_centered_height(area)
}

fn compact_shell_area(area: Rect, requested_height: u16) -> Rect {
    let width = shell_layout::centered_width(area);
    let height = requested_height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn status_height(
    state: &ExecutionState,
    status: &[Line<'static>],
    width: u16,
    compact: bool,
) -> u16 {
    if finished_apply(state) {
        status_line_count(status, width)
    } else if compact {
        COMPACT_STATUS_HEIGHT
    } else if state.stage() == ExecutionStage::Applying && !state.is_cancelling() {
        u16::try_from(status.len()).unwrap_or(u16::MAX).max(1)
    } else {
        STATUS_HEIGHT
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

pub(crate) const fn execution_target_scroll_position_with_view(
    view: ExecutionViewState,
    layout: &ExecutionLayout,
) -> (u16, u16) {
    let current = view.target_vertical_offset(0, layout.target_max_vertical());
    (current, layout.target_max_vertical())
}

pub(crate) fn execution_horizontal_scroll_position_with_view(
    view: ExecutionViewState,
    layout: &ExecutionLayout,
) -> (u16, u16) {
    let max = layout.max_horizontal();
    (view.horizontal().min(max), max)
}

fn prepare_content(state: &ExecutionState) -> PreparedContent<'_> {
    let mut lines = log_lines(state.progress().log());
    if lines.is_empty() {
        if finished_apply(state) {
            lines.push(Line::from(Span::styled(
                "No execution output.",
                theme::secondary_style(),
            )));
        } else {
            lines.push(Line::from("Waiting for Terraform output..."));
        }
    }
    let max_width = max_line_width(&lines);
    PreparedContent { lines, max_width }
}

fn prepare_selected_content(
    state: &ExecutionState,
    view: ExecutionViewState,
) -> PreparedContent<'_> {
    let progress = state.progress();
    let log = view
        .selected_target()
        .and_then(|index| progress.targets().get(index))
        .map_or_else(
            || progress.log().iter().collect::<Vec<_>>(),
            |target| {
                target
                    .log_ids()
                    .iter()
                    .filter_map(|id| progress.log().get(*id))
                    .collect()
            },
        );
    let mut lines = log_lines(log);
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            if finished_apply(state) {
                "No execution output."
            } else if view.selected_target().is_some() {
                "Waiting for target output..."
            } else {
                "Waiting for Terraform output..."
            },
            theme::secondary_style(),
        )));
    }
    let max_width = max_line_width(&lines);
    PreparedContent { lines, max_width }
}

fn log_lines<'a>(log: impl IntoIterator<Item = &'a ExecutionLogLine>) -> Vec<Line<'a>> {
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
    lines
}

fn target_line(
    state: &ExecutionState,
    index: usize,
    selected: Option<usize>,
    now: Instant,
    show_previous: bool,
) -> Line<'static> {
    let target = &state.progress().targets()[index];
    let marker = if selected == Some(index) { "> " } else { "  " };
    let status = target_status_label(target.status());
    let action = target
        .actions()
        .iter()
        .map(plan_action_label)
        .collect::<Vec<_>>()
        .join("/");
    let elapsed = target
        .elapsed_at(now)
        .map_or_else(|| "--".to_owned(), format_elapsed);
    let address = padded_target_address(target.address());
    let text = if show_previous {
        let previous = target
            .previous()
            .map_or_else(|| "--".to_owned(), format_elapsed);
        format!("{marker}{address}  {status:<10} {action:<10} {elapsed:>7}  {previous:>7}")
    } else {
        format!("{marker}{address}  {status:<10} {action:<10} {elapsed:>7}")
    };
    let style = if selected == Some(index) {
        theme::accent_style().add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        match target.status() {
            ExecutionTargetStatus::Failed => theme::error_style(),
            ExecutionTargetStatus::Completed => theme::success_style(),
            ExecutionTargetStatus::Incomplete | ExecutionTargetStatus::Skipped => {
                theme::warning_style()
            }
            _ => theme::body_style(),
        }
    };
    Line::from(Span::styled(text, style))
}

fn padded_target_address(address: &str) -> String {
    let address = truncate_middle(address, TARGET_ADDRESS_WIDTH);
    let padding = TARGET_ADDRESS_WIDTH.saturating_sub(Line::from(address.as_str()).width());
    format!("{address}{}", " ".repeat(padding))
}

const fn target_status_label(status: ExecutionTargetStatus) -> &'static str {
    match status {
        ExecutionTargetStatus::Pending => "Pending",
        ExecutionTargetStatus::Running => "Running",
        ExecutionTargetStatus::Completed => "Completed",
        ExecutionTargetStatus::Failed => "Failed",
        ExecutionTargetStatus::Skipped => "Skipped",
        ExecutionTargetStatus::Incomplete => "Incomplete",
    }
}

const fn plan_action_label(action: &PlanAction) -> &'static str {
    match action {
        PlanAction::Create => "create",
        PlanAction::Read => "read",
        PlanAction::Update => "update",
        PlanAction::Delete => "delete",
        PlanAction::NoOp => "no-op",
        PlanAction::Unknown(_) => "unknown",
    }
}

fn apply_status_lines(state: &ExecutionState, now: Instant) -> Vec<Line<'static>> {
    let progress = state.progress();
    let stage_label = match state.stage() {
        ExecutionStage::ApplySucceeded => "Apply complete",
        ExecutionStage::ApplyFailed => "Apply failed",
        ExecutionStage::ApplyInterrupted => "Apply interrupted",
        ExecutionStage::Applying if state.is_cancelling() => "Stopping...",
        ExecutionStage::Applying => "Applying...",
        _ => "Apply",
    };
    let stage_style = match state.stage() {
        ExecutionStage::ApplySucceeded => theme::success_style(),
        ExecutionStage::ApplyFailed => theme::error_style(),
        ExecutionStage::ApplyInterrupted => theme::warning_style(),
        _ => theme::body_style(),
    };
    let summary = format!(
        "    Completed: {}/{}    Elapsed: {}",
        progress.completed_count(),
        progress.targets().len(),
        format_elapsed(state.elapsed_at(now)),
    );
    let counts = format!(
        "Failed: {}    Incomplete: {}    Skipped: {}",
        progress.failed_count(),
        progress.incomplete_count(),
        progress.skipped_count(),
    );
    let warning = state.is_cancelling()
        || matches!(
            state.stage(),
            ExecutionStage::ApplyFailed | ExecutionStage::ApplyInterrupted
        );
    let mut detail = vec![Span::styled(counts, theme::secondary_style())];
    if progress.has_previous() {
        detail.push(Span::styled(
            "    Previous: local success",
            theme::secondary_style(),
        ));
    }
    let mut lines = vec![
        Line::from(vec![
            Span::styled(stage_label, stage_style),
            Span::raw(summary),
        ]),
        Line::from(detail),
    ];
    if warning {
        lines.push(Line::from(Span::styled(
            "Changes may already be applied.",
            theme::warning_style(),
        )));
    }
    lines
}

fn status_lines(
    state: &ExecutionState,
    view: ExecutionViewState,
    now: Instant,
) -> Vec<Line<'static>> {
    if compact_apply(state, view) {
        let elapsed = Line::from(format!("Elapsed {}", format_elapsed(state.elapsed_at(now))));
        if state.is_cancelling() {
            return vec![
                Line::from(vec![
                    Span::styled("Stopping...", theme::body_style()),
                    Span::styled(" Changes may already be applied.", theme::warning_style()),
                ]),
                Line::default(),
                elapsed,
            ];
        }
        return vec![
            running_status_line("Applying...", state, now),
            Line::default(),
            elapsed,
        ];
    }

    if !state.is_cancelling() && finished_apply(state) {
        return completed_apply_status_lines(state, now);
    }

    let status = if state.is_cancelling() {
        if state.is_apply() {
            Line::from("Stopping... Changes may already be applied.")
        } else {
            Line::from("Stopping...")
        }
    } else {
        match state.stage() {
            ExecutionStage::Initializing => running_status_line("Initializing...", state, now),
            ExecutionStage::Planning => running_status_line("Planning...", state, now),
            ExecutionStage::Reading => running_status_line("Reading plan...", state, now),
            ExecutionStage::Applying => running_status_line("Applying...", state, now),
            ExecutionStage::ApplySucceeded => Line::from("Apply complete"),
            ExecutionStage::ApplyFailed => Line::from("Apply failed"),
            ExecutionStage::ApplyInterrupted => Line::from("Apply interrupted"),
            ExecutionStage::Failed => Line::from(state.result().map_or_else(
                || "Terraform failed.".to_owned(),
                |result| format!("Terraform failed: {:?}", result.termination().status),
            )),
        }
    };
    let detail = if state.is_apply() && state.stage() == ExecutionStage::Applying {
        None
    } else if state.is_apply() {
        Some(Line::from(
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
            .to_owned(),
        ))
    } else {
        Some(Line::from(format!(
            "Waiting {}s    Follow: {}",
            state.waiting_at(now).as_secs(),
            if view.follows_latest() { "On" } else { "Off" }
        )))
    };
    let mut lines = vec![status];
    if let Some(detail) = detail {
        lines.push(detail);
    }
    lines.push(Line::from(format!(
        "Elapsed {}",
        format_elapsed(state.elapsed_at(now))
    )));
    lines
}

fn running_status_line(label: &str, state: &ExecutionState, now: Instant) -> Line<'static> {
    let spinner = ['|', '/', '-', '\\']
        [usize::try_from(state.elapsed_at(now).as_millis() / 100).unwrap_or(0) % 4];
    Line::from(vec![
        Span::styled(spinner.to_string(), theme::accent_style()),
        Span::styled(format!(" {label}"), theme::body_style()),
    ])
}

fn completed_apply_status_lines(state: &ExecutionState, now: Instant) -> Vec<Line<'static>> {
    let status = match state.stage() {
        ExecutionStage::ApplySucceeded => Line::from(Span::styled(
            state
                .result()
                .and_then(|result| result.summary_line())
                .map_or_else(|| "Apply complete.".to_owned(), str::to_owned),
            theme::success_style(),
        )),
        ExecutionStage::ApplyFailed => {
            Line::from(Span::styled("Apply failed", theme::error_style()))
        }
        ExecutionStage::ApplyInterrupted => {
            Line::from(Span::styled("Apply interrupted", theme::warning_style()))
        }
        _ => unreachable!("completed apply status should be an apply result"),
    };
    let detail = match state.stage() {
        ExecutionStage::ApplySucceeded => None,
        ExecutionStage::ApplyFailed | ExecutionStage::ApplyInterrupted => Some(Line::from(
            Span::styled("Changes may already be applied.", theme::warning_style()),
        )),
        _ => unreachable!("completed apply detail should be an apply result"),
    };
    let mut lines = vec![status];
    if let Some(detail) = detail {
        lines.push(detail);
    }
    lines.push(Line::from(Span::styled(
        format!("Elapsed {}", format_elapsed(state.elapsed_at(now))),
        theme::secondary_style(),
    )));
    lines
}

const fn finished_apply(state: &ExecutionState) -> bool {
    matches!(
        state.stage(),
        ExecutionStage::ApplySucceeded
            | ExecutionStage::ApplyFailed
            | ExecutionStage::ApplyInterrupted
    )
}

fn result_screen(state: &ExecutionState) -> bool {
    state.stage() == ExecutionStage::Failed || finished_apply(state)
}

fn status_paragraph(status: Vec<Line<'static>>, wrap: bool) -> Paragraph<'static> {
    let paragraph = Paragraph::new(status).style(theme::body_style());
    if wrap {
        paragraph.wrap(Wrap { trim: false })
    } else {
        paragraph
    }
}

fn status_line_count(status: &[Line<'static>], width: u16) -> u16 {
    status_paragraph(status.to_vec(), true)
        .line_count(width)
        .try_into()
        .unwrap_or(u16::MAX)
        .max(1)
}

fn footer_lines(
    state: &ExecutionState,
    view: ExecutionViewState,
    width: u16,
    notice: Option<&str>,
) -> Vec<Line<'static>> {
    let items = if compact_apply(state, view) {
        vec![
            footer::hint(&["Ctrl-C"], "cancel"),
            footer::hint(&["v"], "logs"),
        ]
    } else if state.stage() == ExecutionStage::Failed || finished_apply(state) {
        vec![
            footer::hint(&["q", "Ctrl-C"], "quit"),
            footer::hint(&["↑", "↓", "PgUp", "PgDn"], "scroll"),
            footer::hint(
                &["y"],
                if finished_apply(state) {
                    "yank result"
                } else {
                    "copy diagnostic"
                },
            ),
        ]
    } else if state.stage() == ExecutionStage::Applying && view.logs_open() {
        vec![
            footer::hint(&["Ctrl-C"], "cancel"),
            footer::hint(&["Esc"], "close"),
            footer::hint(&["↑", "↓", "PgUp", "PgDn"], "scroll"),
            footer::hint(&["End"], "follow latest"),
        ]
    } else {
        vec![
            footer::hint(&["Ctrl-C"], "cancel"),
            footer::hint(&["↑", "↓", "PgUp", "PgDn"], "scroll"),
            footer::hint(&["End"], "follow latest"),
        ]
    };
    footer::layout_with_notice(items, width, notice)
}

fn apply_footer_lines(
    state: &ExecutionState,
    view: ExecutionViewState,
    width: u16,
    notice: Option<&str>,
) -> Vec<Line<'static>> {
    let items = if finished_apply(state) {
        vec![
            footer::hint(&["q", "Ctrl-C"], "quit"),
            footer::hint(
                &["↑", "↓"],
                if view.logs_open() {
                    "scroll log"
                } else {
                    "select"
                },
            ),
            footer::hint(&["Tab"], "focus"),
            footer::hint(&["y"], "yank result"),
        ]
    } else {
        vec![
            footer::hint(&["Ctrl-C"], "cancel"),
            footer::hint(
                &["↑", "↓", "j", "k"],
                if view.logs_open() {
                    "scroll log"
                } else {
                    "select"
                },
            ),
            footer::hint(&["Tab"], "focus"),
            footer::hint(&["End"], "follow latest"),
        ]
    };
    footer::layout_with_notice(items, width, notice)
}

fn apply_required_footer_lines(
    state: &ExecutionState,
    width: u16,
    notice: Option<&str>,
) -> Vec<Line<'static>> {
    let item = if finished_apply(state) {
        footer::hint(&["q", "Ctrl-C"], "quit")
    } else {
        footer::hint(&["Ctrl-C"], "cancel")
    };
    footer::layout_with_notice(vec![item], width, notice)
}

fn required_footer_lines(
    state: &ExecutionState,
    width: u16,
    notice: Option<&str>,
) -> Vec<Line<'static>> {
    let item = if state.stage() == ExecutionStage::Failed || finished_apply(state) {
        footer::hint(&["q", "Ctrl-C"], "quit")
    } else {
        footer::hint(&["Ctrl-C"], "cancel")
    };
    footer::layout_with_notice(vec![item], width, notice)
}

fn compact_apply(state: &ExecutionState, view: ExecutionViewState) -> bool {
    state.stage() == ExecutionStage::Applying && !view.logs_open()
}

fn layout_target_max(target_count: usize, height: u16) -> u16 {
    u16::try_from(target_count.saturating_sub(usize::from(height))).unwrap_or(u16::MAX)
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
        ApplyStatus, Diagnostic, DiagnosticSeverity, DiagnosticSource, ExecutionAction,
        ExecutionContext, ExecutionEvent, ExecutionEventKind, ExecutionLogLine,
        ExecutionTargetSpec, ResourceAction, ResourceEvent, ResourceEventKind,
    };
    use crate::app::session::{self, Action, SessionState};
    use crate::ui::features::execution::ExecutionScroll;
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

    fn render_execution_with_view(
        frame: &mut Frame<'_>,
        state: &ExecutionState,
        view: ExecutionViewState,
        now: Instant,
    ) {
        super::render_execution_with_quit_confirmation(frame, state, view, now, false);
    }

    fn execution_layout(area: Rect, state: &ExecutionState) -> ExecutionLayout {
        execution_layout_with_view(area, state, ExecutionViewState::default())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the fixture covers the complete apply result event stream"
    )]
    fn apply_state(status: ApplyStatus) -> (ExecutionState, Instant) {
        let started_at = Instant::now();
        let finished_at = started_at + Duration::from_secs(4);
        let mut state = ExecutionState::applying_with_previous(
            started_at,
            ExecutionContext::loading("/repo/environments/production/main")
                .with_workspace("default"),
            vec![
                ExecutionTargetSpec {
                    address: "terraform_data.api".to_owned(),
                    actions: vec![PlanAction::Update],
                },
                ExecutionTargetSpec {
                    address: "terraform_data.worker".to_owned(),
                    actions: vec![PlanAction::Delete, PlanAction::Create],
                },
                ExecutionTargetSpec {
                    address: "terraform_data.old".to_owned(),
                    actions: vec![PlanAction::Delete],
                },
                ExecutionTargetSpec {
                    address: "terraform_data.new".to_owned(),
                    actions: vec![PlanAction::Create],
                },
            ],
            Vec::new(),
            &[
                Some(Duration::from_secs(11)),
                None,
                None,
                Some(Duration::from_secs(4)),
            ],
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
        for (address, action) in [
            ("terraform_data.api", ResourceAction::Update),
            ("terraform_data.worker", ResourceAction::Delete),
            ("terraform_data.worker", ResourceAction::Create),
            ("terraform_data.old", ResourceAction::Delete),
            ("terraform_data.new", ResourceAction::Create),
        ] {
            state.record(ExecutionEvent {
                received_at: started_at,
                kind: ExecutionEventKind::Resource(ResourceEvent {
                    address: address.to_owned(),
                    kind: ResourceEventKind::ApplyStart,
                    action: Some(action.clone()),
                    message: Some(format!("{address}: {action:?} started")),
                }),
            });
            state.record(ExecutionEvent {
                received_at: started_at + Duration::from_secs(1),
                kind: ExecutionEventKind::Resource(ResourceEvent {
                    address: address.to_owned(),
                    kind: ResourceEventKind::ApplyComplete,
                    action: Some(action),
                    message: Some(format!("{address}: apply complete")),
                }),
            });
        }
        if status == ApplyStatus::Failed {
            state.record(ExecutionEvent {
                received_at: started_at + Duration::from_secs(2),
                kind: ExecutionEventKind::Resource(ResourceEvent {
                    address: "terraform_data.api".to_owned(),
                    kind: ResourceEventKind::ApplyErrored,
                    action: Some(ResourceAction::Update),
                    message: None,
                }),
            });
            state.record(ExecutionEvent {
                received_at: started_at + Duration::from_secs(3),
                kind: ExecutionEventKind::Diagnostic(Diagnostic {
                    severity: DiagnosticSeverity::Error,
                    summary: "AccessDenied: synthetic provider rejected the request".to_owned(),
                    detail: None,
                    address: Some("terraform_data.api".to_owned()),
                    position: None,
                    source: DiagnosticSource::Terraform,
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
            None,
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
                0 => "a deliberately long synthetic apply line keeps horizontal scrolling visible after the result is complete".to_owned(),
                1 => "Warning: synthetic provider emitted a non-blocking diagnostic".to_owned(),
                3 => "Error: initial failure".to_owned(),
                39 => "tail marker".to_owned(),
                _ => format!("log line {index}"),
            };
            let stream = if index == 1 {
                EventStream::Stderr
            } else {
                EventStream::Stdout
            };
            state.record(ExecutionEvent {
                received_at: started_at,
                kind: ExecutionEventKind::Log(ExecutionLogLine { stream, text }),
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
    fn renders_apply_progress_stopping_and_log_view_vrt_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let (state, now) = applying_state_with_content(6, 16);
            let compact = render_to_buffer((width, height), |frame| {
                render_execution_with_view(frame, &state, ExecutionViewState::default(), now);
            });
            snapshot(&format!("ux12r_{width}x{height}_apply-progress"), &compact);

            let mut stopping_state = state.clone();
            stopping_state.apply(ExecutionAction::RequestCancellation);
            let stopping = render_to_buffer((width, height), |frame| {
                render_execution_with_view(
                    frame,
                    &stopping_state,
                    ExecutionViewState::default(),
                    now,
                );
            });
            snapshot(&format!("ux12r_{width}x{height}_apply-stopping"), &stopping);

            let mut view = ExecutionViewState::default();
            view.open_logs();
            let logs = render_to_buffer((width, height), |frame| {
                render_execution_with_view(frame, &state, view, now);
            });
            snapshot(&format!("ux12r_{width}x{height}_apply-logs"), &logs);
        }
    }

    #[test]
    fn renders_apply_quit_confirmation_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let (state, now) = apply_state(ApplyStatus::Succeeded);
            let buffer = render_to_buffer((width, height), |frame| {
                render_execution_with_quit_confirmation(
                    frame,
                    &state,
                    ExecutionViewState::default(),
                    now,
                    true,
                );
            });

            snapshot(
                &format!("preview_{width}x{height}_quit-confirmation"),
                &buffer,
            );
        }
    }

    #[test]
    fn apply_stopping_at_minimum_size_shows_resize_notice() {
        let (state, now) = applying_state_with_content(1, 1);
        let mut stopping_state = state;
        stopping_state.apply(ExecutionAction::RequestCancellation);
        let buffer = render_to_buffer((32, 9), |frame| {
            render_execution_with_view(frame, &stopping_state, ExecutionViewState::default(), now);
        });

        snapshot("ux12r_32x9_apply-stopping", &buffer);
        let text = buffer_text(&buffer);
        assert!(text.contains("Terminal too small."));
        assert!(!text.contains("Stopping..."));
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

    mod layout {
        use super::*;

        fn execution_layout_with_quit_confirmation(
            area: Rect,
            state: &ExecutionState,
            quit_confirmation: bool,
        ) -> ExecutionLayout {
            super::execution_layout_with_quit_confirmation_and_view(
                area,
                state,
                ExecutionViewState::default(),
                quit_confirmation,
            )
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

        fn execution_buffer_at(
            area: Rect,
            state: &ExecutionState,
            now: Instant,
            vertical: u16,
            horizontal: u16,
        ) -> (ExecutionLayout, Buffer) {
            let mut view = ExecutionViewState::default();
            view.open_logs();
            let layout = execution_layout_with_view(area, state, view);
            let mut current_vertical = 0;
            view.apply_scroll(
                ExecutionScroll::Top,
                current_vertical,
                layout.max_vertical(),
                layout.body().height,
            );
            for _ in 0..vertical {
                view.apply_scroll(
                    ExecutionScroll::Down,
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
                ExecutionScroll::LeftEdge,
                current_horizontal,
                layout.max_horizontal(),
                current_vertical,
            );
            for _ in 0..horizontal {
                view.apply_horizontal_scroll(
                    ExecutionScroll::Right,
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
                let track_end = if symbols.last().is_some_and(|symbol| symbol == "▶︎") {
                    symbols.len() - 1
                } else {
                    symbols.len()
                };
                assert_thumb_segments(
                    &symbols[1..track_end],
                    "─",
                    "═",
                    usize::from(horizontal),
                    usize::from(layout.max_horizontal()),
                );
            }
        }

        fn assert_thumb_segments(
            track: &[String],
            _track_symbol: &str,
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
                    .all(|symbol| symbol != thumb_symbol)
            );
            assert!(
                track[thumb_end + 1..]
                    .iter()
                    .all(|symbol| symbol != thumb_symbol)
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

        #[test]
        fn quit_confirmation_replaces_the_result_footer_and_has_a_narrow_notice() {
            let (state, now) = apply_state(ApplyStatus::Succeeded);
            let buffer = render_to_buffer((80, 24), |frame| {
                render_execution_with_quit_confirmation(
                    frame,
                    &state,
                    ExecutionViewState::default(),
                    now,
                    true,
                );
            });
            let text = buffer_text(&buffer);
            assert!(text.contains("Quit Terracotta?   [Enter] Quit   [Esc] Cancel"));
            assert!(!text.contains("q/Ctrl-C quit"));

            let narrow = render_to_buffer((32, 9), |frame| {
                render_execution_with_quit_confirmation(
                    frame,
                    &state,
                    ExecutionViewState::default(),
                    now,
                    true,
                );
            });
            assert!(buffer_text(&narrow).contains("Quit? Enter exit / Esc cancel"));
        }

        #[test]
        fn quit_confirmation_preserves_the_execution_body_and_scroll_limits() {
            let (state, _) = apply_state(ApplyStatus::Succeeded);
            let area = Rect::new(0, 0, 50, 24);
            let normal = execution_layout(area, &state);
            let waiting = execution_layout_with_quit_confirmation(area, &state, true);

            assert!(
                footer_lines(
                    &state,
                    ExecutionViewState::default(),
                    shell_layout::centered_width(area),
                    None,
                )
                .len()
                    >= 2
            );
            assert_eq!(waiting.body(), normal.body());
            assert_eq!(waiting.max_vertical(), normal.max_vertical());
            assert_eq!(waiting.max_horizontal(), normal.max_horizontal());
        }

        #[test]
        fn production_execution_render_draws_shell_scrollbars_and_stream_colors() {
            let (state, now) = long_apply_state(ApplyStatus::Succeeded);
            let area = Rect::new(0, 0, 80, 24);
            let layout = execution_layout(area, &state);
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render_execution_with_view(frame, &state, ExecutionViewState::default(), now);
            });
            let mut top_view = ExecutionViewState::default();
            top_view.apply_scroll(
                ExecutionScroll::Top,
                0,
                layout.max_vertical(),
                layout.body().height,
            );
            let top_buffer = render_to_buffer((area.width, area.height), |frame| {
                render_execution_with_view(frame, &state, top_view, now);
            });

            assert_shell_frame_and_footer(
                &buffer,
                layout.shell.content(),
                layout.shell.footer(),
                "y yank result",
            );
            let text = buffer_text(&buffer);
            assert!(text.contains("Apply result"));
            assert!(text.contains("Apply complete"));
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
                &top_buffer,
                "Warning: synthetic provider emitted a non-blocking diagnostic",
                Color::Rgb(0xeb, 0xcb, 0x8b),
                Modifier::BOLD,
            );
        }

        #[test]
        fn production_execution_scrollbars_reach_offsets_after_resize_and_single_overflow() {
            let (state, now) = long_apply_state(ApplyStatus::Succeeded);
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
                    let (layout, buffer) =
                        execution_buffer_at(area, &state, now, vertical, horizontal);
                    assert_scrollbar_positions(&buffer, &layout, vertical, horizontal);
                }
            }

            let area = Rect::new(0, 0, 80, 24);
            let (base_state, _) = applying_state_with_content(1, 1);
            let mut logs_view = ExecutionViewState::default();
            logs_view.open_logs();
            let available = execution_layout_with_view(area, &base_state, logs_view).log_area();

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
    }

    mod scroll {
        use super::*;

        #[test]
        fn reopening_apply_logs_starts_at_the_newest_line() {
            let started_at = Instant::now();
            let mut state =
                ExecutionState::applying(started_at, ExecutionContext::loading("/repo"));
            for text in ["first", "second", "tail"] {
                state.record(ExecutionEvent {
                    received_at: started_at,
                    kind: ExecutionEventKind::Log(ExecutionLogLine {
                        stream: EventStream::Stdout,
                        text: text.to_owned(),
                    }),
                });
            }

            let mut view = ExecutionViewState::default();
            view.open_logs();
            view.apply_scroll(ExecutionScroll::Top, 0, 2, 1);
            view.close_logs();
            view.open_logs();

            assert!(view.logs_open());
            assert!(view.follows_latest());
            let layout = execution_layout_with_view(Rect::new(0, 0, 80, 24), &state, view);
            assert_eq!(
                view.vertical_offset(0, layout.max_vertical()),
                layout.max_vertical()
            );
        }

        #[test]
        fn cancelling_apply_keeps_the_warning_and_cancel_action_in_both_views() {
            let started_at = Instant::now();
            let mut state =
                ExecutionState::applying(started_at, ExecutionContext::loading("/repo"));
            state.record(ExecutionEvent {
                received_at: started_at,
                kind: ExecutionEventKind::Log(ExecutionLogLine {
                    stream: EventStream::Stdout,
                    text: "Applying saved plan...".to_owned(),
                }),
            });
            state.apply(ExecutionAction::RequestCancellation);

            let compact = render_to_buffer((80, 24), |frame| {
                render_execution_with_view(
                    frame,
                    &state,
                    ExecutionViewState::default(),
                    started_at,
                );
            });
            let mut logs_view = ExecutionViewState::default();
            logs_view.open_logs();
            let logs = render_to_buffer((80, 24), |frame| {
                render_execution_with_view(frame, &state, logs_view, started_at);
            });

            for buffer in [&compact, &logs] {
                let text = buffer_text(buffer);
                assert!(text.contains("Stopping..."));
                assert!(text.contains("Changes may already be applied."));
                assert!(text.contains("Ctrl-C cancel"));
                assert!(text.contains("Tab focus"));
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
    }

    mod result {
        use super::*;

        #[test]
        fn production_execution_failure_render_draws_diagnostic_color() {
            let (state, now) = apply_state(ApplyStatus::Failed);
            let buffer = render_to_buffer((80, 24), |frame| {
                render_execution_with_view(frame, &state, ExecutionViewState::default(), now);
            });

            assert!(
                buffer_text(&buffer)
                    .contains("AccessDenied: synthetic provider rejected the request")
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
        fn completed_apply_statuses_use_their_result_styles() {
            struct StatusCase {
                name: &'static str,
                status: ApplyStatus,
                headline: &'static str,
                headline_color: Color,
                warning: Option<&'static str>,
            }

            for case in [
                StatusCase {
                    name: "success_status",
                    status: ApplyStatus::Succeeded,
                    headline: "Apply complete",
                    headline_color: Color::Rgb(0xa3, 0xbe, 0x8c),
                    warning: None,
                },
                StatusCase {
                    name: "failure_status",
                    status: ApplyStatus::Failed,
                    headline: "Apply failed",
                    headline_color: Color::Rgb(0xbf, 0x61, 0x6a),
                    warning: Some("Changes may already be applied."),
                },
                StatusCase {
                    name: "interrupted_status",
                    status: ApplyStatus::Interrupted,
                    headline: "Apply interrupted",
                    headline_color: Color::Rgb(0xeb, 0xcb, 0x8b),
                    warning: Some("Changes may already be applied."),
                },
            ] {
                let (state, now) = apply_state(case.status);
                let area = Rect::new(0, 0, 80, 24);
                let layout = execution_layout(area, &state);
                let buffer = render_to_buffer((area.width, area.height), |frame| {
                    render_execution_with_view(frame, &state, ExecutionViewState::default(), now);
                });
                let headline = find_text_cell(&buffer, layout.status(), case.headline);

                assert_eq!(headline.fg, case.headline_color, "case: {}", case.name);
                assert!(
                    headline.modifier.contains(Modifier::BOLD),
                    "case: {}",
                    case.name
                );
                if let Some(warning) = case.warning {
                    let warning_cell = find_text_cell(&buffer, layout.status(), warning);
                    assert_eq!(
                        warning_cell.fg,
                        Color::Rgb(0xeb, 0xcb, 0x8b),
                        "case: {}",
                        case.name
                    );
                    assert!(
                        warning_cell.modifier.contains(Modifier::BOLD),
                        "case: {}",
                        case.name
                    );
                }
            }
        }

        #[test]
        fn terraform_summary_stays_in_the_log_without_an_appended_copy() {
            let now = Instant::now();
            let summary = "Apply complete! Resources: 1 added, 0 changed, 0 destroyed.";
            let mut state = ExecutionState::applying(now, ExecutionContext::loading("/project"));
            state.record(ExecutionEvent {
                received_at: now,
                kind: ExecutionEventKind::Log(ExecutionLogLine {
                    stream: EventStream::Stdout,
                    text: summary.to_owned(),
                }),
            });
            state.finish_apply(
                ApplyStatus::Succeeded,
                Some(summary.to_owned()),
                None,
                now + Duration::from_secs(1),
            );

            let buffer = render_to_buffer((80, 24), |frame| {
                render_execution_with_view(
                    frame,
                    &state,
                    ExecutionViewState::default(),
                    now + Duration::from_secs(1),
                );
            });

            assert_eq!(buffer_text(&buffer).matches(summary).count(), 1);
            assert_eq!(
                prepare_content(&state)
                    .lines
                    .iter()
                    .map(Line::to_string)
                    .filter(|line| line == summary)
                    .count(),
                1
            );
        }

        #[test]
        fn completed_apply_without_log_shows_a_distinct_empty_output_message() {
            let started_at = Instant::now();
            let mut state = ExecutionState::applying(
                started_at,
                ExecutionContext::loading("/repo/environments/production/main"),
            );
            state.finish_apply(
                ApplyStatus::Succeeded,
                None,
                None,
                started_at + Duration::from_secs(1),
            );

            let buffer = render_to_buffer((80, 24), |frame| {
                render_execution_with_view(
                    frame,
                    &state,
                    ExecutionViewState::default(),
                    started_at + Duration::from_secs(1),
                );
            });
            let text = buffer_text(&buffer);

            assert!(text.contains("Apply result"));
            assert!(text.contains("Apply complete"));
            assert!(text.contains("No execution output."));
            assert!(!text.contains("Waiting for Terraform output..."));
        }

        #[test]
        fn completed_apply_wraps_the_fixed_warning_before_the_log_separator() {
            let (state, now) = apply_state(ApplyStatus::Failed);
            let area = Rect::new(0, 0, 32, 24);
            let layout = execution_layout(area, &state);
            let mut view = ExecutionViewState::default();
            view.apply_scroll(
                ExecutionScroll::Top,
                0,
                layout.max_vertical(),
                layout.body().height,
            );
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render_execution_with_view(frame, &state, view, now);
            });
            let text = buffer_text(&buffer);

            assert!(layout.body().height > 0);
            assert!(layout.status().height >= 3);
            assert!(text.contains("Apply result"));
            assert!(text.contains("Changes may already be"));
            assert!(layout.log_area().y > layout.status().y);
        }

        #[test]
        fn completed_apply_keeps_all_wrapped_summary_lines_before_the_log() {
            let now = Instant::now();
            let summary = "Resources: 12345 added, 67890 changed, 12345 destroyed.";
            let mut state = ExecutionState::applying(now, ExecutionContext::loading("/project"));
            state.record(ExecutionEvent {
                received_at: now,
                kind: ExecutionEventKind::Log(ExecutionLogLine {
                    stream: EventStream::Stdout,
                    text: "log output".to_owned(),
                }),
            });
            state.finish_apply(
                ApplyStatus::Succeeded,
                Some(summary.to_owned()),
                None,
                now + Duration::from_secs(1),
            );

            let area = Rect::new(0, 0, 40, 24);
            let layout = execution_layout(area, &state);
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render_execution_with_view(
                    frame,
                    &state,
                    ExecutionViewState::default(),
                    now + Duration::from_secs(1),
                );
            });
            let text = buffer_text(&buffer);

            assert!(layout.status().height >= APPLY_STATUS_HEIGHT);
            assert!(layout.log_area().height > 0);
            assert!(text.contains("Elapsed: 1.0s"));
            assert!(text.contains("log output"));
        }
    }

    mod progress {
        use super::*;

        #[test]
        fn running_apply_keeps_the_compact_frame_fixed_as_logs_and_state_change() {
            for &(width, height) in &SIZES {
                let (empty_state, _) = applying_state_with_content(0, 0);
                let (short_state, _) = applying_state_with_content(1, 1);
                let (long_state, _) = applying_state_with_content(40, 1);
                let mut stopping_state = short_state.clone();
                stopping_state.apply(ExecutionAction::RequestCancellation);
                let area = Rect::new(0, 0, width, height);
                let empty_layout = execution_layout(area, &empty_state);
                let short_layout = execution_layout(area, &short_state);
                let long_layout = execution_layout(area, &long_state);
                let stopping_layout = execution_layout(area, &stopping_state);

                assert_eq!(empty_layout.shell.content(), short_layout.shell.content());
                assert_eq!(short_layout.shell.content(), long_layout.shell.content());
                assert_eq!(
                    short_layout.shell.content(),
                    stopping_layout.shell.content()
                );
                assert_eq!(empty_layout.shell.footer(), short_layout.shell.footer());
                assert_eq!(short_layout.shell.footer(), long_layout.shell.footer());
                assert_eq!(short_layout.shell.footer(), stopping_layout.shell.footer());
                assert_eq!(short_layout.status(), long_layout.status());
                assert!(stopping_layout.status().height >= short_layout.status().height);
                assert!(short_layout.shell.footer().bottom() > short_layout.shell.header().y);

                let long_buffer = render_to_buffer((width, height), |frame| {
                    render_execution_with_view(
                        frame,
                        &long_state,
                        ExecutionViewState::default(),
                        Instant::now(),
                    );
                });
                let long_text = buffer_text(&long_buffer);
                assert!(long_text.contains("Applying"));
                assert!(long_text.contains('x'));
                assert!(!long_text.contains("Waiting for Terraform output..."));

                let stopping_buffer = render_to_buffer((width, height), |frame| {
                    render_execution_with_view(
                        frame,
                        &stopping_state,
                        ExecutionViewState::default(),
                        Instant::now(),
                    );
                });
                let stopping_text = buffer_text(&stopping_buffer);
                assert!(stopping_text.contains("Stopping..."));
                assert!(stopping_text.contains("Changes may already be applied."));
            }
        }

        #[test]
        fn running_status_keeps_fixed_height_when_following_is_off() {
            let started_at = Instant::now();
            let now = started_at + Duration::from_secs(10_000);
            let state =
                ExecutionState::with_context(started_at, ExecutionContext::loading("/project"));
            let area = Rect::new(0, 0, 32, 24);
            let layout = execution_layout(area, &state);
            let mut view = ExecutionViewState::default();
            view.apply_scroll(
                ExecutionScroll::Down,
                0,
                layout.max_vertical(),
                layout.body().height,
            );
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render_execution_with_view(frame, &state, view, now);
            });

            assert_eq!(layout.status().height, STATUS_HEIGHT);
            assert_eq!(layout.log_area().y, layout.status().bottom());
            assert_eq!(layout.separator().y, layout.log_area().bottom());
            let text = buffer_text(&buffer);
            assert!(text.contains("Elapsed 10000.0s"), "{text}");
        }

        #[test]
        fn running_status_cycles_the_ascii_spinner_without_repeating_apply_progress() {
            let started_at = Instant::now();
            let state =
                ExecutionState::with_context(started_at, ExecutionContext::loading("/project"));
            let frames = ["|", "/", "-", "\\"];

            for (index, frame) in frames.into_iter().enumerate() {
                let status = status_lines(
                    &state,
                    ExecutionViewState::default(),
                    started_at + Duration::from_millis(u64::try_from(index).unwrap() * 100),
                );
                assert_eq!(status[0].to_string(), format!("{frame} Initializing..."));
            }

            let applying =
                ExecutionState::applying(started_at, ExecutionContext::loading("/project"));
            let status = status_lines(
                &applying,
                ExecutionViewState::default(),
                started_at + Duration::from_millis(100),
            );
            assert_eq!(status.len(), 3);
            assert_eq!(status[0].to_string(), "/ Applying...");
            assert!(
                status
                    .iter()
                    .all(|line| !line.to_string().contains("Applying...") || line == &status[0])
            );
        }

        #[test]
        fn append_only_log_is_rendered_in_receive_order() {
            let now = Instant::now();
            let mut state =
                ExecutionState::with_context(now, ExecutionContext::loading("/project"));
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
    }

    mod copy {
        use super::*;

        #[test]
        fn production_execution_copy_flash_uses_accent_background_then_restores_log_style() {
            let started_at = Instant::now();
            let mut state =
                ExecutionState::applying(started_at, ExecutionContext::loading("/repo"));
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
            let mut view = ExecutionViewState::default();
            view.open_logs();
            let before = render_to_buffer((80, 24), |frame| {
                render_execution_with_view(
                    frame,
                    session.execution().expect("execution should be visible"),
                    view,
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
                render_execution_with_view(frame, state, view, started_at);
            });
            let after = render_to_buffer((80, 24), |frame| {
                render_execution_with_view(
                    frame,
                    state,
                    view,
                    started_at + Duration::from_millis(201),
                );
            });

            let body = execution_layout_with_view(Rect::new(0, 0, 80, 24), state, view).body();
            let flash_cell = find_text_cell(&flash, body, "terraform apply review.tfplan");
            assert_eq!(flash_cell.fg, Color::Rgb(0x11, 0x14, 0x19));
            assert_eq!(flash_cell.bg, Color::Rgb(0xf4, 0x9e, 0x4c));
            let before_cell = find_text_cell(&before, body, "terraform apply review.tfplan");
            let after_cell = find_text_cell(&after, body, "terraform apply review.tfplan");
            assert_eq!(after_cell, before_cell);
        }

        #[test]
        fn completed_apply_statuses_keep_full_log_order_for_render_and_copy() {
            struct ApplyCase {
                name: &'static str,
                status: ApplyStatus,
                expected_headline: &'static str,
                expects_warning: bool,
            }

            for case in [
                ApplyCase {
                    name: "succeeded",
                    status: ApplyStatus::Succeeded,
                    expected_headline: "Apply complete.",
                    expects_warning: false,
                },
                ApplyCase {
                    name: "failed",
                    status: ApplyStatus::Failed,
                    expected_headline: "Apply failed.",
                    expects_warning: true,
                },
                ApplyCase {
                    name: "interrupted",
                    status: ApplyStatus::Interrupted,
                    expected_headline: "Apply interrupted.",
                    expects_warning: true,
                },
            ] {
                let now = Instant::now();
                let mut state =
                    ExecutionState::applying(now, ExecutionContext::loading("/project"));
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
                let copied = state
                    .copy_effect(CopyTarget::Execution)
                    .expect("completed apply should be copyable")
                    .text()
                    .to_owned();
                assert!(
                    copied.starts_with(case.expected_headline),
                    "case: {}",
                    case.name
                );
                assert!(copied.contains("Completed: 0/0"), "case: {}", case.name);
                assert!(copied.contains("Elapsed: 1.0s"), "case: {}", case.name);
                assert_eq!(
                    case.expects_warning,
                    copied.contains("Changes may already be applied."),
                    "case: {}",
                    case.name
                );
                assert!(
                    copied.ends_with("first\nsecond\nthird"),
                    "case: {}",
                    case.name
                );
            }
        }
    }
}
