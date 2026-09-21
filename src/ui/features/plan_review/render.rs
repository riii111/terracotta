use std::time::Instant;

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{
    copy::CopyNotice,
    execution::DiagnosticSeverity,
    review::{FilteredPlan, PlanLineKind, PlanReview},
    session::{ApplyConfirmationState, ReviewSessionState},
};
use crate::ui::primitives::{
    atoms::{scrollbar, separator},
    molecules::terminal_notice,
};
use crate::ui::shell::{footer, header, layout as shell_layout};
use crate::ui::theme;

use super::{ApplyConfirmationViewState, PlanReviewMatch, PlanReviewViewState};

const MIN_WIDTH: u16 = 24;
const MIN_HEIGHT: u16 = 6;
const CONFIRMATION_MAX_WIDTH: u16 = 80;
const CONFIRMATION_HEADER_HEIGHT: u16 = 2;
const CONFIRMATION_NOTICE: &str = "Terminal too small. Resize or press Esc to go back.";
struct PreparedContent<'a> {
    lines: Vec<Line<'a>>,
    metrics: ContentMetrics,
    sources: Vec<Option<PlanSource<'a>>>,
    matches: Vec<PlanReviewMatch>,
}

#[derive(Clone, Copy)]
struct ContentMetrics {
    line_count: usize,
    max_width: usize,
}

impl PreparedContent<'_> {
    const fn metrics(&self) -> ContentMetrics {
        self.metrics
    }
}

#[derive(Clone, Copy)]
struct PlanSource<'a> {
    text: &'a str,
    kind: PlanLineKind,
}

pub(crate) struct ApplyConfirmationLayout {
    header: Rect,
    notice: Rect,
    frame: Rect,
    footer: Rect,
    inner: Rect,
    input: Rect,
    lines: Vec<Line<'static>>,
    footer_lines: Vec<Line<'static>>,
    renderable: bool,
}

impl ApplyConfirmationLayout {
    pub(crate) const fn header(&self) -> Rect {
        self.header
    }

    pub(crate) const fn notice(&self) -> Rect {
        self.notice
    }

    pub(crate) const fn frame(&self) -> Rect {
        self.frame
    }

    pub(crate) const fn footer(&self) -> Rect {
        self.footer
    }

    pub(crate) const fn inner(&self) -> Rect {
        self.inner
    }

    pub(crate) const fn input(&self) -> Rect {
        self.input
    }

    pub(crate) fn lines(&self) -> &[Line<'static>] {
        &self.lines
    }

    pub(crate) fn footer_lines(&self) -> &[Line<'static>] {
        &self.footer_lines
    }

    pub(crate) const fn renderable(&self) -> bool {
        self.renderable
    }
}

pub(crate) struct PlanReviewLayout {
    shell: shell_layout::ShellLayout,
    body: Rect,
    status: Rect,
    separator: Rect,
    footer_status: Option<(String, Style)>,
    vertical_scrollbar: bool,
    horizontal_scrollbar: bool,
    max_vertical: u16,
    max_horizontal: u16,
    matches: Vec<PlanReviewMatch>,
}

impl PlanReviewLayout {
    pub(crate) const fn body(&self) -> Rect {
        self.body
    }

    pub(crate) const fn status(&self) -> Rect {
        self.status
    }

    pub(crate) const fn separator(&self) -> Rect {
        self.separator
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

    pub(crate) fn matches(&self) -> &[PlanReviewMatch] {
        &self.matches
    }
}

pub(crate) fn layout(area: Rect, searching: bool, state: &ReviewSessionState) -> PlanReviewLayout {
    layout_with_quit_confirmation(area, searching, state, false)
}

pub(crate) fn layout_with_quit_confirmation(
    area: Rect,
    searching: bool,
    state: &ReviewSessionState,
    quit_confirmation: bool,
) -> PlanReviewLayout {
    let filtered_view = filter_active(searching, state);
    let (content, base_metrics) = prepare_view_content(state, filtered_view);
    layout_with_content(
        area,
        searching,
        state,
        &content,
        base_metrics,
        state.copy_notice(),
        quit_confirmation,
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "the plan layout keeps all width, height, footer, and scroll calculations together"
)]
fn layout_with_content(
    area: Rect,
    searching: bool,
    state: &ReviewSessionState,
    content: &PreparedContent<'_>,
    base_metrics: ContentMetrics,
    copy_notice: Option<CopyNotice>,
    quit_confirmation: bool,
) -> PlanReviewLayout {
    let panel_width = shell_layout::centered_width(area);
    let content_metrics = content.metrics();
    let filter_visible = filter_active(searching, state);
    let showing = filter_footer_status(
        state.review().search_query(),
        content.matches.len(),
        panel_width,
    );
    let footer_status = copy_notice
        .map(|notice| {
            (
                notice.message().to_owned(),
                if matches!(notice, CopyNotice::Failed) {
                    theme::error_style()
                } else {
                    theme::accent_style()
                },
            )
        })
        .or_else(|| {
            if filter_visible && !quit_confirmation {
                showing
                    .as_ref()
                    .map(|message| (message.clone(), theme::secondary_style()))
            } else {
                None
            }
        });
    let footer_message = footer_status.as_ref().map(|(message, _)| message.as_str());
    let normal_footer_lines = footer::layout_with_notice(
        footer_items(
            searching,
            state.review().metadata().applyable(),
            content.matches.len(),
            filter_visible,
        ),
        panel_width,
        footer_message,
    );
    let normal_required = footer::layout_with_notice(
        required_footer_items(searching, content.matches.len(), filter_visible),
        panel_width,
        footer_message,
    );
    let inner_width = panel_width.saturating_sub(2);
    let body_height = shell_layout::required_body_height(
        base_metrics.line_count,
        base_metrics.max_width,
        inner_width,
    );
    let fixed_status_height: u16 = 2;
    let content_height = 2u16
        .saturating_add(fixed_status_height)
        .saturating_add(body_height);
    let footer_height = common_footer_height(
        state.review().metadata().applyable(),
        content.matches.len(),
        panel_width,
        copy_notice.map(CopyNotice::message),
        showing.as_deref(),
    );
    let frame_footer_lines = footer::pad_lines(normal_footer_lines, footer_height);
    let frame_required = footer::pad_lines(normal_required, footer_height);
    let requested_height =
        shell_layout::required_height(content_height, &frame_footer_lines, &frame_required);
    let panel = shell_layout::centered_area(area, requested_height);
    let footer_lines = if quit_confirmation {
        footer::pad_lines(
            footer::quit_confirmation_lines(panel_width, copy_notice.map(CopyNotice::message)),
            footer_height,
        )
    } else {
        frame_footer_lines
    };
    let required = if quit_confirmation {
        footer::pad_lines(
            footer::quit_confirmation_lines(panel_width, copy_notice.map(CopyNotice::message)),
            footer_height,
        )
    } else {
        frame_required
    };
    let shell = shell_layout::layout(panel, footer_lines, required, 1);
    let inner = shell.content_inner();
    let status = Rect::new(inner.x, inner.y, inner.width, 1);
    let separator = Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1);
    let available = Rect::new(
        inner.x,
        inner.y.saturating_add(fixed_status_height),
        inner.width,
        inner.height.saturating_sub(fixed_status_height),
    );
    let (vertical_scrollbar, horizontal_scrollbar) = scrollbar_reservations(
        content_metrics.line_count,
        content_metrics.max_width,
        available,
    );
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
        limits(content_metrics.line_count, content_metrics.max_width, body);
    PlanReviewLayout {
        shell,
        body,
        status,
        separator,
        footer_status,
        vertical_scrollbar,
        horizontal_scrollbar,
        max_vertical,
        max_horizontal,
        matches: content.matches.clone(),
    }
}

fn common_footer_height(
    applyable: bool,
    match_count: usize,
    width: u16,
    copy_notice: Option<&str>,
    showing: Option<&str>,
) -> usize {
    [None, copy_notice, showing]
        .into_iter()
        .flat_map(|notice| {
            [
                footer_items(false, applyable, match_count, false),
                footer_items(true, applyable, match_count, true),
                footer_items(false, applyable, match_count.max(2), true),
                required_footer_items(false, match_count, false),
                required_footer_items(true, match_count, true),
                required_footer_items(false, match_count.max(2), true),
            ]
            .into_iter()
            .map(move |items| footer::layout_with_notice(items, width, notice).len())
        })
        .max()
        .unwrap_or(1)
}

pub(crate) fn render_apply_confirmation(
    frame: &mut Frame<'_>,
    state: &ApplyConfirmationState,
    view: &ApplyConfirmationViewState,
) {
    let area = frame.area();
    let layout = apply_confirmation_layout(area, state);
    if layout.header().height > 0 {
        header::render_review(frame, layout.header(), state.review());
    }
    if !layout.renderable() {
        terminal_notice::render_wrapped(frame, layout.notice(), CONFIRMATION_NOTICE);
        return;
    }

    let block_inner =
        shell_layout::render_content_block_line(frame, layout.frame(), Line::default());
    let inner = padded_confirmation_inner(block_inner);
    debug_assert_eq!(inner, layout.inner());
    let info_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    frame.render_widget(
        Paragraph::new(layout.lines().to_owned())
            .style(theme::body_style())
            .wrap(Wrap { trim: false }),
        info_area,
    );
    frame.render_widget(
        Paragraph::new(confirmation_input_line(view))
            .style(theme::body_style())
            .scroll((0, confirmation_input_scroll(view, layout.input().width))),
        layout.input(),
    );
    footer::render(frame, layout.footer(), layout.footer_lines(), None);
}

pub(crate) fn apply_confirmation_layout(
    area: Rect,
    state: &ApplyConfirmationState,
) -> ApplyConfirmationLayout {
    let panel = shell_layout::max_centered_area(area);
    let header_height = panel.height.min(CONFIRMATION_HEADER_HEIGHT);
    let header = Rect::new(panel.x, panel.y, panel.width, header_height);
    let available = Rect::new(
        panel.x,
        panel.y.saturating_add(header_height),
        panel.width,
        panel.height.saturating_sub(header_height),
    );
    let frame_width = panel.width.min(CONFIRMATION_MAX_WIDTH);
    let footer_items = vec![
        footer::hint(&["Enter"], "confirm"),
        footer::hint(&["Esc"], "back"),
    ];
    let footer_lines = footer::layout(footer_items.clone(), frame_width);
    let footer_required_width = footer_items.iter().map(Line::width).sum::<usize>() + 3;
    let footer_fits = footer_lines.len() == 1
        && footer_lines
            .first()
            .is_some_and(|line| line.width() == footer_required_width);
    let inner_width = frame_width.saturating_sub(4);
    let lines = confirmation_lines(state);
    let body = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
    let body_height = body.line_count(inner_width).saturating_add(1);
    let frame_height = u16::try_from(body_height)
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    let group_height = frame_height.saturating_add(1);
    let renderable =
        inner_width > 0 && footer_fits && group_height <= available.height && frame_width >= 5;
    let frame_x = panel.x + panel.width.saturating_sub(frame_width) / 2;
    let group_y = available.y + available.height.saturating_sub(group_height) / 2;
    let frame = Rect::new(frame_x, group_y, frame_width, frame_height);
    let footer = Rect::new(frame.x, frame.bottom(), frame.width, 1);
    let inner = padded_confirmation_inner(Block::new().borders(Borders::ALL).inner(frame));
    let input = Rect::new(
        inner.x,
        inner.y.saturating_add(inner.height.saturating_sub(1)),
        inner.width,
        u16::from(inner.height > 0),
    );
    ApplyConfirmationLayout {
        header,
        notice: available,
        frame,
        footer,
        inner,
        input,
        lines,
        footer_lines,
        renderable,
    }
}

const fn padded_confirmation_inner(inner: Rect) -> Rect {
    Rect::new(
        inner.x.saturating_add(1),
        inner.y.saturating_add(1),
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(2),
    )
}

fn confirmation_lines(state: &ApplyConfirmationState) -> Vec<Line<'static>> {
    let metadata = state.review().metadata();
    let mut lines = vec![
        Line::from("Apply this reviewed plan?"),
        Line::default(),
        Line::from(vec![
            Span::styled("Target: ", theme::secondary_style()),
            Span::styled(
                state.review().root().display().to_string(),
                theme::body_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Workspace: ", theme::secondary_style()),
            Span::styled(state.review().workspace().to_owned(), theme::body_style()),
        ]),
        Line::from(format!(
            "Plan: {} to add, {} to change, {} to destroy.",
            metadata.additions(),
            metadata.changes(),
            metadata.deletions()
        )),
    ];
    if !state.review().search_query().is_empty() {
        lines.push(Line::from(Span::styled(
            "Filter changes display only. Apply uses all changes.",
            theme::secondary_style(),
        )));
    }
    if metadata.deletions() > 0 {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "This plan includes resource deletion.",
            theme::warning_style(),
        )));
        lines.push(Line::default());
    }
    lines.push(Line::from("Apply this plan? Type yes or no."));
    lines
}

fn confirmation_input_line(view: &ApplyConfirmationViewState) -> Line<'static> {
    let cursor = view.cursor().min(view.input().len());
    Line::from(vec![
        Span::styled("> ", theme::body_style()),
        Span::styled(view.input()[..cursor].to_owned(), theme::body_style()),
        Span::styled("|", theme::accent_style()),
        Span::styled(view.input()[cursor..].to_owned(), theme::body_style()),
    ])
}

fn confirmation_input_scroll(view: &ApplyConfirmationViewState, width: u16) -> u16 {
    let width = usize::from(width);
    let cursor = view.cursor().min(view.input().len());
    let cursor_width = 2 + Line::from(view.input()[..cursor].to_owned()).width();
    u16::try_from(cursor_width.saturating_sub(width.saturating_sub(1))).unwrap_or(u16::MAX)
}

#[expect(
    clippy::too_many_lines,
    reason = "the plan renderer keeps the feature layout and content projection in one path"
)]
pub(crate) fn render_with_quit_confirmation(
    frame: &mut Frame<'_>,
    state: &ReviewSessionState,
    view: &PlanReviewViewState,
    now: Instant,
    quit_confirmation: bool,
) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        terminal_notice::render_wrapped(
            frame,
            area,
            terminal_notice_message(
                view.searching(),
                filter_active(view.searching(), state),
                quit_confirmation,
            ),
        );
        return;
    }

    let filtered_view = filter_active(view.searching(), state);
    let (content, base_metrics) = prepare_view_content(state, filtered_view);
    let layout = layout_with_content(
        area,
        view.searching(),
        state,
        &content,
        base_metrics,
        state.copy_notice_at(now),
        quit_confirmation,
    );
    if layout.body().width == 0 || layout.body().height == 0 {
        terminal_notice::render_wrapped(
            frame,
            area,
            terminal_notice_message(
                view.searching(),
                filter_active(view.searching(), state),
                quit_confirmation,
            ),
        );
        return;
    }
    header::render_review(frame, layout.shell.header(), state.review());
    let title = if filter_active(view.searching(), state) {
        Line::from("Plan | Filter")
    } else {
        Line::from("Plan")
    };
    let inner = shell_layout::render_content_block_line(frame, layout.shell.content(), title);
    debug_assert_eq!(inner, layout.shell.content_inner());

    render_status(frame, &layout, state, view);

    let content_metrics = content.metrics();
    let line_count = content_metrics.line_count;
    let max_line_width = content_metrics.max_width;
    let max_vertical = layout.max_vertical();
    let max_horizontal = layout.max_horizontal();
    let (vertical, horizontal) = view.scroll();
    let vertical = vertical.min(max_vertical);
    let horizontal = horizontal.min(max_horizontal);
    let lines = if state.copy_flash_active(now) {
        flash_lines(&content.lines)
    } else {
        content_lines_with_selection(
            &content,
            state.review().search_query(),
            view.selected()
                .and_then(|selected| content.matches.get(selected)),
        )
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .scroll((vertical, horizontal)),
        layout.body(),
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
            usize::from(vertical),
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
    footer::render(
        frame,
        layout.shell.footer(),
        layout.shell.footer_lines(),
        layout
            .footer_status
            .as_ref()
            .map(|(message, style)| (message.as_str(), *style)),
    );
}

fn prepare_content<'a>(
    state: &'a ReviewSessionState,
    filtered_view: bool,
    filter_query: &str,
) -> PreparedContent<'a> {
    let review = state.review();
    let filtered = review.document().filter(filter_query);
    let (lines, sources, matches) = review_lines(review, &filtered, filtered_view, filter_query);
    let metrics = ContentMetrics {
        line_count: lines.len(),
        max_width: max_line_width(&lines),
    };
    PreparedContent {
        lines,
        metrics,
        sources,
        matches,
    }
}

fn prepare_view_content(
    state: &ReviewSessionState,
    filtered_view: bool,
) -> (PreparedContent<'_>, ContentMetrics) {
    let content = prepare_content(state, filtered_view, state.review().search_query());
    let base_metrics = if filtered_view {
        prepare_content(state, false, "").metrics()
    } else {
        content.metrics()
    };
    (content, base_metrics)
}

fn render_status(
    frame: &mut Frame<'_>,
    layout: &PlanReviewLayout,
    state: &ReviewSessionState,
    view: &PlanReviewViewState,
) {
    let (line, horizontal) = if filter_active(view.searching(), state) {
        filter_status_line(view, state, layout.status().width)
    } else {
        (plan_status_line(state), 0)
    };
    frame.render_widget(
        Paragraph::new(line)
            .style(theme::body_style())
            .scroll((0, horizontal)),
        layout.status(),
    );
    frame.render_widget(
        separator::render(layout.separator().width),
        layout.separator(),
    );
}

fn review_lines<'a>(
    review: &'a PlanReview,
    filtered: &FilteredPlan<'a>,
    filtered_view: bool,
    filter_query: &str,
) -> (
    Vec<Line<'a>>,
    Vec<Option<PlanSource<'a>>>,
    Vec<PlanReviewMatch>,
) {
    let mut lines = diagnostic_lines(review);
    let mut sources = vec![None; lines.len()];
    let mut matches = Vec::new();
    let mut removed_summary = false;
    if filtered.matching_resources() == 0
        && filtered.matching_outputs() == 0
        && !filter_query.is_empty()
    {
        lines.push(Line::from(Span::styled(
            "No matching changes.",
            theme::warning_style(),
        )));
        sources.push(None);
        lines.push(Line::default());
        sources.push(None);
    }
    for (line_number, line) in filtered.lines_with_indices() {
        let kind = review.document().line_kind(line_number);
        if kind == PlanLineKind::Intro {
            continue;
        }
        if kind == PlanLineKind::Summary {
            removed_summary = true;
            continue;
        }
        if filtered_view && filtered.matching_outputs() == 0 && kind == PlanLineKind::OutputSection
        {
            continue;
        }
        let line_index = lines.len();
        let (rendered, line_matches) =
            plan_line_and_matches(line, filter_query, line_index, None, kind);
        lines.push(rendered);
        sources.push(Some(PlanSource { text: line, kind }));
        matches.extend(line_matches);
    }
    if removed_summary {
        while lines.last().is_some_and(|line| line.width() == 0) {
            lines.pop();
            sources.pop();
        }
    }
    (lines, sources, matches)
}

fn diagnostic_lines(review: &PlanReview) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    for diagnostic in review.diagnostics() {
        let style = match diagnostic.severity {
            DiagnosticSeverity::Error => theme::error_style(),
            _ => theme::warning_style(),
        };
        lines.push(Line::from(vec![
            Span::styled(severity_label(diagnostic.severity), style),
            Span::styled(": ", style),
            Span::styled(diagnostic.summary.as_str(), style),
        ]));
        if let Some(detail) = diagnostic.detail.as_deref() {
            lines.extend(detail.lines().map(Line::from));
        }
    }
    if !lines.is_empty() && !review.document().text().is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn plan_line_and_matches<'a>(
    line: &'a str,
    query: &str,
    line_index: usize,
    selected: Option<&PlanReviewMatch>,
    kind: PlanLineKind,
) -> (Line<'a>, Vec<PlanReviewMatch>) {
    if query.is_empty() {
        return (
            Line::from(Span::styled(line, plan_line_style(line, kind))),
            Vec::new(),
        );
    }
    let mut result = Line::default();
    let mut matches = Vec::new();
    let mut rest = line;
    let mut rendered_column = 0;
    while let Some(index) = rest.find(query) {
        let (before, matched_and_after) = rest.split_at(index);
        if !before.is_empty() {
            result.push_span(Span::styled(before, plan_line_style(line, kind)));
        }
        rendered_column += Line::from(before).width();
        let (match_text, after) = matched_and_after.split_at(query.len());
        let start_column = rendered_column;
        rendered_column += Line::from(match_text).width();
        let end_column = rendered_column;
        let rendered_match = PlanReviewMatch::new(
            line_index,
            u16::try_from(start_column).unwrap_or(u16::MAX),
            u16::try_from(end_column).unwrap_or(u16::MAX),
        );
        let style = selected
            .filter(|selected| {
                selected.start() == rendered_match.start() && selected.end() == rendered_match.end()
            })
            .map_or_else(theme::search_match_style, |_| {
                theme::selected_search_match_style()
            });
        result.push_span(Span::styled(match_text, style));
        matches.push(rendered_match);
        rest = after;
    }
    if !rest.is_empty() {
        result.push_span(Span::styled(rest, plan_line_style(line, kind)));
    }
    (result, matches)
}

fn plan_line_style(line: &str, kind: PlanLineKind) -> Style {
    if kind == PlanLineKind::Note {
        theme::plan_note_style()
    } else {
        theme::plan_line_style(line)
    }
}

fn flash_lines(lines: &[Line<'_>]) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|line| Line::from(Span::styled(line.to_string(), theme::copy_flash_style())))
        .collect()
}

fn content_lines_with_selection<'a>(
    content: &'a PreparedContent<'a>,
    query: &str,
    selected: Option<&PlanReviewMatch>,
) -> Vec<Line<'a>> {
    content
        .lines
        .iter()
        .zip(&content.sources)
        .enumerate()
        .map(|(line_index, (line, source))| {
            source.map_or_else(
                || line.clone(),
                |source| {
                    selected
                        .filter(|selected| selected.line() == line_index)
                        .map_or_else(
                            || line.clone(),
                            |selected| {
                                plan_line_and_matches(
                                    source.text,
                                    query,
                                    line_index,
                                    Some(selected),
                                    source.kind,
                                )
                                .0
                            },
                        )
                },
            )
        })
        .collect()
}

fn limits(line_count: usize, line_width: usize, body: Rect) -> (u16, u16) {
    let max_vertical =
        u16::try_from(line_count.saturating_sub(usize::from(body.height))).unwrap_or(u16::MAX);
    let max_horizontal =
        u16::try_from(line_width.saturating_sub(usize::from(body.width))).unwrap_or(u16::MAX);
    (max_vertical, max_horizontal)
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

fn filter_active(searching: bool, state: &ReviewSessionState) -> bool {
    searching || !state.review().search_query().is_empty()
}

const fn terminal_notice_message(
    searching: bool,
    filtered: bool,
    quit_confirmation: bool,
) -> &'static str {
    if quit_confirmation {
        "Quit? Enter exit / Esc cancel"
    } else if searching {
        "Terminal too small. Resize or press Esc to cancel filter."
    } else if filtered {
        "Terminal too small. Resize or press Esc to clear filter."
    } else {
        "Terminal too small. Resize or press q to quit."
    }
}

fn plan_status_line(state: &ReviewSessionState) -> Line<'static> {
    Line::from(Span::styled(
        format!(
            "Plan: {} to add, {} to change, {} to destroy.",
            state.review().metadata().additions(),
            state.review().metadata().changes(),
            state.review().metadata().deletions(),
        ),
        theme::body_style(),
    ))
}

fn filter_status_line(
    view: &PlanReviewViewState,
    state: &ReviewSessionState,
    width: u16,
) -> (Line<'static>, u16) {
    let searching = view.searching();
    let (query_line, cursor) = if searching {
        let Some((line, cursor)) = search_query_line(view) else {
            return (Line::default(), 0);
        };
        (line, Some(cursor))
    } else {
        (
            Line::from(vec![
                Span::styled("/", theme::secondary_style()),
                Span::styled(
                    state.review().search_query().to_owned(),
                    theme::secondary_style(),
                ),
            ]),
            None,
        )
    };
    let prefix = Span::styled("Filter: ", theme::secondary_style());
    let prefix_width = Line::from(prefix.clone()).width();
    let mut line = Line::from(prefix);
    line.extend(query_line.spans);
    let horizontal = cursor.map_or(0, |(start, end)| {
        horizontal_offset(
            prefix_width + start,
            prefix_width + end,
            line.width(),
            width,
        )
    });
    (line, horizontal)
}

fn filter_footer_status(query: &str, match_count: usize, width: u16) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    if width >= 72 {
        Some(match match_count {
            0 => "No matches".to_owned(),
            1 => "1 match".to_owned(),
            count => format!("{count} matches"),
        })
    } else {
        None
    }
}

fn search_query_line(view: &PlanReviewViewState) -> Option<(Line<'static>, (usize, usize))> {
    let query = view.search_query()?;
    let cursor = view.search_cursor()?;
    let before = query[..cursor].to_owned();
    let after = query[cursor..].to_owned();
    let (cursor_grapheme, after_cursor) = next_grapheme(&after);
    let line = Line::from(vec![
        Span::styled("/", theme::accent_style()),
        Span::styled(before.clone(), theme::body_style()),
        Span::styled(cursor_grapheme.clone(), theme::search_cursor_style()),
        Span::styled(after_cursor, theme::body_style()),
    ]);
    let cursor_start = 1 + Line::from(before).width();
    let cursor_end = cursor_start + Line::from(cursor_grapheme).width().max(1);
    Some((line, (cursor_start, cursor_end)))
}

fn next_grapheme(text: &str) -> (String, String) {
    let line = Line::from(text);
    let mut graphemes = line.styled_graphemes(Style::default());
    let Some(grapheme) = graphemes.next() else {
        return (" ".to_owned(), String::new());
    };
    let cursor = grapheme.symbol.len();
    (grapheme.symbol.to_owned(), text[cursor..].to_owned())
}

fn horizontal_offset(start: usize, end: usize, line_width: usize, width: u16) -> u16 {
    let width = usize::from(width);
    if width == 0 {
        return 0;
    }
    let offset = if end.saturating_sub(start) >= width {
        start
    } else {
        end.saturating_sub(width)
    };
    u16::try_from(offset.min(line_width.saturating_sub(width))).unwrap_or(u16::MAX)
}

fn footer_items(
    searching: bool,
    applyable: bool,
    match_count: usize,
    filtered: bool,
) -> Vec<Line<'static>> {
    if searching {
        vec![
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "cancel"),
        ]
    } else if filtered {
        let mut items = vec![footer::hint(&["Esc"], "clear")];
        items.extend([
            footer::hint(&["/"], "edit"),
            footer::hint(&["↑", "↓", "←", "→"], "scroll"),
        ]);
        if match_count >= 2 {
            items.push(footer::hint(&["n/N"], "next/prev"));
        }
        items
    } else {
        let mut items = vec![
            footer::hint(&["↑", "↓", "←", "→"], "scroll"),
            footer::hint(&["/"], "filter"),
            footer::hint(&["y"], "copy plan"),
        ];
        if applyable {
            items.push(footer::hint(&["a"], "apply"));
        }
        items.push(footer::hint(&["q"], "quit"));
        items
    }
}

fn required_footer_items(
    searching: bool,
    match_count: usize,
    filtered: bool,
) -> Vec<Line<'static>> {
    if searching {
        vec![
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "cancel"),
        ]
    } else if filtered {
        let mut items = vec![footer::hint(&["Esc"], "clear")];
        items.extend([
            footer::hint(&["/"], "edit"),
            footer::hint(&["↑", "↓", "←", "→"], "scroll"),
        ]);
        if match_count >= 2 {
            items.push(footer::hint(&["n/N"], "next/prev"));
        }
        items
    } else {
        vec![
            footer::hint(&["↑", "↓"], "scroll"),
            footer::hint(&["y"], "copy plan"),
            footer::hint(&["q"], "quit"),
        ]
    }
}

const fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "Error",
        DiagnosticSeverity::Warning => "Warning",
        DiagnosticSeverity::Info => "Info",
        DiagnosticSeverity::Unknown => "Diagnostic",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{
        buffer::Buffer,
        style::{Color, Modifier},
        widgets::Widget,
    };

    use crate::app::{
        copy::{CopyResult, CopyTarget},
        execution::{Diagnostic, DiagnosticSource, ExecutionContext, ExecutionState},
        review::{
            PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata, test_support::plan_document,
        },
        session::{self, Action, SessionState},
    };
    use crate::ui::{
        features::plan_review::{ApplyConfirmationInput, PlanReviewInput, key_to_input},
        test_support::{
            assert_shell_frame_and_footer, buffer_terminal_capture, buffer_text, render_to_buffer,
            write_buffer_captures,
        },
    };

    use super::*;

    const SIZES: [(u16, u16); 3] = [(80, 24), (120, 40), (160, 60)];
    const SEARCH_TERM: &str = "terraform_data";
    const PLAN_TEXT: &str = r#"Terraform will perform the following actions:

  # terraform_data.api will be updated in-place
  ~ resource "terraform_data" "api" {
      id       = "api-20260920"
      ~ input  = "before" -> "after"
      # (4 unchanged attributes hidden)
    }

  # terraform_data.worker must be replaced
-/+ resource "terraform_data" "worker" {
      ~ input = "worker-before" -> "worker-after" # forces replacement
      - old_checksum = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef"
      + new_checksum = (known after apply)
    }

  # terraform_data.old will be destroyed
  - resource "terraform_data" "old" {
      id = "old-20260920"
    }

  # terraform_data.new will be created
  + resource "terraform_data" "new" {
      input = "new-value"
      note  = "A deliberately long synthetic value keeps horizontal scrolling visible"
    }

Changes to Outputs:
  + endpoint = (known after apply)
  ~ summary  = "old summary" -> "new summary with a deliberately long value for review"

Warning: Value for "pending" is not known until apply

Plan: 2 to add, 2 to change, 1 to destroy.

Synthetic review text continues below so the viewport and scrollbar remain meaningful.
The same long body is intentionally reused across every review state and terminal size.
No Terraform process, provider, state file, or cloud credential is used by this fixture.
The review surface preserves Terraform order, attributes, output values, and diagnostics.
Long lines remain unwrapped in the plan body; horizontal movement exposes the hidden suffix.
Vertical movement exposes later lines in this synthetic plan body.

End of synthetic plan body."#;

    fn render(
        frame: &mut Frame<'_>,
        state: &ReviewSessionState,
        view: &PlanReviewViewState,
        now: Instant,
    ) {
        super::render_with_quit_confirmation(frame, state, view, now, false);
    }

    fn review() -> PlanReview {
        review_with_applyable(true)
    }

    fn review_with_applyable(applyable: bool) -> PlanReview {
        PlanReview::new(
            PathBuf::from("/repo/environments/production/main"),
            "default".to_owned(),
            PlanDocument::with_blocks_and_line_kinds(
                PLAN_TEXT.to_owned(),
                vec![
                    PlanBlock::new(0..2, PlanBlockKind::Common),
                    PlanBlock::new(2..8, PlanBlockKind::Resource),
                    PlanBlock::new(8..9, PlanBlockKind::Common),
                    PlanBlock::new(9..15, PlanBlockKind::Resource),
                    PlanBlock::new(15..16, PlanBlockKind::Common),
                    PlanBlock::new(16..20, PlanBlockKind::Resource),
                    PlanBlock::new(20..21, PlanBlockKind::Common),
                    PlanBlock::new(21..26, PlanBlockKind::Resource),
                    PlanBlock::new(26..28, PlanBlockKind::Common),
                    PlanBlock::new(28..29, PlanBlockKind::Output),
                    PlanBlock::new(29..30, PlanBlockKind::Output),
                    PlanBlock::new(30..43, PlanBlockKind::Common),
                ],
                vec![
                    PlanLineKind::Intro,
                    PlanLineKind::Intro,
                    PlanLineKind::Note,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Note,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Note,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Note,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Note,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::OutputSection,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                    PlanLineKind::Body,
                ],
            ),
            PlanMetadata::new(
                vec![
                    "terraform_data.api".to_owned(),
                    "terraform_data.worker".to_owned(),
                    "terraform_data.old".to_owned(),
                    "terraform_data.new".to_owned(),
                ],
                vec!["endpoint".to_owned(), "summary".to_owned()],
                2,
                2,
                1,
                applyable,
            ),
            Vec::new(),
        )
    }

    fn review_state(plan: PlanReview) -> ReviewSessionState {
        let now = Instant::now();
        let mut session = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/repo"),
        ));
        session::update(&mut session, Action::ReviewCompleted(plan), now);
        session
            .review()
            .expect("review should be available")
            .clone()
    }

    fn confirmation_state(plan: PlanReview) -> ApplyConfirmationState {
        let now = Instant::now();
        let mut session = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/repo"),
        ));
        session::update(&mut session, Action::ReviewCompleted(plan), now);
        session::update(&mut session, Action::OpenApplyConfirmation, now);
        session
            .apply_confirmation()
            .expect("confirmation should be available")
            .clone()
    }

    fn snapshot(name: &str, buffer: &Buffer) {
        insta::assert_snapshot!(name.to_string(), buffer_text(buffer));
        write_buffer_captures(name, buffer);
    }

    fn assert_text_prefix_uses_style(
        buffer: &Buffer,
        text: &str,
        styled_prefix: &str,
        foreground: Color,
        background: Color,
        modifier: Modifier,
    ) {
        assert_text_segment_uses_style(
            buffer,
            text,
            0,
            styled_prefix.chars().count(),
            foreground,
            background,
            modifier,
        );
    }

    fn assert_text_segment_uses_style(
        buffer: &Buffer,
        text: &str,
        segment_start: usize,
        segment_length: usize,
        foreground: Color,
        background: Color,
        modifier: Modifier,
    ) {
        let area = buffer.area();
        for y in area.y..area.bottom() {
            let symbols = (area.x..area.right())
                .map(|x| buffer.cell((x, y)).expect("search cell").symbol())
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
            for offset in segment_start..segment_start + segment_length {
                let cell = buffer
                    .cell((
                        area.x + u16::try_from(start + offset).expect("search offset"),
                        y,
                    ))
                    .expect("search cell");
                assert_eq!(cell.fg, foreground, "{text}");
                assert_eq!(cell.bg, background, "{text}");
                assert_eq!(cell.modifier, modifier, "{text}");
            }
            return;
        }
        panic!("text should be visible: {text}");
    }

    #[test]
    fn renders_plan_review_normal_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let state = review_state(review());
            let view = PlanReviewViewState::default();
            let buffer = render_to_buffer((width, height), |frame| {
                render(frame, &state, &view, Instant::now());
            });

            snapshot(&format!("preview_{width}x{height}_normal"), &buffer);
        }
    }

    #[test]
    fn renders_plan_review_quit_confirmation_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let state = review_state(review());
            let buffer = render_to_buffer((width, height), |frame| {
                render_with_quit_confirmation(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
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
    fn renders_normal_plan_height_variants_at_small_and_large_sizes() {
        struct HeightCase {
            name: &'static str,
            line_count: u16,
        }

        for height_case in [
            HeightCase {
                name: "short",
                line_count: 3,
            },
            HeightCase {
                name: "medium",
                line_count: 25,
            },
            HeightCase {
                name: "long",
                line_count: 60,
            },
        ] {
            for &(width, height) in &[(80, 24), (160, 60)] {
                let state = review_state(review_with_content(height_case.line_count, 48));
                let area = Rect::new(0, 0, width, height);
                let layout = layout(area, false, &state);
                let buffer = render_to_buffer((width, height), |frame| {
                    render(
                        frame,
                        &state,
                        &PlanReviewViewState::default(),
                        Instant::now(),
                    );
                });

                let panel_height = layout.shell.footer().bottom() - layout.shell.header().y;
                let top_margin = layout.shell.header().y;
                let bottom_margin = height.saturating_sub(layout.shell.footer().bottom());
                assert_eq!(
                    top_margin,
                    (height - panel_height) / 2,
                    "case: {} {width}x{height}",
                    height_case.name
                );
                assert!(
                    top_margin.abs_diff(bottom_margin) <= 1,
                    "case: {} {width}x{height}",
                    height_case.name
                );
                match (height_case.name, width) {
                    ("short", _) | ("medium", 160) => assert!(!layout.vertical_scrollbar()),
                    ("medium", 80) | ("long", _) => assert!(layout.vertical_scrollbar()),
                    _ => unreachable!(),
                }
                snapshot(
                    &format!("preview_{width}x{height}_normal-{}", height_case.name),
                    &buffer,
                );
            }
        }
    }

    #[test]
    fn renders_plan_review_search_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let mut plan = review();
            plan.set_search_query(SEARCH_TERM.to_owned());
            let state = review_state(plan);
            let mut view = PlanReviewViewState::default();
            view.apply_with_matches(
                PlanReviewInput::SearchStart,
                Rect::new(0, 0, width, height),
                0,
                0,
                SEARCH_TERM,
                &[],
            );
            let buffer = render_to_buffer((width, height), |frame| {
                render(frame, &state, &view, Instant::now());
            });

            snapshot(&format!("preview_{width}x{height}_search"), &buffer);
        }
    }

    #[test]
    fn renders_apply_confirmation_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let state = confirmation_state(review());
            let view = ApplyConfirmationViewState::default();
            let buffer = render_to_buffer((width, height), |frame| {
                render_apply_confirmation(frame, &state, &view);
            });

            snapshot(
                &format!("preview_{width}x{height}_apply-confirmation"),
                &buffer,
            );
        }
    }

    fn review_with_content(line_count: u16, line_width: u16) -> PlanReview {
        let line = "x".repeat(usize::from(line_width));
        let text = (0..line_count)
            .map(|_| line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        PlanReview::new(
            PathBuf::from("/repo"),
            "default".to_owned(),
            PlanDocument::with_blocks(
                text,
                vec![PlanBlock::new(
                    0..usize::from(line_count),
                    PlanBlockKind::Common,
                )],
            ),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, true),
            Vec::new(),
        )
    }

    mod layout {
        use super::*;

        fn assert_text_color(buffer: &Buffer, text: &str, color: Color) {
            let area = buffer.area();
            for y in area.y..area.bottom() {
                let symbols = (area.x..area.right())
                    .map(|x| buffer.cell((x, y)).expect("plan cell").symbol())
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
                            area.x + u16::try_from(start + offset).expect("plan offset"),
                            y,
                        ))
                        .expect("plan cell");
                    assert_eq!(cell.fg, color, "{text}");
                }
                return;
            }
            panic!("text should be visible: {text}");
        }

        fn filter_height_review() -> PlanReview {
            PlanReview::new(
                PathBuf::from("/repo"),
                "default".to_owned(),
                PlanDocument::with_blocks(
                    "api line 1\napi line 2\nworker line 1\nworker line 2\ncommon line\n"
                        .to_owned(),
                    vec![
                        PlanBlock::new(0..2, PlanBlockKind::Resource),
                        PlanBlock::new(2..4, PlanBlockKind::Resource),
                        PlanBlock::new(4..5, PlanBlockKind::Common),
                    ],
                ),
                PlanMetadata::new(
                    vec!["api".to_owned(), "worker".to_owned()],
                    Vec::new(),
                    0,
                    2,
                    0,
                    true,
                ),
                Vec::new(),
            )
        }

        fn common_only_review() -> PlanReview {
            PlanReview::new(
                PathBuf::from("/repo"),
                "default".to_owned(),
                PlanDocument::with_blocks(
                    "common line 1\ncommon line 2\n".to_owned(),
                    vec![PlanBlock::new(0..2, PlanBlockKind::Common)],
                ),
                PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, true),
                Vec::new(),
            )
        }

        fn review_buffer_at(
            area: Rect,
            state: &ReviewSessionState,
            vertical: u16,
            horizontal: u16,
        ) -> (PlanReviewLayout, Buffer) {
            let layout = layout(area, false, state);
            let mut view = PlanReviewViewState::default();
            for _ in 0..vertical {
                view.apply_with_matches(
                    PlanReviewInput::Down,
                    layout.body(),
                    layout.max_vertical(),
                    layout.max_horizontal(),
                    "",
                    &[],
                );
            }
            for _ in 0..horizontal {
                view.apply_with_matches(
                    PlanReviewInput::Right,
                    layout.body(),
                    layout.max_vertical(),
                    layout.max_horizontal(),
                    "",
                    &[],
                );
            }
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render(frame, state, &view, Instant::now());
            });
            (layout, buffer)
        }

        fn assert_scrollbar_positions(
            buffer: &Buffer,
            layout: &PlanReviewLayout,
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

        #[test]
        fn quit_confirmation_preserves_the_plan_body_and_scroll_limits() {
            let state = review_state(review());
            let area = Rect::new(0, 0, 50, 24);
            let normal = layout(area, false, &state);
            let waiting = layout_with_quit_confirmation(area, false, &state, true);
            let content = prepare_content(&state, false, "");
            let footer = footer::layout_with_notice(
                footer_items(
                    false,
                    state.review().metadata().applyable(),
                    content.matches.len(),
                    false,
                ),
                shell_layout::centered_width(area),
                None,
            );

            assert!(footer.len() >= 2);
            assert_eq!(waiting.body(), normal.body());
            assert_eq!(waiting.max_vertical(), normal.max_vertical());
            assert_eq!(waiting.max_horizontal(), normal.max_horizontal());
        }

        #[test]
        fn filter_height_uses_the_unfiltered_plan_as_its_baseline() {
            let mut plan = filter_height_review();
            let state = review_state(plan.clone());
            let area = Rect::new(0, 0, 80, 24);
            let normal = layout(area, false, &state);

            plan.set_search_query("api".to_owned());
            let first_filter_state = review_state(plan.clone());
            plan.set_search_query("missing".to_owned());
            let second_filter_state = review_state(plan);
            let first_filter = layout(area, false, &first_filter_state);
            let second_filter = layout(area, false, &second_filter_state);
            let searching = layout(area, true, &state);

            assert_eq!(
                first_filter.shell.header().y,
                second_filter.shell.header().y
            );
            assert_eq!(
                first_filter.shell.footer().bottom(),
                second_filter.shell.footer().bottom()
            );
            assert_eq!(first_filter.shell.content(), normal.shell.content());
            assert_eq!(first_filter.body(), normal.body());
            assert_eq!(first_filter.status(), normal.status());
            assert_eq!(first_filter.separator(), normal.separator());
            assert_eq!(
                first_filter.shell.footer().bottom(),
                normal.shell.footer().bottom()
            );
            assert_eq!(searching.shell.content(), normal.shell.content());
            assert_eq!(searching.body(), normal.body());
            assert_eq!(searching.status(), normal.status());
            assert_eq!(searching.separator(), normal.separator());
        }

        #[test]
        fn filter_input_keeps_a_common_only_plan_height_stable() {
            let mut plan = common_only_review();
            let area = Rect::new(0, 0, 80, 24);
            let empty_filter = layout(area, true, &review_state(plan.clone()));

            plan.set_search_query("missing".to_owned());
            let typed_filter = layout(area, true, &review_state(plan));

            assert_eq!(typed_filter.shell.header().y, empty_filter.shell.header().y);
            assert_eq!(
                typed_filter.shell.footer().bottom(),
                empty_filter.shell.footer().bottom()
            );
        }

        #[test]
        fn production_review_render_draws_shell_scrollbars_and_plan_colors() {
            let state = review_state(review());
            let view = PlanReviewViewState::default();
            let area = Rect::new(0, 0, 80, 24);
            let layout = layout(area, false, &state);
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });

            assert_shell_frame_and_footer(
                &buffer,
                layout.shell.content(),
                layout.shell.footer(),
                "q quit",
            );
            let text = buffer_text(&buffer);
            assert!(!text.contains("Terraform will perform the following actions:"));
            assert!(layout.vertical_scrollbar());
            assert!(layout.horizontal_scrollbar());
            let body = layout.body();
            let vertical_x = body.x.saturating_add(body.width);
            let horizontal_y = body.y.saturating_add(body.height);
            let horizontal_end_x = vertical_x;
            assert_eq!(buffer[(vertical_x, body.y)].symbol(), "▲");
            assert_eq!(
                buffer[(vertical_x, body.y)].fg,
                Color::Rgb(0x50, 0x52, 0x5e)
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
            assert_text_color(
                &buffer,
                "~ resource \"terraform_data\" \"api\"",
                Color::Rgb(0xeb, 0xcb, 0x8b),
            );
            assert_text_color(
                &buffer,
                "# terraform_data.api will be updated in-place",
                Color::Rgb(0xc0, 0xb8, 0xb8),
            );
            assert_text_color(&buffer, "- old_checksum", Color::Rgb(0xbf, 0x61, 0x6a));
            assert_text_color(&buffer, "+ new_checksum", Color::Rgb(0xa3, 0xbe, 0x8c));
        }

        #[test]
        fn production_review_scrollbars_reach_offsets_after_resize_and_single_overflow() {
            let state = review_state(review());
            let mut previous_body = None;
            for area in [Rect::new(0, 0, 80, 24), Rect::new(0, 0, 88, 24)] {
                let layout = layout(area, false, &state);
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
                    let (layout, buffer) = review_buffer_at(area, &state, vertical, horizontal);
                    assert_scrollbar_positions(&buffer, &layout, vertical, horizontal);
                }
            }

            let area = Rect::new(0, 0, 80, 24);
            let base_layout = layout(area, false, &state);
            let vertical_state = review_state(review_with_content(
                base_layout.body().height.saturating_add(2),
                base_layout.body().width,
            ));
            let (vertical_layout, vertical_buffer) = review_buffer_at(area, &vertical_state, 1, 0);
            assert_eq!(vertical_layout.max_vertical(), 1);
            assert!(!vertical_layout.horizontal_scrollbar());
            assert_scrollbar_positions(&vertical_buffer, &vertical_layout, 1, 0);

            let horizontal_state = review_state(review_with_content(
                base_layout.body().height.saturating_sub(1),
                base_layout.body().width.saturating_add(2),
            ));
            let (horizontal_layout, horizontal_buffer) =
                review_buffer_at(area, &horizontal_state, 0, 1);
            assert_eq!(horizontal_layout.max_horizontal(), 1);
            assert!(!horizontal_layout.vertical_scrollbar());
            assert_scrollbar_positions(&horizontal_buffer, &horizontal_layout, 0, 1);
        }

        #[test]
        fn normal_body_omits_only_the_final_summary_and_its_trailing_blank() {
            let review = PlanReview::new(
                PathBuf::from("/project"),
                "default".to_owned(),
                PlanDocument::with_blocks_and_line_kinds(
                    "body\nPlan: 1 to add, 0 to change, 0 to destroy.\n".to_owned(),
                    vec![PlanBlock::new(0..3, PlanBlockKind::Common)],
                    vec![
                        PlanLineKind::Body,
                        PlanLineKind::Summary,
                        PlanLineKind::Body,
                    ],
                ),
                PlanMetadata::new(Vec::new(), Vec::new(), 1, 0, 0, true),
                Vec::new(),
            );
            let filtered = review.document().filter(review.search_query());
            let lines = review_lines(&review, &filtered, false, "").0;

            assert_eq!(
                lines.iter().map(Line::to_string).collect::<Vec<_>>(),
                ["body"]
            );
        }

        #[test]
        fn normal_body_keeps_unknown_plan_text_and_following_lines() {
            let review = PlanReview::new(
                PathBuf::from("/project"),
                "default".to_owned(),
                plan_document("Plan: application text\nfollowing body text\n".to_owned()),
                PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
                Vec::new(),
            );
            let filtered = review.document().filter(review.search_query());
            let lines = review_lines(&review, &filtered, false, "").0;

            assert_eq!(
                lines.iter().map(Line::to_string).collect::<Vec<_>>(),
                ["Plan: application text", "following body text", ""]
            );
        }
    }

    mod filter {
        use super::*;

        fn zero_match_review() -> PlanReview {
            PlanReview::new(
                PathBuf::from("/repo/environments/production/main"),
                "default".to_owned(),
                PlanDocument::with_blocks_and_line_kinds(
                    "Warning: synthetic diagnostic\nCommon context stays visible\n  # terraform_data.api will be created\n  + resource \"terraform_data\" \"api\" {\n  + endpoint = (known after apply)\nPlan: 1 to add, 0 to change, 0 to destroy.\n"
                        .to_owned(),
                    vec![
                        PlanBlock::new(0..2, PlanBlockKind::Common),
                        PlanBlock::new(2..4, PlanBlockKind::Resource),
                        PlanBlock::new(4..5, PlanBlockKind::Output),
                        PlanBlock::new(5..7, PlanBlockKind::Common),
                    ],
                    vec![
                        PlanLineKind::Body,
                        PlanLineKind::Body,
                        PlanLineKind::Note,
                        PlanLineKind::Body,
                        PlanLineKind::Body,
                        PlanLineKind::Summary,
                        PlanLineKind::Body,
                    ],
                ),
                PlanMetadata::new(
                    vec!["terraform_data.api".to_owned()],
                    vec!["endpoint".to_owned()],
                    1,
                    0,
                    0,
                    true,
                ),
                vec![Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    summary: "Synthetic diagnostic".to_owned(),
                    detail: None,
                    position: None,
                    source: DiagnosticSource::Terraform,
                }],
            )
        }

        fn search_match_style_counts(buffer: &Buffer, query: &str) -> (usize, usize) {
            let mut normal = 0;
            let mut selected = 0;
            let query_width = query.chars().count();
            let area = buffer.area();
            for y in area.y..area.bottom() {
                let symbols = (area.x..area.right())
                    .map(|x| buffer.cell((x, y)).expect("match cell").symbol())
                    .collect::<Vec<_>>();
                for start in 0..symbols.len().saturating_sub(query_width.saturating_sub(1)) {
                    if !symbols[start..]
                        .iter()
                        .copied()
                        .collect::<String>()
                        .starts_with(query)
                    {
                        continue;
                    }
                    let cell = buffer
                        .cell((area.x + u16::try_from(start).expect("match offset"), y))
                        .expect("match cell");
                    if cell.bg == Color::Rgb(0xf4, 0x9e, 0x4c) {
                        normal += 1;
                    }
                    if cell.bg == Color::Rgb(0xff, 0xd0, 0x8a) {
                        assert_eq!(cell.modifier, Modifier::BOLD | Modifier::UNDERLINED);
                        selected += 1;
                    }
                }
            }
            (normal, selected)
        }

        fn search_prompt(view: &PlanReviewViewState, width: u16) -> Option<(Line<'static>, u16)> {
            let (line, (cursor_start, cursor_end)) = search_query_line(view)?;
            Some((
                line.clone(),
                horizontal_offset(cursor_start, cursor_end, line.width(), width),
            ))
        }

        #[test]
        fn production_search_render_draws_search_input_and_match_color() {
            let mut plan = review();
            plan.set_search_query(SEARCH_TERM.to_owned());
            let state = review_state(plan);
            let mut view = PlanReviewViewState::default();
            view.apply_with_matches(
                PlanReviewInput::SearchStart,
                Rect::new(0, 0, 80, 24),
                0,
                0,
                SEARCH_TERM,
                &[],
            );
            let buffer = render_to_buffer((80, 24), |frame| {
                render(frame, &state, &view, Instant::now());
            });

            assert!(buffer_text(&buffer).contains("/terraform_data "));
            assert_text_segment_uses_style(
                &buffer,
                "/terraform_data ",
                0,
                1,
                Color::Rgb(0xf4, 0x9e, 0x4c),
                Color::Reset,
                Modifier::empty(),
            );
            assert_text_segment_uses_style(
                &buffer,
                "/terraform_data ",
                1,
                SEARCH_TERM.chars().count(),
                Color::Rgb(0xe9, 0xdb, 0xdb),
                Color::Reset,
                Modifier::empty(),
            );
            assert_text_segment_uses_style(
                &buffer,
                "/terraform_data ",
                1 + SEARCH_TERM.chars().count(),
                1,
                Color::Rgb(0x11, 0x14, 0x19),
                Color::Rgb(0xf4, 0x9e, 0x4c),
                Modifier::empty(),
            );
            assert_text_prefix_uses_style(
                &buffer,
                "terraform_data.api",
                SEARCH_TERM,
                Color::Rgb(0x11, 0x14, 0x19),
                Color::Rgb(0xf4, 0x9e, 0x4c),
                Modifier::BOLD,
            );
            let confirmed_buffer = render_to_buffer((80, 24), |frame| {
                render(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                );
            });
            assert_text_segment_uses_style(
                &confirmed_buffer,
                "/terraform_data",
                0,
                "/terraform_data".chars().count(),
                Color::Rgb(0xc0, 0xb8, 0xb8),
                Color::Reset,
                Modifier::empty(),
            );
            assert_text_prefix_uses_style(
                &confirmed_buffer,
                "8 matches",
                "8 matches",
                Color::Rgb(0xc0, 0xb8, 0xb8),
                Color::Reset,
                Modifier::empty(),
            );
            let capture = buffer_terminal_capture(&buffer);
            assert!(capture.contains("\x1b[48;2;244;158;76m"));
            assert!(capture.contains("\x1b[48;2;244;158;76m\x1b[1m"));
        }

        #[test]
        fn production_search_cursor_styles_full_width_and_zwj_graphemes_without_inserting_a_bar() {
            let state = review_state(review());
            let mut view = PlanReviewViewState::default();
            let body = Rect::new(0, 0, 120, 40);
            view.apply_with_matches(PlanReviewInput::SearchStart, body, 0, 0, "", &[]);
            for character in "全e\u{301}👩\u{200d}💻".chars() {
                view.apply_with_matches(
                    PlanReviewInput::SearchChar(character),
                    body,
                    0,
                    0,
                    "",
                    &[],
                );
            }
            view.apply_with_matches(PlanReviewInput::SearchLeft, body, 0, 0, "", &[]);

            let buffer = render_to_buffer((120, 40), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            write_buffer_captures("ux06-filter-grapheme-cursor", &buffer);
            let text = buffer_text(&buffer);
            assert!(text.contains("/全"));
            assert!(text.contains("e\u{301}"));
            assert!(text.contains("👩\u{200d}💻"));
            let search_row = (buffer.area().y..buffer.area().bottom())
                .map(|y| {
                    (buffer.area().x..buffer.area().right())
                        .map(|x| buffer.cell((x, y)).expect("search row cell").symbol())
                        .collect::<String>()
                })
                .find(|row| row.contains("/全"))
                .expect("search row should be visible");
            assert!(!search_row.contains("matches"));
            assert!(!text.contains("No matches"));

            let mut found = false;
            for y in buffer.area().y..buffer.area().bottom() {
                for x in buffer.area().x..buffer.area().right() {
                    let cell = buffer.cell((x, y)).expect("grapheme cursor cell");
                    if cell.symbol() == "👩\u{200d}💻" {
                        assert_eq!(cell.fg, Color::Rgb(0x11, 0x14, 0x19));
                        assert_eq!(cell.bg, Color::Rgb(0xf4, 0x9e, 0x4c));
                        found = true;
                    }
                }
            }
            assert!(found, "ZWJ grapheme should be rendered as the cursor");

            view.apply_with_matches(PlanReviewInput::SearchEnd, body, 0, 0, "", &[]);
            let end_buffer = render_to_buffer((120, 40), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            assert!(
                (end_buffer.area().y..end_buffer.area().bottom()).any(|y| {
                    (end_buffer.area().x..end_buffer.area().right()).any(|x| {
                        let cell = end_buffer.cell((x, y)).expect("end cursor cell");
                        cell.symbol() == " "
                            && cell.fg == Color::Rgb(0x11, 0x14, 0x19)
                            && cell.bg == Color::Rgb(0xf4, 0x9e, 0x4c)
                    })
                }),
                "end cursor should style a blank cell",
            );
        }

        #[test]
        fn production_partial_zwj_match_tracks_the_rendered_span_columns() {
            let line = format!("{}👩\u{200d}💻", "a".repeat(20));
            let query = "💻";
            let (_, matches) = plan_line_and_matches(&line, query, 0, None, PlanLineKind::Body);
            assert_eq!(matches, [PlanReviewMatch::new(0, 22, 24)]);

            let (selected_line, _) =
                plan_line_and_matches(&line, query, 0, matches.first(), PlanLineKind::Body);
            let mut buffer = Buffer::empty(Rect::new(0, 0, 30, 1));
            Paragraph::new(vec![selected_line]).render(*buffer.area(), &mut buffer);
            let cell = buffer
                .cell((22, 0))
                .expect("selected partial grapheme cell");
            assert_eq!(cell.fg, Color::Rgb(0x11, 0x14, 0x19));
            assert_eq!(cell.bg, Color::Rgb(0xff, 0xd0, 0x8a));
            assert_eq!(cell.modifier, Modifier::BOLD | Modifier::UNDERLINED);
        }

        #[test]
        fn production_filter_selects_one_match_and_moves_with_footer_priority() {
            let mut plan = review();
            plan.set_search_query(SEARCH_TERM.to_owned());
            let state = review_state(plan);
            let area = Rect::new(0, 0, 80, 24);
            let layout = layout(area, false, &state);
            assert!(layout.matches().len() >= 2);
            let mut view = PlanReviewViewState::default();
            view.apply_with_matches(
                PlanReviewInput::SearchStart,
                layout.body(),
                layout.max_vertical(),
                layout.max_horizontal(),
                SEARCH_TERM,
                layout.matches(),
            );
            view.apply_with_matches(
                PlanReviewInput::SearchConfirm,
                layout.body(),
                layout.max_vertical(),
                layout.max_horizontal(),
                SEARCH_TERM,
                layout.matches(),
            );
            assert_eq!(view.selected(), Some(0));
            let first = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let (normal, selected) = search_match_style_counts(&first, SEARCH_TERM);
            assert_eq!(selected, 1, "normal={normal}");
            assert_eq!(normal + selected, 4);
            let footer = buffer_text(&first);
            assert!(footer.contains("Esc clear"));
            assert!(footer.contains("n/N next/prev"));
            assert!(footer.contains("↑/↓/←/→ scroll"));
            assert!(footer.contains("/ edit"));
            assert!(!footer.contains("a apply"));
            assert!(!footer.contains("y yank"));
            assert!(!footer.contains("q quit"));

            view.apply_with_matches(
                PlanReviewInput::SearchNext,
                layout.body(),
                layout.max_vertical(),
                layout.max_horizontal(),
                SEARCH_TERM,
                layout.matches(),
            );
            assert_eq!(view.selected(), Some(1));
            let second = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            assert_eq!(search_match_style_counts(&second, SEARCH_TERM).1, 1);

            assert_eq!(
                view.apply_with_matches(
                    PlanReviewInput::SearchCancel,
                    layout.body(),
                    layout.max_vertical(),
                    layout.max_horizontal(),
                    SEARCH_TERM,
                    layout.matches(),
                ),
                Some(String::new())
            );
            assert_eq!(view.selected(), None);
            assert_eq!(view.scroll(), (0, 0));
        }

        #[test]
        fn confirmed_filter_footer_only_offers_match_navigation_when_needed() {
            struct Case {
                name: &'static str,
                query: &'static str,
                expected_matches: usize,
            }

            for case in [
                Case {
                    name: "zero_matches",
                    query: "not-present",
                    expected_matches: 0,
                },
                Case {
                    name: "one_match",
                    query: "endpoint",
                    expected_matches: 1,
                },
                Case {
                    name: "multiple_matches",
                    query: SEARCH_TERM,
                    expected_matches: 8,
                },
            ] {
                let mut plan = if case.expected_matches == 0 {
                    zero_match_review()
                } else {
                    review()
                };
                plan.set_search_query(case.query.to_owned());
                let state = review_state(plan);
                let layout = layout(Rect::new(0, 0, 120, 40), false, &state);
                assert_eq!(
                    layout.matches().len(),
                    case.expected_matches,
                    "case: {}",
                    case.name
                );
                let expected_label = match case.expected_matches {
                    0 => "No matches".to_owned(),
                    1 => "1 match".to_owned(),
                    count => format!("{count} matches"),
                };
                assert_eq!(
                    layout
                        .footer_status
                        .as_ref()
                        .map(|status| status.0.as_str()),
                    Some(expected_label.as_str()),
                    "case: {}",
                    case.name
                );
                let buffer = render_to_buffer((120, 40), |frame| {
                    render(
                        frame,
                        &state,
                        &PlanReviewViewState::default(),
                        Instant::now(),
                    );
                });
                let text = buffer_text(&buffer);
                assert!(text.contains("Esc clear"), "case: {}", case.name);
                assert!(text.contains("↑/↓/←/→ scroll"), "case: {}", case.name);
                assert!(text.contains("/ edit"), "case: {}", case.name);
                assert_eq!(
                    text.contains("n/N next/prev"),
                    case.expected_matches >= 2,
                    "case: {}",
                    case.name
                );
                assert!(!text.contains("y yank"), "case: {}", case.name);
                assert!(!text.contains("a apply"), "case: {}", case.name);
                assert!(!text.contains("q quit"), "case: {}", case.name);
            }
        }

        #[test]
        fn confirmed_filter_narrow_footer_keeps_required_actions_before_match_navigation() {
            for (query, plan) in [
                ("not-present", zero_match_review()),
                ("endpoint", review()),
                (SEARCH_TERM, review()),
            ] {
                let mut plan = plan;
                plan.set_search_query(query.to_owned());
                let state = review_state(plan);
                let buffer = render_to_buffer((24, 24), |frame| {
                    render(
                        frame,
                        &state,
                        &PlanReviewViewState::default(),
                        Instant::now(),
                    );
                });
                let text = buffer_text(&buffer);
                assert!(text.contains("Esc clear"), "query: {query}");
                assert!(text.contains("↑/↓/←/→ scroll"), "query: {query}");
                assert!(text.contains("/ edit"), "query: {query}");
                assert!(!text.contains("n/N next/prev"), "query: {query}");
            }
        }

        #[test]
        fn production_filter_states_show_search_hits_in_the_footer() {
            let mut input_view = PlanReviewViewState::default();
            input_view.apply_with_matches(
                PlanReviewInput::SearchStart,
                Rect::new(0, 0, 120, 40),
                0,
                0,
                "",
                &[],
            );
            let input_buffer = render_to_buffer((120, 40), |frame| {
                render(frame, &review_state(review()), &input_view, Instant::now());
            });
            let input_text = buffer_text(&input_buffer);
            write_buffer_captures("ux02-filter-input", &input_buffer);
            assert!(input_text.contains("Plan | Filter"));
            assert!(input_text.contains("Filter: /"));
            assert!(!input_text.contains(" matches"));
            assert!(!input_text.contains("Filter changes display only"));
            assert!(!input_text.contains("Matching changes"));

            let mut confirmed = review();
            confirmed.set_search_query("worker".to_owned());
            let confirmed_state = review_state(confirmed);
            let confirmed_buffer = render_to_buffer((120, 40), |frame| {
                render(
                    frame,
                    &confirmed_state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                );
            });
            let confirmed_text = buffer_text(&confirmed_buffer);
            write_buffer_captures("ux02-filter-confirmed", &confirmed_buffer);
            assert!(confirmed_text.contains("Plan | Filter"));
            assert!(confirmed_text.contains("Filter: /worker"));
            assert!(confirmed_text.contains("4 matches"));
            assert!(!confirmed_text.contains("Filter changes display only"));
            assert!(!confirmed_text.contains("Matching changes"));
            assert!(!confirmed_text.contains("terraform_data.api will be updated"));

            let cleared_buffer = render_to_buffer((120, 40), |frame| {
                render(
                    frame,
                    &review_state(review()),
                    &PlanReviewViewState::default(),
                    Instant::now(),
                );
            });
            let cleared_text = buffer_text(&cleared_buffer);
            assert!(cleared_text.contains("┌Plan"));
            assert!(!cleared_text.contains("Plan | Filter"));
            assert!(!cleared_text.contains(" matches"));
            assert!(!cleared_text.contains("Scope: full plan"));
        }

        #[test]
        fn filter_input_keeps_the_footer_count_and_plan_body_fixed_while_typing() {
            let area = Rect::new(0, 0, 120, 40);
            let normal_state = review_state(review());
            let normal_layout = layout(area, false, &normal_state);
            let mut positions = Vec::new();

            for query in ["a", "worker", "a-very-long-filter-query"] {
                let mut plan = review();
                plan.set_search_query(query.to_owned());
                let state = review_state(plan);
                let mut view = PlanReviewViewState::default();
                view.apply_with_matches(PlanReviewInput::SearchStart, area, 0, 0, query, &[]);
                let layout = layout(area, true, &state);
                let buffer = render_to_buffer((area.width, area.height), |frame| {
                    render(frame, &state, &view, Instant::now());
                });
                let label = layout
                    .footer_status
                    .as_ref()
                    .expect("filter count should be visible")
                    .0
                    .as_str();
                let footer = layout.shell.footer();
                let x = footer.right() - u16::try_from(label.len()).expect("footer count width");
                let y = footer.y + u16::try_from(layout.shell.footer_lines().len() - 1).unwrap();
                for (offset, character) in label.chars().enumerate() {
                    assert_eq!(
                        buffer
                            .cell((x + u16::try_from(offset).unwrap(), y))
                            .expect("footer count cell")
                            .symbol(),
                        character.to_string(),
                        "query: {query}"
                    );
                }
                assert_eq!(layout.body().y, normal_layout.body().y, "query: {query}");
                positions.push((x + u16::try_from(label.len()).unwrap(), y));
            }

            assert!(positions.windows(2).all(|pair| pair[0] == pair[1]));
        }

        #[test]
        fn filter_footer_count_compacts_or_disappears_when_controls_need_room() {
            let mut plan = review();
            plan.set_search_query("worker".to_owned());
            let state = review_state(plan);

            for (width, expected) in [(24, None), (48, None), (80, Some("4 matches"))] {
                let buffer = render_to_buffer((width, 24), |frame| {
                    render(
                        frame,
                        &state,
                        &PlanReviewViewState::default(),
                        Instant::now(),
                    );
                });
                let text = buffer_text(&buffer);
                if let Some(expected) = expected {
                    assert!(text.contains(expected), "width: {width}");
                } else {
                    assert!(!text.contains(" matches"), "width: {width}");
                    assert!(!text.contains(" hits"), "width: {width}");
                }
                assert!(text.contains("Esc clear"), "width: {width}");
            }
        }

        #[test]
        fn filter_toggle_keeps_the_body_origin_at_small_and_large_terminal_sizes() {
            let normal = review_state(review());
            let mut plan = review();
            plan.set_search_query("worker".to_owned());
            let filtered = review_state(plan);

            for (width, height) in [(48, 24), (80, 24), (120, 40), (160, 60)] {
                let area = Rect::new(0, 0, width, height);
                let normal_layout = layout(area, false, &normal);
                let filtered_layout = layout(area, false, &filtered);
                assert_eq!(
                    normal_layout.body().y,
                    filtered_layout.body().y,
                    "terminal: {width}x{height}"
                );
                assert_eq!(
                    normal_layout.shell.footer().y,
                    filtered_layout.shell.footer().y,
                    "terminal: {width}x{height}"
                );
            }
        }

        #[test]
        fn production_filter_keeps_common_text_matches_when_no_changes_match() {
            let mut plan = zero_match_review();
            plan.set_search_query("Common".to_owned());
            let state = review_state(plan);
            let buffer = render_to_buffer((120, 40), |frame| {
                render(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                );
            });
            let text = buffer_text(&buffer);
            write_buffer_captures("ux02-filter-zero-match", &buffer);

            assert!(text.contains("No matching changes."));
            assert!(text.contains("1 match"));
            assert!(text.contains("Warning: Synthetic diagnostic"));
            assert!(text.contains("Common context stays visible"));
            assert!(!text.contains("Plan total (full plan):"));
            assert!(!text.contains("terraform_data.api will be created"));
            assert!(!text.contains("endpoint = (known after apply)"));
        }

        #[test]
        fn production_confirmed_filter_shows_the_query_prefix_without_expanding_the_title() {
            let mut plan = review();
            plan.set_search_query("long-query-".repeat(20));
            let state = review_state(plan);
            let buffer = render_to_buffer((80, 24), |frame| {
                render(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                );
            });
            let text = buffer_text(&buffer);
            write_buffer_captures("ux02-filter-long-query", &buffer);

            assert!(text.contains("┌Plan | Filter"));
            assert!(text.contains("/long-query-long-query-"));
            assert!(!text.contains("Plan | Filter: long-query"));
        }

        #[test]
        fn production_filter_layout_uses_a_single_status_row_and_separator() {
            let mut plan = review();
            plan.set_search_query("worker".to_owned());
            let state = review_state(plan);
            let area = Rect::new(0, 0, 80, 20);
            let layout = layout(area, false, &state);
            let status = layout.status();
            let separator = layout.separator();
            assert_eq!(status.height, 1);
            assert_eq!(separator.y, status.y + status.height);
            assert_eq!(layout.body().y, separator.y + separator.height);
            assert!(layout.body().height > 0);

            let mut view = PlanReviewViewState::default();
            for _ in 0..layout.max_vertical() {
                view.apply_with_matches(
                    PlanReviewInput::Down,
                    layout.body(),
                    layout.max_vertical(),
                    layout.max_horizontal(),
                    "worker",
                    &[],
                );
            }
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let text = buffer_text(&buffer);
            write_buffer_captures("ux02-filter-narrow", &buffer);
            assert!(text.contains("4 matches"));
            assert!(!text.contains("Filter changes display only"));
            assert!(!text.contains("Matching changes"));

            let tiny_buffer = render_to_buffer((24, 6), |frame| {
                render(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                );
            });
            write_buffer_captures("ux02-filter-terminal-too-small", &tiny_buffer);
            assert!(buffer_text(&tiny_buffer).contains("Terminal too small"));
        }

        #[test]
        fn production_filter_resize_notice_keeps_escape_cancel_available() {
            let area = Rect::new(0, 0, 24, 6);
            let state = review_state(review());
            let mut view = PlanReviewViewState::default();
            let initial_layout = layout(area, false, &state);
            view.apply_with_matches(
                PlanReviewInput::SearchStart,
                initial_layout.body(),
                initial_layout.max_vertical(),
                initial_layout.max_horizontal(),
                state.review().search_query(),
                &[],
            );

            let searching_layout = layout(area, view.searching(), &state);
            assert_eq!(searching_layout.body().height, 0);
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            write_buffer_captures("ux02-filter-input-terminal-too-small", &buffer);
            assert!(buffer_text(&buffer).contains("press Esc"));
            assert!(buffer_text(&buffer).contains("cancel"));
            assert_eq!(
                key_to_input(
                    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                    view.searching(),
                    false,
                ),
                Some(PlanReviewInput::SearchCancel)
            );

            let input = key_to_input(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                view.searching(),
                false,
            )
            .expect("Esc should cancel the filter");
            assert_eq!(
                view.apply_with_matches(
                    input,
                    searching_layout.body(),
                    searching_layout.max_vertical(),
                    searching_layout.max_horizontal(),
                    state.review().search_query(),
                    &[],
                ),
                Some(String::new())
            );
            assert!(!view.searching());
        }

        #[test]
        fn production_confirmed_filter_resize_notice_keeps_escape_clear_available() {
            let mut plan = review();
            plan.set_search_query("worker".to_owned());
            let state = review_state(plan);
            let buffer = render_to_buffer((24, 6), |frame| {
                render(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                );
            });

            let text = buffer_text(&buffer);
            assert!(text.contains("press Esc"));
            assert!(text.contains("clear filter"));
        }

        #[test]
        fn search_prompt_keeps_the_cursor_visible() {
            let mut view = PlanReviewViewState::default();
            let body = Rect::new(0, 0, 10, 10);
            view.apply_with_matches(PlanReviewInput::SearchStart, body, 0, 0, "", &[]);
            for character in "abcdefgh".chars() {
                view.apply_with_matches(
                    PlanReviewInput::SearchChar(character),
                    body,
                    0,
                    0,
                    "",
                    &[],
                );
            }
            let Some((line, horizontal)) = search_prompt(&view, 6) else {
                panic!("search prompt should be visible");
            };
            assert_eq!(line.to_string(), "/abcdefgh ");
            assert_eq!(horizontal, 4);
        }

        #[test]
        fn search_prompt_keeps_a_wide_cursor_inside_the_input_width() {
            let mut view = PlanReviewViewState::default();
            let body = Rect::new(0, 0, 10, 10);
            view.apply_with_matches(PlanReviewInput::SearchStart, body, 0, 0, "", &[]);
            for character in "aaaaaaaaaaaaaaaaaaaa😀".chars() {
                view.apply_with_matches(
                    PlanReviewInput::SearchChar(character),
                    body,
                    0,
                    0,
                    "",
                    &[],
                );
            }
            view.apply_with_matches(PlanReviewInput::SearchLeft, body, 0, 0, "", &[]);

            let Some((line, horizontal)) = search_prompt(&view, 22) else {
                panic!("search prompt should be visible");
            };
            assert_eq!(line.width(), 23);
            assert_eq!(horizontal, 1);
        }

        #[test]
        fn filtered_body_omits_the_plan_summary() {
            let mut review = PlanReview::new(
                PathBuf::from("/project"),
                "default".to_owned(),
                PlanDocument::with_blocks_and_line_kinds(
                    "Plan: 1 to add, 0 to change, 0 to destroy.\n".to_owned(),
                    vec![PlanBlock::new(0..2, PlanBlockKind::Common)],
                    vec![PlanLineKind::Summary, PlanLineKind::Body],
                ),
                PlanMetadata::new(Vec::new(), Vec::new(), 1, 0, 0, true),
                Vec::new(),
            );
            review.set_search_query("api".to_owned());

            let filtered = review.document().filter(review.search_query());
            let lines = review_lines(&review, &filtered, true, "api").0;
            assert!(
                lines
                    .iter()
                    .all(|line| line.to_string() != "Plan total (full plan):")
            );
            assert!(
                lines.iter().all(|line| {
                    line.to_string() != "Plan: 1 to add, 0 to change, 0 to destroy."
                })
            );
        }
    }

    mod confirmation {
        use super::*;

        fn confirmation_review(
            root: &str,
            workspace: &str,
            additions: usize,
            changes: usize,
            deletions: usize,
        ) -> PlanReview {
            PlanReview::new(
                PathBuf::from(root),
                workspace.to_owned(),
                plan_document("Plan: 0 to add, 0 to change, 0 to destroy.\n".to_owned()),
                PlanMetadata::new(Vec::new(), Vec::new(), additions, changes, deletions, true),
                Vec::new(),
            )
        }

        #[test]
        fn production_apply_confirmation_uses_body_input_and_accent_cursor() {
            let state = confirmation_state(review());
            let mut view = ApplyConfirmationViewState::default();
            for character in "yes".chars() {
                view.apply(ApplyConfirmationInput::Character(character));
            }
            let buffer = render_to_buffer((120, 40), |frame| {
                render_apply_confirmation(frame, &state, &view);
            });

            assert_text_prefix_uses_style(
                &buffer,
                "yes|",
                "yes",
                Color::Rgb(0xe9, 0xdb, 0xdb),
                Color::Reset,
                Modifier::empty(),
            );
            assert!(buffer_text(&buffer).contains("> yes|"));
            assert_text_segment_uses_style(
                &buffer,
                "yes|",
                3,
                1,
                Color::Rgb(0xf4, 0x9e, 0x4c),
                Color::Reset,
                Modifier::empty(),
            );
        }

        #[test]
        fn production_confirmation_layout_keeps_the_footer_adjacent_to_a_compact_frame() {
            for &(width, height) in &SIZES {
                let layout = apply_confirmation_layout(
                    Rect::new(0, 0, width, height),
                    &confirmation_state(review()),
                );

                assert!(layout.renderable());
                assert_eq!(
                    layout.frame().width,
                    width.saturating_sub(2).min(CONFIRMATION_MAX_WIDTH)
                );
                assert_eq!(layout.footer().y, layout.frame().bottom());
                assert_eq!(layout.footer().x, layout.frame().x);
                assert_eq!(layout.inner().width, layout.frame().width - 4);
                assert_eq!(layout.inner().height, layout.frame().height - 4);
                assert_eq!(layout.input().height, 1);
                assert!(layout.frame().height < height);
            }
        }

        #[test]
        fn production_confirmation_wraps_target_and_preserves_scope_and_workspace() {
            let plan = confirmation_review(
                "/repo/environments/production/東京/with-a-very-long-target-name-that-must-wrap",
                "staging",
                0,
                1,
                0,
            );
            let state = confirmation_state(plan);
            let area = Rect::new(0, 0, 48, 30);
            let layout = apply_confirmation_layout(area, &state);
            assert!(layout.renderable());
            assert!(layout.frame().height > 12);
            let too_short = Rect::new(0, 0, area.width, 12);
            assert!(!apply_confirmation_layout(too_short, &state).renderable());
            let too_short_buffer = render_to_buffer((too_short.width, too_short.height), |frame| {
                render_apply_confirmation(frame, &state, &ApplyConfirmationViewState::default());
            });
            assert!(buffer_text(&too_short_buffer).contains("Terminal too small"));

            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render_apply_confirmation(frame, &state, &ApplyConfirmationViewState::default());
            });
            let text = buffer_text(&buffer);
            let flat = text.replace('\n', "");
            let compact = flat
                .chars()
                .filter(|character| !character.is_whitespace() && *character != '│')
                .collect::<String>();
            assert!(text.contains("Target:"));
            assert!(compact.contains("/repo"));
            assert!(compact.contains("environments/production"));
            assert!(compact.contains("東京"));
            assert!(compact.contains("with-a-very-long-target-name-that-must-wrap"));
            assert!(text.contains("Workspace: staging"));
            assert!(text.contains("Plan: 0 to add, 1 to change, 0 to destroy."));
            assert!(!text.contains("This plan includes resource deletion."));
            assert_eq!(layout.footer().y, layout.frame().bottom());
        }

        #[test]
        fn production_confirmation_requires_a_complete_footer_and_keeps_notice_below_header() {
            let state = confirmation_state(review());
            let narrow = Rect::new(0, 0, 24, 30);
            assert!(!apply_confirmation_layout(narrow, &state).renderable());

            let area = Rect::new(0, 0, 48, 12);
            let layout = apply_confirmation_layout(area, &state);
            assert!(!layout.renderable());
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render_apply_confirmation(frame, &state, &ApplyConfirmationViewState::default());
            });
            let text = buffer_text(&buffer);
            let lines = text.lines().collect::<Vec<_>>();
            assert!(lines[usize::from(layout.header().y)].contains("Terracotta |"));
            assert!(!lines[usize::from(layout.header().y)].contains("Terminal too small"));
            assert!(lines[usize::from(layout.notice().y)].contains("Terminal too small"));
        }

        #[test]
        fn production_confirmation_uses_role_styles_for_labels_values_scope_and_warning() {
            let state = confirmation_state(review());
            let buffer = render_to_buffer((120, 40), |frame| {
                render_apply_confirmation(frame, &state, &ApplyConfirmationViewState::default());
            });
            assert_text_segment_uses_style(
                &buffer,
                "Target: /repo/environments/production/main",
                0,
                "Target: ".chars().count(),
                Color::Rgb(0xc0, 0xb8, 0xb8),
                Color::Reset,
                Modifier::empty(),
            );
            assert_text_segment_uses_style(
                &buffer,
                "Target: /repo/environments/production/main",
                "Target: ".chars().count(),
                "/repo/environments/production/main".chars().count(),
                Color::Rgb(0xe9, 0xdb, 0xdb),
                Color::Reset,
                Modifier::empty(),
            );
            assert_text_segment_uses_style(
                &buffer,
                "Workspace: default",
                0,
                "Workspace: ".chars().count(),
                Color::Rgb(0xc0, 0xb8, 0xb8),
                Color::Reset,
                Modifier::empty(),
            );
            assert_text_prefix_uses_style(
                &buffer,
                "This plan includes resource deletion.",
                "This plan includes resource deletion.",
                Color::Rgb(0xeb, 0xcb, 0x8b),
                Color::Reset,
                Modifier::BOLD,
            );
        }

        #[test]
        fn production_confirmation_scrolls_long_input_to_the_cursor() {
            let state = confirmation_state(review());
            let mut view = ApplyConfirmationViewState::default();
            for character in "this-is-a-long-invalid-confirmation-input"
                .repeat(3)
                .chars()
            {
                view.apply(ApplyConfirmationInput::Character(character));
            }
            let layout = apply_confirmation_layout(Rect::new(0, 0, 80, 24), &state);
            assert!(confirmation_input_scroll(&view, layout.input().width) > 0);

            let buffer = render_to_buffer((80, 24), |frame| {
                render_apply_confirmation(frame, &state, &view);
            });
            assert!(buffer_text(&buffer).contains("input|"));
        }
    }

    mod overlay {
        use super::*;

        fn diagnostic_review() -> PlanReview {
            PlanReview::new(
                PathBuf::from("/repo/environments/production/main"),
                "default".to_owned(),
                PlanDocument::with_blocks_and_line_kinds(
                    "Plan: 1 to add, 0 to change, 0 to destroy.\n".to_owned(),
                    vec![PlanBlock::new(0..2, PlanBlockKind::Common)],
                    vec![PlanLineKind::Summary, PlanLineKind::Body],
                ),
                PlanMetadata::new(
                    vec!["terraform_data.api".to_owned()],
                    Vec::new(),
                    1,
                    0,
                    0,
                    true,
                ),
                vec![
                    Diagnostic {
                        severity: DiagnosticSeverity::Error,
                        summary: "Invalid configuration".to_owned(),
                        detail: Some("error detail line 1\nerror detail line 2".to_owned()),
                        position: None,
                        source: DiagnosticSource::Terraform,
                    },
                    Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        summary: "Deprecated configuration".to_owned(),
                        detail: Some("warning detail line 1\nwarning detail line 2".to_owned()),
                        position: None,
                        source: DiagnosticSource::Terraform,
                    },
                ],
            )
        }

        fn assert_area_unchanged(before: &Buffer, after: &Buffer, area: Rect) {
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    assert_eq!(
                        before.cell((x, y)).expect("before cell"),
                        after.cell((x, y)).expect("after cell"),
                        "cell changed at ({x}, {y})"
                    );
                }
            }
        }

        fn assert_frame_unchanged(before: &Buffer, after: &Buffer, area: Rect) {
            for x in area.x..area.right() {
                assert_eq!(
                    before.cell((x, area.y)).expect("top frame cell"),
                    after.cell((x, area.y)).expect("top frame cell")
                );
                assert_eq!(
                    before
                        .cell((x, area.bottom() - 1))
                        .expect("bottom frame cell"),
                    after
                        .cell((x, area.bottom() - 1))
                        .expect("bottom frame cell")
                );
            }
            for y in area.y..area.bottom() {
                assert_eq!(
                    before.cell((area.x, y)).expect("left frame cell"),
                    after.cell((area.x, y)).expect("left frame cell")
                );
                assert_eq!(
                    before
                        .cell((area.right() - 1, y))
                        .expect("right frame cell"),
                    after.cell((area.right() - 1, y)).expect("right frame cell")
                );
            }
        }

        fn assert_flash_body_cells(before: &Buffer, after: &Buffer, body: Rect) {
            let mut flashed_cells = 0;
            for y in body.y..body.bottom() {
                if y == body.y {
                    continue;
                }
                let last_content = (body.x..body.right()).rev().find(|&x| {
                    let cell = before.cell((x, y)).expect("before plan cell");
                    !cell.symbol().is_empty() && !cell.symbol().chars().all(char::is_whitespace)
                });
                let Some(last_content) = last_content else {
                    for x in body.x..body.right() {
                        assert_eq!(
                            before.cell((x, y)).expect("before blank cell"),
                            after.cell((x, y)).expect("after blank cell"),
                            "empty row changed at ({x}, {y})"
                        );
                    }
                    continue;
                };
                for x in body.x..body.right() {
                    let before_cell = before.cell((x, y)).expect("before plan cell");
                    let after_cell = after.cell((x, y)).expect("after plan cell");

                    assert_eq!(before_cell.symbol(), after_cell.symbol());
                    if x > last_content || before_cell.symbol().is_empty() {
                        assert_eq!(before_cell, after_cell, "blank cell changed at ({x}, {y})");
                    } else {
                        assert_eq!(after_cell.fg, Color::Rgb(0x11, 0x14, 0x19));
                        assert_eq!(after_cell.bg, Color::Rgb(0xf4, 0x9e, 0x4c));
                        flashed_cells += 1;
                    }
                }
            }
            assert!(flashed_cells > 0);
        }

        fn assert_area_restored_after_flash(before: &Buffer, after: &Buffer, body: Rect) {
            for y in body.y.saturating_add(1)..body.bottom() {
                for x in body.x..body.right() {
                    assert_eq!(
                        before.cell((x, y)).expect("before plan cell"),
                        after.cell((x, y)).expect("after plan cell"),
                        "plan body should restore after flash at ({x}, {y})"
                    );
                }
            }
        }

        #[test]
        fn quit_confirmation_replaces_the_plan_footer_and_has_a_narrow_notice() {
            let state = review_state(review_with_applyable(false));
            let buffer = render_to_buffer((80, 24), |frame| {
                render_with_quit_confirmation(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                    true,
                );
            });
            let text = buffer_text(&buffer);
            assert!(text.contains("Quit Terracotta?   [Enter] Quit   [Esc] Cancel"));
            assert!(!text.contains("q quit"));

            let narrow = render_to_buffer((32, 9), |frame| {
                render_with_quit_confirmation(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                    true,
                );
            });
            assert!(buffer_text(&narrow).contains("Quit? Enter exit / Esc cancel"));
        }

        #[test]
        fn production_filter_uses_support_style_for_footer_count() {
            let mut plan = review();
            plan.set_search_query(SEARCH_TERM.to_owned());
            let state = review_state(plan);
            let buffer = render_to_buffer((160, 60), |frame| {
                render(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                );
            });

            assert_text_prefix_uses_style(
                &buffer,
                "8 matches",
                "8 matches",
                Color::Rgb(0xc0, 0xb8, 0xb8),
                Color::Reset,
                Modifier::empty(),
            );
        }

        #[test]
        fn copy_flash_styles_plan_cells_without_overwriting_the_review_shell() {
            let (before, flash, flash_at_100ms, after, layout) = copy_flash_buffers();

            assert_eq!(buffer_text(&flash), buffer_text(&flash_at_100ms));
            assert!(buffer_text(&flash).contains("Copied."));
            assert_text_prefix_uses_style(
                &flash,
                "terraform_data.api",
                "terraform_data",
                Color::Rgb(0x11, 0x14, 0x19),
                Color::Rgb(0xf4, 0x9e, 0x4c),
                Modifier::empty(),
            );
            assert_text_prefix_uses_style(
                &flash_at_100ms,
                "terraform_data.api",
                "terraform_data",
                Color::Rgb(0x11, 0x14, 0x19),
                Color::Rgb(0xf4, 0x9e, 0x4c),
                Modifier::empty(),
            );
            assert_area_restored_after_flash(&before, &after, layout.body());

            assert_area_unchanged(&before, &flash, layout.shell.header());
            assert_text_prefix_uses_style(
                &flash,
                "Copied.",
                "Copied.",
                Color::Rgb(0xf4, 0x9e, 0x4c),
                Color::Reset,
                Modifier::empty(),
            );
            assert_frame_unchanged(&before, &flash, layout.shell.content());
            assert_area_unchanged(&before, &flash, layout.status());
            assert_area_unchanged(&before, &flash, layout.separator());
            assert_area_unchanged(
                &before,
                &flash,
                Rect::new(
                    layout.body().x + layout.body().width,
                    layout.body().y,
                    u16::from(layout.vertical_scrollbar()),
                    layout.body().height,
                ),
            );
            assert_area_unchanged(
                &before,
                &flash,
                Rect::new(
                    layout.body().x,
                    layout.body().y + layout.body().height,
                    layout.body().width + u16::from(layout.vertical_scrollbar()),
                    u16::from(layout.horizontal_scrollbar()),
                ),
            );
            assert_flash_body_cells(&before, &flash, layout.body());
        }

        fn copy_flash_buffers() -> (Buffer, Buffer, Buffer, Buffer, PlanReviewLayout) {
            let area = Rect::new(0, 0, 80, 24);
            let mut plan = review();
            plan.set_search_query(SEARCH_TERM.to_owned());
            let state = review_state(plan);
            let scroll_layout = layout(area, false, &state);
            let mut view = PlanReviewViewState::default();
            view.apply_with_matches(
                PlanReviewInput::Right,
                area,
                scroll_layout.max_vertical(),
                scroll_layout.max_horizontal(),
                SEARCH_TERM,
                &[],
            );
            view.apply_with_matches(
                PlanReviewInput::SearchStart,
                area,
                scroll_layout.max_vertical(),
                scroll_layout.max_horizontal(),
                SEARCH_TERM,
                &[],
            );
            assert_eq!(view.scroll(), (0, 1));

            let started_at = Instant::now();
            let before = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, started_at);
            });
            let mut session = SessionState::new(ExecutionState::with_context(
                started_at,
                ExecutionContext::loading("/repo"),
            ));
            session::update(
                &mut session,
                Action::ReviewCompleted(state.review().clone()),
                started_at,
            );
            let layout = layout(
                area,
                true,
                session.review().expect("review should be visible"),
            );
            session::update(
                &mut session,
                Action::CopyCompleted {
                    target: CopyTarget::Plan,
                    result: CopyResult::Written,
                },
                started_at,
            );

            let flash = render_to_buffer((area.width, area.height), |frame| {
                render(
                    frame,
                    session.review().expect("review should be visible"),
                    &view,
                    started_at,
                );
            });
            let flash_at_100ms = render_to_buffer((area.width, area.height), |frame| {
                render(
                    frame,
                    session.review().expect("review should be visible"),
                    &view,
                    started_at + std::time::Duration::from_millis(100),
                );
            });
            let after = render_to_buffer((area.width, area.height), |frame| {
                render(
                    frame,
                    session.review().expect("review should be visible"),
                    &view,
                    started_at + std::time::Duration::from_millis(201),
                );
            });
            (before, flash, flash_at_100ms, after, layout)
        }

        #[test]
        fn production_review_render_orders_diagnostics_before_plan_and_styles_severity() {
            let state = review_state(diagnostic_review());
            let view = PlanReviewViewState::default();
            let area = Rect::new(0, 0, 120, 40);
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let text = buffer_text(&buffer);
            let lines = text.lines().collect::<Vec<_>>();
            let position = |marker: &str| {
                lines
                    .iter()
                    .position(|line| line.contains(marker))
                    .unwrap_or_else(|| panic!("text should be visible: {marker}"))
            };

            assert!(position("Error: Invalid configuration") < position("error detail line 1"));
            assert!(
                position("error detail line 2") < position("Warning: Deprecated configuration")
            );
            assert!(
                position("Warning: Deprecated configuration") < position("warning detail line 1")
            );
            assert!(position("Plan: 1 to add") < position("Error: Invalid configuration"));
            assert_text_prefix_uses_style(
                &buffer,
                "Error: Invalid configuration",
                "Error",
                Color::Rgb(0xbf, 0x61, 0x6a),
                Color::Reset,
                Modifier::BOLD,
            );
            assert_text_prefix_uses_style(
                &buffer,
                "Warning: Deprecated configuration",
                "Warning",
                Color::Rgb(0xeb, 0xcb, 0x8b),
                Color::Reset,
                Modifier::BOLD,
            );
        }

        #[test]
        fn non_applyable_review_footer_keeps_viewing_actions_without_apply() {
            let state = review_state(review_with_applyable(false));
            let view = PlanReviewViewState::default();
            let area = Rect::new(0, 0, 120, 40);
            let layout = layout(area, false, &state);
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let footer = layout.shell.footer();
            let mut footer_text = String::new();
            for y in footer.y..footer.bottom() {
                for x in footer.x..footer.right() {
                    footer_text.push_str(buffer.cell((x, y)).expect("footer cell").symbol());
                }
            }

            assert!(!footer_text.contains("a apply"), "{footer_text}");
            assert!(footer_text.contains("/ filter"), "{footer_text}");
            assert!(footer_text.contains("y copy plan"), "{footer_text}");
            assert!(footer_text.contains("q quit"), "{footer_text}");
        }
    }
}
