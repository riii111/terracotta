use std::time::Instant;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{
    copy::CopyNotice,
    execution::{DiagnosticSeverity, ExecutionContext, ExecutionContextValue},
    review::{FilteredPlan, PlanLineKind, PlanMetadata, PlanReview},
    session::{ApplyConfirmationState, ReviewSessionState},
};
use crate::ui::primitives::{
    atoms::{scrollbar, separator},
    molecules::{help_dialog, terminal_notice},
};
use crate::ui::shell::{context, footer, header, layout as shell_layout};
use crate::ui::theme;

use super::{
    ApplyConfirmationViewState, ConfirmationOverlay, PlanReviewMatch, PlanReviewOverlay,
    PlanReviewViewState,
};

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
    line_number: usize,
}

pub(crate) struct ApplyConfirmationLayout {
    header: Rect,
    notice: Rect,
    frame: Rect,
    footer: Rect,
    inner: Rect,
    input: Rect,
    prefix: Rect,
    scroll: Rect,
    suffix: Rect,
    prefix_lines: Vec<Line<'static>>,
    scroll_lines: Vec<Line<'static>>,
    suffix_lines: Vec<Line<'static>>,
    footer_lines: Vec<Line<'static>>,
    max_vertical: u16,
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

    pub(crate) const fn prefix(&self) -> Rect {
        self.prefix
    }

    pub(crate) const fn scroll(&self) -> Rect {
        self.scroll
    }

    pub(crate) const fn suffix(&self) -> Rect {
        self.suffix
    }

    pub(crate) fn prefix_lines(&self) -> &[Line<'static>] {
        &self.prefix_lines
    }

    pub(crate) fn scroll_lines(&self) -> &[Line<'static>] {
        &self.scroll_lines
    }

    pub(crate) fn suffix_lines(&self) -> &[Line<'static>] {
        &self.suffix_lines
    }

    pub(crate) fn footer_lines(&self) -> &[Line<'static>] {
        &self.footer_lines
    }

    pub(crate) const fn max_vertical(&self) -> u16 {
        self.max_vertical
    }

    pub(crate) const fn renderable(&self) -> bool {
        self.renderable
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReviewNavigation {
    Standalone,
    Environments,
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

pub(crate) fn source_offset(review: &ReviewSessionState, line: usize) -> usize {
    let content = prepare_content(review, false, "");
    content
        .sources
        .iter()
        .position(|source| source.is_some_and(|source| source.line_number == line))
        .unwrap_or(0)
}

pub(crate) fn environment_layout(
    area: Rect,
    searching: bool,
    state: &ReviewSessionState,
) -> PlanReviewLayout {
    layout_for_navigation(
        area,
        searching,
        state,
        false,
        ReviewNavigation::Environments,
    )
}

pub(crate) fn layout_with_quit_confirmation(
    area: Rect,
    searching: bool,
    state: &ReviewSessionState,
    quit_confirmation: bool,
) -> PlanReviewLayout {
    layout_for_navigation(
        area,
        searching,
        state,
        quit_confirmation,
        ReviewNavigation::Standalone,
    )
}

fn layout_for_navigation(
    area: Rect,
    searching: bool,
    state: &ReviewSessionState,
    quit_confirmation: bool,
    navigation: ReviewNavigation,
) -> PlanReviewLayout {
    let filtered_view = filter_active(searching, state);
    let content = prepare_view_content(state, filtered_view);
    layout_with_content(
        area,
        searching,
        state,
        &content,
        state.copy_feedback().notice(),
        quit_confirmation,
        navigation,
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
    copy_notice: Option<CopyNotice>,
    quit_confirmation: bool,
    navigation: ReviewNavigation,
) -> PlanReviewLayout {
    let panel_width = area.width;
    let content_metrics = content.metrics();
    let applyable = state.review().apply_entry()
        && state.review().apply_allowed()
        && state.review().metadata().applyable();
    let filter_visible = filter_active(searching, state);
    let showing = filter_footer_status(
        state.review().search_query(),
        content.matches.len(),
        panel_width,
    );
    let footer_status = if quit_confirmation {
        None
    } else {
        copy_notice
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
                if filter_visible {
                    showing.as_ref().map(|message| {
                        (
                            review_footer_status_text(
                                message,
                                &position_status_for_content(
                                    content,
                                    0,
                                    state.review().document().text().split('\n').count(),
                                    panel_width,
                                ),
                            ),
                            theme::secondary_style(),
                        )
                    })
                } else {
                    Some((
                        position_status_for_content(
                            content,
                            0,
                            state.review().document().text().split('\n').count(),
                            panel_width,
                        ),
                        theme::secondary_style(),
                    ))
                }
            })
    };
    let footer_message = footer_status.as_ref().map(|(message, _)| message.as_str());
    let available_footer_width = footer::available_width(panel_width, footer_message);
    let normal_footer_lines = footer::layout_with_notice(
        footer_items(
            searching,
            applyable,
            content.matches.len(),
            filter_visible,
            navigation,
            available_footer_width,
        ),
        panel_width,
        footer_message,
    );
    let normal_required = footer::layout_with_notice(
        required_footer_items(searching, content.matches.len(), filter_visible, navigation),
        panel_width,
        footer_message,
    );
    let fixed_status_height: u16 = 2;
    let footer_height = common_footer_height(
        applyable,
        content.matches.len(),
        panel_width,
        copy_notice.map(CopyNotice::message),
        showing.as_deref(),
        navigation,
    );
    let frame_footer_lines = footer::pad_lines(normal_footer_lines, footer_height);
    let frame_required = footer::pad_lines(normal_required, footer_height);
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
    let shell = shell_layout::full_width_layout(area, footer_lines, required);
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
    navigation: ReviewNavigation,
) -> usize {
    [None, copy_notice, showing]
        .into_iter()
        .flat_map(|notice| {
            [
                footer_items(
                    false,
                    applyable,
                    match_count,
                    false,
                    navigation,
                    footer::available_width(width, notice),
                ),
                footer_items(
                    true,
                    applyable,
                    match_count,
                    true,
                    navigation,
                    footer::available_width(width, notice),
                ),
                footer_items(
                    false,
                    applyable,
                    match_count.max(2),
                    true,
                    navigation,
                    footer::available_width(width, notice),
                ),
                required_footer_items(false, match_count, false, navigation),
                required_footer_items(true, match_count, true, navigation),
                required_footer_items(false, match_count.max(2), true, navigation),
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
    let background = ReviewSessionState::new(state.review().clone());
    render_with_quit_confirmation(
        frame,
        &background,
        &PlanReviewViewState::default(),
        Instant::now(),
        false,
    );
    dim_background(frame);
    let layout = apply_confirmation_layout(area, state);
    if layout.header().height > 0 {
        header::render_review(frame, layout.header(), state.review());
    }
    if !layout.renderable() {
        terminal_notice::render_wrapped(frame, layout.notice(), CONFIRMATION_NOTICE);
        return;
    }

    frame.render_widget(Clear, layout.frame());
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
    debug_assert_eq!(
        info_area,
        Rect::new(
            layout.prefix().x,
            layout.prefix().y,
            layout.prefix().width,
            layout.prefix().height + layout.scroll().height + layout.suffix().height
        )
    );
    frame.render_widget(
        Paragraph::new(layout.prefix_lines().to_owned())
            .style(theme::body_style())
            .wrap(Wrap { trim: false }),
        layout.prefix(),
    );
    frame.render_widget(
        Paragraph::new(layout.scroll_lines().to_owned())
            .style(theme::body_style())
            .wrap(Wrap { trim: false })
            .scroll((view.scroll().min(layout.max_vertical()), 0)),
        layout.scroll(),
    );
    frame.render_widget(
        Paragraph::new(layout.suffix_lines().to_owned())
            .style(theme::body_style())
            .wrap(Wrap { trim: false }),
        layout.suffix(),
    );
    frame.render_widget(
        Paragraph::new(confirmation_input_line(view))
            .style(theme::body_style())
            .scroll((0, confirmation_input_scroll(view, layout.input().width))),
        layout.input(),
    );
    footer::render(frame, layout.footer(), layout.footer_lines(), None);
    clear_dim(frame, layout.header());
    clear_dim(frame, layout.frame());
    clear_dim(frame, layout.footer());
    if let Some(overlay) = view.overlay() {
        render_confirmation_overlay(frame, area, state.review(), overlay, view.overlay_scroll());
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the confirmation layout keeps content and safety constraints together"
)]
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
        footer::hint(&["?"], "help"),
    ];
    let footer_lines = footer::layout(footer_items.clone(), frame_width);
    let footer_required_width = footer_items.iter().map(Line::width).sum::<usize>()
        + footer_items.len().saturating_sub(1) * 3;
    let footer_fits = footer_lines.len() == 1
        && footer_lines
            .first()
            .is_some_and(|line| line.width() == footer_required_width);
    let inner_width = frame_width.saturating_sub(4);
    let lines = confirmation_lines(state);
    let body = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
    let prefix_lines = lines[..7].to_vec();
    let suffix_start = lines.len().saturating_sub(2);
    let scroll_lines = lines[7..suffix_start].to_vec();
    let suffix_lines = lines[suffix_start..].to_vec();
    let body_height = body.line_count(inner_width).saturating_add(1);
    let natural_frame_height = u16::try_from(body_height)
        .unwrap_or(u16::MAX)
        .saturating_add(4);
    let max_frame_height = available.height.saturating_sub(1);
    let frame_height = natural_frame_height.min(max_frame_height);
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
    let info_height = inner.height.saturating_sub(1);
    let prefix_height = u16::try_from(
        Paragraph::new(prefix_lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(inner_width),
    )
    .unwrap_or(u16::MAX);
    let suffix_height = u16::try_from(
        Paragraph::new(suffix_lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(inner_width),
    )
    .unwrap_or(u16::MAX);
    let fixed_height = prefix_height.saturating_add(suffix_height);
    let scroll_height = info_height.saturating_sub(fixed_height);
    let prefix = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        prefix_height.min(info_height),
    );
    let suffix_y = inner.y + info_height.saturating_sub(suffix_height);
    let suffix = Rect::new(
        inner.x,
        suffix_y,
        inner.width,
        suffix_height.min(info_height),
    );
    let scroll = Rect::new(
        inner.x,
        inner.y.saturating_add(prefix.height),
        inner.width,
        scroll_height,
    );
    let renderable = renderable
        && usize::from(info_height) >= usize::from(fixed_height)
        && (scroll_lines.is_empty() || scroll_height > 0);
    let max_vertical = u16::try_from(
        Paragraph::new(scroll_lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(inner_width)
            .saturating_sub(usize::from(scroll_height)),
    )
    .unwrap_or(u16::MAX);
    ApplyConfirmationLayout {
        header,
        notice: available,
        frame,
        footer,
        inner,
        input,
        prefix,
        scroll,
        suffix,
        prefix_lines,
        scroll_lines,
        suffix_lines,
        footer_lines,
        max_vertical,
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
    let context = state.review().context();
    let target = match context.display_name() {
        ExecutionContextValue::Known(name) => {
            let suffix = context.is_production().is_some_and(|production| production);
            if suffix {
                format!("{name} [PROD]")
            } else {
                name.clone()
            }
        }
        ExecutionContextValue::Loading => "loading...".to_owned(),
    };
    let mut lines = vec![
        Line::from("Apply this reviewed plan?"),
        Line::default(),
        Line::from(vec![
            Span::styled("Target: ", theme::secondary_style()),
            Span::styled(target, theme::body_style()),
        ]),
        Line::from(vec![
            Span::styled("Workspace: ", theme::secondary_style()),
            Span::styled(state.review().workspace().to_owned(), theme::body_style()),
        ]),
        Line::from(vec![
            Span::styled("Directory: ", theme::secondary_style()),
            Span::styled(
                context::relative_directory(context.cwd_path(), context.launch_root_path()),
                theme::body_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Tool: ", theme::secondary_style()),
            Span::styled(tool_version(context), theme::body_style()),
        ]),
        Line::from(format!(
            "Plan: +{} add  ~{} update  {} replace  -{} destroy",
            metadata.additions(),
            metadata.changes(),
            metadata.replacements(),
            metadata.deletions(),
        )),
    ];
    append_variable_sources(&mut lines, context);
    append_destructive_resources(&mut lines, metadata);
    if !state.review().search_query().is_empty() {
        lines.push(Line::from(Span::styled(
            "Filter changes display only. Apply uses all changes.",
            theme::secondary_style(),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(format!(
        "Type {} to apply (exact match).",
        state.review().confirmation_input()
    )));
    lines
}

fn append_variable_sources(lines: &mut Vec<Line<'static>>, context: &ExecutionContext) {
    let sources = context.variable_sources();
    if sources.automatic_files().is_empty()
        && sources.explicit_files().is_empty()
        && !sources.has_var_argument()
        && sources.environment_variables().is_empty()
    {
        return;
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Variable sources:",
        theme::secondary_style(),
    )));
    for path in sources.automatic_files() {
        lines.push(Line::from(format!("  auto: {}", source_name(path))));
    }
    for path in sources.explicit_files() {
        lines.push(Line::from(format!("  -var-file: {}", source_name(path))));
    }
    if sources.has_var_argument() {
        lines.push(Line::from("  -var: provided"));
    }
    if !sources.environment_variables().is_empty() {
        lines.push(Line::from(format!(
            "  TF_VAR_*: {} provided",
            sources.environment_variables().len()
        )));
    }
}

fn append_destructive_resources(lines: &mut Vec<Line<'static>>, metadata: &PlanMetadata) {
    let destroy = metadata.destructive_addresses().collect::<Vec<_>>();
    let replace = metadata.replacement_addresses().collect::<Vec<_>>();
    if destroy.is_empty() && replace.is_empty() {
        return;
    }
    lines.push(Line::default());
    for (label, addresses, style) in [
        ("Destroy", destroy, theme::error_style()),
        ("Replace", replace, theme::warning_style()),
    ] {
        if addresses.is_empty() {
            continue;
        }
        lines.push(Line::from(Span::styled(format!("{label}:"), style)));
        lines.extend(
            addresses
                .into_iter()
                .map(|address| Line::from(Span::styled(format!("  {address}"), style))),
        );
    }
}

fn source_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn tool_version(context: &ExecutionContext) -> String {
    let version = match context.tool_version() {
        ExecutionContextValue::Known(version) => version.as_str(),
        ExecutionContextValue::Loading => "loading...",
    };
    format!("{} {version}", context.tool_name())
}

fn dim_background(frame: &mut Frame<'_>) {
    for cell in &mut frame.buffer_mut().content {
        cell.set_style(cell.style().add_modifier(Modifier::DIM));
    }
}

fn clear_dim(frame: &mut Frame<'_>, area: Rect) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                cell.modifier.remove(Modifier::DIM);
            }
        }
    }
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

fn render_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    review: &PlanReview,
    view: &PlanReviewViewState,
    navigation: ReviewNavigation,
) {
    let Some(overlay) = view.overlay() else {
        return;
    };
    match overlay {
        PlanReviewOverlay::Help => help_dialog::render(
            frame,
            area,
            overlay_title(overlay),
            &plan_help_sections(
                review,
                navigation,
                !view.searching() && !review.search_query().is_empty(),
            ),
            view.overlay_scroll(),
        ),
        PlanReviewOverlay::Context => render_dialog(
            frame,
            area,
            overlay_title(overlay),
            context::context_lines(review.context()),
            view.overlay_scroll(),
        ),
    }
}

fn render_confirmation_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    review: &PlanReview,
    overlay: ConfirmationOverlay,
    scroll: u16,
) {
    match overlay {
        ConfirmationOverlay::Help => help_dialog::render(
            frame,
            area,
            confirmation_overlay_title(overlay),
            &[
                help_dialog::HelpSection::new(
                    "Navigation",
                    vec![
                        help_dialog::HelpAction::new("↑ / ↓ / PgUp / PgDn", "scroll confirmation"),
                        help_dialog::HelpAction::new("← / →", "move in confirmation input"),
                        help_dialog::HelpAction::new("Home / End", "move to input start or end"),
                    ],
                ),
                help_dialog::HelpSection::new(
                    "Input",
                    vec![
                        help_dialog::HelpAction::new("Type", "enter the confirmation text"),
                        help_dialog::HelpAction::new("Backspace", "delete before the cursor"),
                    ],
                ),
                help_dialog::HelpSection::new(
                    "Context",
                    vec![help_dialog::HelpAction::new(
                        "Tab",
                        "show execution context",
                    )],
                ),
                help_dialog::HelpSection::new(
                    "Apply",
                    vec![
                        help_dialog::HelpAction::new("Enter", "confirm apply"),
                        help_dialog::HelpAction::new("Esc", "return to plan review"),
                    ],
                ),
            ],
            scroll,
        ),
        ConfirmationOverlay::Context => render_dialog(
            frame,
            area,
            confirmation_overlay_title(overlay),
            context::context_lines(review.context()),
            scroll,
        ),
    }
}

fn plan_help_sections(
    review: &PlanReview,
    navigation: ReviewNavigation,
    filter_confirmed: bool,
) -> Vec<help_dialog::HelpSection> {
    let mut move_actions = Vec::new();
    if navigation == ReviewNavigation::Environments {
        move_actions.push(help_dialog::HelpAction::new(
            "Tab / Shift-Tab",
            "next / previous environment",
        ));
    }
    move_actions.extend([
        help_dialog::HelpAction::new("↑ / ↓ / j / k", "scroll vertically"),
        help_dialog::HelpAction::new("← / → / h / l", "scroll horizontally"),
        help_dialog::HelpAction::new("PgUp / PgDn", "scroll one page"),
        help_dialog::HelpAction::new("Home / End", "go to the top or bottom"),
    ]);
    if navigation == ReviewNavigation::Standalone {
        move_actions.push(help_dialog::HelpAction::new("s", "overview"));
    } else {
        move_actions.push(help_dialog::HelpAction::new("0 / s", "return to overview"));
    }

    let mut review_actions = vec![help_dialog::HelpAction::new(
        "/",
        if review.search_query().is_empty() {
            "filter the full plan"
        } else {
            "edit the full-plan filter"
        },
    )];
    if !review.search_query().is_empty() {
        review_actions.push(help_dialog::HelpAction::new(
            "n / N",
            "next or previous match",
        ));
    }
    if filter_confirmed {
        review_actions.push(help_dialog::HelpAction::new(
            "Esc",
            "close Help; press Esc again to clear filter",
        ));
    }
    let mut action_items = vec![
        help_dialog::HelpAction::new("c", "show execution context"),
        help_dialog::HelpAction::new("y", "copy the full plan"),
    ];
    if review.apply_entry() && review.apply_allowed() && review.metadata().applyable() {
        action_items.push(help_dialog::HelpAction::new("a", "apply the full plan"));
    }
    vec![
        help_dialog::HelpSection::new("Navigation", move_actions),
        help_dialog::HelpSection::new("Review", review_actions),
        help_dialog::HelpSection::new("Actions", action_items),
        help_dialog::HelpSection::new("Exit", vec![help_dialog::HelpAction::new("q", "quit")]),
    ]
}

const fn overlay_title(overlay: PlanReviewOverlay) -> &'static str {
    match overlay {
        PlanReviewOverlay::Help => "Help",
        PlanReviewOverlay::Context => "Context",
    }
}

const fn confirmation_overlay_title(overlay: ConfirmationOverlay) -> &'static str {
    match overlay {
        ConfirmationOverlay::Help => "Apply help",
        ConfirmationOverlay::Context => "Context",
    }
}

fn render_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &'static str,
    lines: Vec<Line<'static>>,
    scroll: u16,
) {
    let width = area.width.saturating_sub(4).min(96);
    let inner_width = width.saturating_sub(2);
    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    let height = u16::try_from(body.line_count(inner_width))
        .unwrap_or(u16::MAX)
        .saturating_add(3)
        .min(area.height.saturating_sub(2));
    if width < 12 || height < 4 {
        terminal_notice::render_wrapped(frame, area, "Terminal too small. Resize or press Esc.");
        return;
    }
    let dialog = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, dialog);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(theme::frame_style())
        .style(theme::body_style())
        .title(title);
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);
    let footer_area = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(1),
        inner.width,
        u16::from(inner.height > 0),
    );
    let content_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let max_scroll = body
        .line_count(content_area.width)
        .saturating_sub(usize::from(content_area.height));
    let scroll = u16::try_from(usize::from(scroll).min(max_scroll)).unwrap_or(u16::MAX);
    frame.render_widget(
        body.style(theme::body_style()).scroll((scroll, 0)),
        content_area,
    );
    footer::render(
        frame,
        footer_area,
        &[footer::hint(&["?", "Esc"], "close")],
        None,
    );
}

pub(crate) fn render_environment(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &ReviewSessionState,
    view: &mut PlanReviewViewState,
    now: Instant,
) {
    render_for_navigation(
        frame,
        state,
        view,
        now,
        false,
        ReviewNavigation::Environments,
        area,
    );
}

pub(crate) fn render_with_quit_confirmation(
    frame: &mut Frame<'_>,
    state: &ReviewSessionState,
    view: &PlanReviewViewState,
    now: Instant,
    quit_confirmation: bool,
) {
    let mut view = view.clone();
    render_for_navigation(
        frame,
        state,
        &mut view,
        now,
        quit_confirmation,
        ReviewNavigation::Standalone,
        frame.area(),
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "the plan renderer keeps the feature layout and content projection in one path"
)]
fn render_for_navigation(
    frame: &mut Frame<'_>,
    state: &ReviewSessionState,
    view: &mut PlanReviewViewState,
    now: Instant,
    quit_confirmation: bool,
    navigation: ReviewNavigation,
    area: Rect,
) {
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
    let content = prepare_view_content(state, filtered_view);
    let layout = layout_with_content(
        area,
        view.searching(),
        state,
        &content,
        state.copy_feedback().notice_at(now),
        quit_confirmation,
        navigation,
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
    view.reconcile_scroll(layout.max_vertical(), layout.max_horizontal());
    header::render_plan_review(frame, layout.shell.header(), state.review());
    frame.render_widget(
        Block::new().style(theme::body_style()),
        layout.shell.content(),
    );
    render_status(frame, &layout, state, view);

    let content_metrics = content.metrics();
    let line_count = content_metrics.line_count;
    let max_line_width = content_metrics.max_width;
    let max_vertical = layout.max_vertical();
    let max_horizontal = layout.max_horizontal();
    let (vertical, horizontal) = view.scroll();
    let vertical = vertical.min(max_vertical);
    let horizontal = horizontal.min(max_horizontal);
    let lines = if state.copy_feedback().flash_active(now) {
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
    let footer_status = if quit_confirmation || state.copy_feedback().notice_at(now).is_some() {
        layout.footer_status.clone()
    } else {
        Some((
            review_footer_status(state, view, &content, layout.shell.footer().width),
            theme::secondary_style(),
        ))
    };
    footer::render(
        frame,
        layout.shell.footer(),
        layout.shell.footer_lines(),
        footer_status
            .as_ref()
            .map(|(message, style)| (message.as_str(), *style)),
    );
    frame.render_widget(
        separator::render(layout.shell.footer_separator().width),
        layout.shell.footer_separator(),
    );
    render_overlay(frame, area, state.review(), view, navigation);
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

fn prepare_view_content(state: &ReviewSessionState, filtered_view: bool) -> PreparedContent<'_> {
    prepare_content(state, filtered_view, state.review().search_query())
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
        if filtered_view && filtered.matching_outputs() == 0 && kind == PlanLineKind::OutputSection
        {
            continue;
        }
        let line_index = lines.len();
        let (rendered, line_matches) =
            plan_line_and_matches(line, filter_query, line_index, None, kind);
        lines.push(rendered);
        sources.push(Some(PlanSource {
            text: line,
            kind,
            line_number,
        }));
        matches.extend(line_matches);
    }
    while lines.last().is_some_and(|line| line.width() == 0) {
        lines.pop();
        sources.pop();
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
            "Unique targets (replace once): +{} add  ~{} update  {} replace  -{} destroy",
            state.review().metadata().additions(),
            state.review().metadata().changes(),
            state.review().metadata().replacements(),
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

const FILTER_STATUS_WIDTH: usize = 10;

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

const COMPACT_POSITION_STATUS_WIDTH: u16 = 72;

fn position_status(position: u16, total: usize, width: u16) -> String {
    let total = total.max(1);
    let position = usize::from(position).saturating_add(1).min(total);
    if width >= COMPACT_POSITION_STATUS_WIDTH {
        format!("Line {position}/{total}")
    } else {
        format!("L{position}/{total}")
    }
}

fn review_footer_status(
    state: &ReviewSessionState,
    view: &PlanReviewViewState,
    content: &PreparedContent<'_>,
    width: u16,
) -> String {
    let position = position_status_for_content(
        content,
        view.scroll().0,
        state.review().document().text().split('\n').count(),
        width,
    );
    filter_footer_status(state.review().search_query(), content.matches.len(), width).map_or_else(
        || position.clone(),
        |message| review_footer_status_text(&message, &position),
    )
}

fn position_status_for_content(
    content: &PreparedContent<'_>,
    position: u16,
    total: usize,
    width: u16,
) -> String {
    let display_index = usize::from(position);
    let source_position = content
        .sources
        .get(display_index)
        .and_then(|source| source.as_ref())
        .map_or_else(
            || display_index.saturating_add(1),
            |source| source.line_number.saturating_add(1),
        );
    position_status(
        u16::try_from(source_position.saturating_sub(1)).unwrap_or(u16::MAX),
        total,
        width,
    )
}

fn review_footer_status_text(message: &str, position: &str) -> String {
    format!("{message:<FILTER_STATUS_WIDTH$}  {position}")
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
    _match_count: usize,
    filtered: bool,
    navigation: ReviewNavigation,
    width: u16,
) -> Vec<Line<'static>> {
    let mut items = if searching {
        vec![
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "cancel"),
        ]
    } else if filtered {
        let mut items = vec![
            footer::hint(&["Esc"], "clear / edit"),
            footer::hint(&["y"], "copy all"),
        ];
        if applyable {
            items.push(footer::hint(&["a"], "apply all"));
        }
        items.extend([footer::hint(&["?"], "help"), footer::hint(&["q"], "quit")]);
        items
    } else {
        let mut items = if navigation == ReviewNavigation::Standalone && width >= 29 {
            vec![
                footer::hint(&["s"], "overview"),
                footer::hint(&["/"], "filter"),
            ]
        } else {
            vec![footer::hint(&["/"], "filter")]
        };
        if applyable {
            items.push(footer::hint(&["a"], "apply"));
        }
        items.extend([footer::hint(&["?"], "help"), footer::hint(&["q"], "quit")]);
        items
    };
    if navigation == ReviewNavigation::Environments && !searching && !filtered {
        items.insert(0, footer::hint(&["Esc"], "overview"));
    }
    items
}

fn required_footer_items(
    searching: bool,
    match_count: usize,
    filtered: bool,
    navigation: ReviewNavigation,
) -> Vec<Line<'static>> {
    let mut items = if searching {
        vec![
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "cancel"),
        ]
    } else if filtered {
        let mut items = vec![
            footer::hint(&["Esc"], "clear / edit"),
            footer::hint(&["y"], "copy all"),
            footer::hint(&["?"], "help"),
            footer::hint(&["q"], "quit"),
        ];
        if match_count >= 2 {
            items.insert(1, footer::hint(&["n/N"], "next/prev"));
        }
        items
    } else {
        vec![
            footer::hint(&["/"], "filter"),
            footer::hint(&["?"], "help"),
            footer::hint(&["q"], "quit"),
        ]
    };
    if navigation == ReviewNavigation::Environments && !searching && !filtered {
        items.insert(0, footer::hint(&["Esc"], "overview"));
    }
    items
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
    use std::path::{Path, PathBuf};

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{
        buffer::Buffer,
        style::{Color, Modifier},
        widgets::Widget,
    };

    use crate::app::{
        copy::{CopyResult, CopyTarget},
        execution::{
            Diagnostic, DiagnosticSource, ExecutionContext, ExecutionState, Tool, VariableSources,
        },
        review::{
            PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata,
            test_support::{plan_document, plan_document_with_blocks},
        },
        session::{self, Action, SessionState},
    };
    use crate::ui::{
        features::plan_review::{ApplyConfirmationInput, PlanReviewInput, key_to_input},
        test_support::{
            buffer_terminal_capture, buffer_text, render_to_buffer, write_buffer_captures,
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
        review_with_options(applyable, true)
    }

    fn review_with_apply_allowed(applyable: bool, apply_allowed: bool) -> PlanReview {
        review_with_options(applyable, apply_allowed)
    }

    fn review_with_options(applyable: bool, apply_allowed: bool) -> PlanReview {
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
        .with_apply_allowed(apply_allowed)
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
        assert_text_segment_uses_style_from(
            buffer,
            buffer.area().y,
            text,
            segment_start,
            segment_length,
            (foreground, background, modifier),
        );
    }

    fn assert_text_segment_uses_style_from(
        buffer: &Buffer,
        first_line: u16,
        text: &str,
        segment_start: usize,
        segment_length: usize,
        expected: (Color, Color, Modifier),
    ) {
        let area = buffer.area();
        for y in first_line.max(area.y)..area.bottom() {
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
                assert_eq!(cell.fg, expected.0, "{text}");
                assert_eq!(cell.bg, expected.1, "{text}");
                assert_eq!(cell.modifier, expected.2, "{text}");
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
    fn renders_plan_apply_entry_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let state = review_state(review().with_apply_entry(true));
            let view = PlanReviewViewState::default();
            let buffer = render_to_buffer((width, height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let text = buffer_text(&buffer);
            let footer = text
                .lines()
                .find(|line| line.contains("s overview"))
                .expect("overview navigation should be visible");

            assert!(
                footer.starts_with("s overview"),
                "{width}x{height}\n{footer}"
            );
            assert!(footer.contains("a apply"), "{width}x{height}\n{footer}");
            assert!(
                !footer.contains("y copy plan"),
                "{width}x{height}\n{footer}"
            );
            snapshot(&format!("preview_{width}x{height}_apply-entry"), &buffer);
        }
    }

    #[test]
    fn renders_an_empty_plan_snapshot() {
        let plan = PlanReview::new(
            PathBuf::from("/repo/environments/staging/empty"),
            "default".to_owned(),
            plan_document_with_blocks(String::new(), Vec::new()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
            Vec::new(),
        );
        let state = review_state(plan);
        let buffer = render_to_buffer((120, 40), |frame| {
            render(
                frame,
                &state,
                &PlanReviewViewState::default(),
                Instant::now(),
            );
        });

        snapshot("preview_120x40_empty", &buffer);
    }

    #[test]
    fn renders_long_target_header_and_preserves_position_for_overlays() {
        let plan = review().with_apply_entry(true).with_context(
            ExecutionContext::loading(
                "/repo/environments/production/very-long-target-name-for-review",
            )
            .with_launch_root("/repo")
            .with_workspace("default")
            .with_tool_version(Tool::Terraform, "1.9.0")
            .with_variable_sources(VariableSources::new(
                vec![PathBuf::from("/repo/environments/production/common.tfvars")],
                vec![PathBuf::from("/repo/secrets/production.tfvars")],
                true,
                std::iter::once("TF_VAR_region".to_owned())
                    .chain((0..32).map(|index| format!("TF_VAR_{index:02}")))
                    .collect(),
            )),
        );
        let state = review_state(plan);
        let area = Rect::new(0, 0, 120, 40);
        let layout = layout(area, false, &state);
        let mut view = PlanReviewViewState::default();
        view.apply_with_matches(
            PlanReviewInput::Down,
            layout.body(),
            layout.max_vertical(),
            layout.max_horizontal(),
            "",
            layout.matches(),
        );
        let position = view.scroll();
        let normal = render_to_buffer((area.width, area.height), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        snapshot("preview_120x40_long-target", &normal);
        for (width, height) in [(80, 24), (160, 60)] {
            let buffer = render_to_buffer((width, height), |frame| {
                render(
                    frame,
                    &state,
                    &PlanReviewViewState::default(),
                    Instant::now(),
                );
            });
            let text = buffer_text(&buffer);
            assert!(text.contains("Target: "), "{width}x{height}\n{text}");
            assert!(text.contains("[PROD]"), "{width}x{height}\n{text}");
            assert!(
                text.contains("Workspace: default"),
                "{width}x{height}\n{text}"
            );
            assert!(
                text.contains("Tool: terraform 1.9.0"),
                "{width}x{height}\n{text}"
            );
            assert!(text.contains("Dir: ./"), "{width}x{height}\n{text}");
            snapshot(&format!("preview_{width}x{height}_long-target"), &buffer);
        }
        view.apply_with_matches(
            PlanReviewInput::OpenHelp,
            layout.body(),
            layout.max_vertical(),
            layout.max_horizontal(),
            "",
            layout.matches(),
        );
        let help = render_to_buffer((area.width, area.height), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let help_text = buffer_text(&help);
        assert!(help_text.contains("Help"));
        assert_eq!(view.scroll(), position);

        view.close_overlay();
        view.apply_with_matches(
            PlanReviewInput::OpenContext,
            layout.body(),
            layout.max_vertical(),
            layout.max_horizontal(),
            "",
            layout.matches(),
        );
        let context = render_to_buffer((area.width, area.height), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let context_text = buffer_text(&context);
        let compact_context = context_text.replace('\n', "");
        assert!(context_text.contains("very-long-target-name-for-review [PROD]"));
        assert!(context_text.contains("Workspace: default"));
        assert!(context_text.contains("terraform 1.9.0"));
        assert!(
            compact_context
                .contains("/repo/environments/production/very-long-target-name-for-review")
        );
        assert!(context_text.contains("Execution directory"));
        assert!(compact_context.contains("/repo/secrets/production.tfvars"));
        assert!(context_text.contains("TF_VAR_region"));
        view.overlay_bottom();
        let scrolled_context = render_to_buffer((80, 24), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        assert!(buffer_text(&scrolled_context).contains("TF_VAR_31"));
        assert_eq!(view.scroll(), position);
    }

    #[test]
    fn renders_plan_help_with_overview_navigation_and_scrollable_sections() {
        let state = review_state(review().with_apply_entry(true));
        let mut view = PlanReviewViewState::default();
        view.apply_with_matches(PlanReviewInput::OpenHelp, Rect::default(), 0, 0, "", &[]);

        for (width, height) in [(40, 16), (40, 24), (80, 24), (120, 40), (160, 60)] {
            let help = render_to_buffer((width, height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let help_text = buffer_text(&help);
            assert!(help_text.contains("Help"), "{width}x{height}: {help_text}");
            assert!(
                !help_text.contains("clear filter"),
                "{width}x{height}: {help_text}"
            );
            assert_eq!(
                help_text.matches("close").count(),
                1,
                "{width}x{height}: {help_text}"
            );
            if width >= 80 {
                assert!(
                    help_text.lines().any(|line| {
                        let words: Vec<_> = line.split_whitespace().collect();
                        words.contains(&"s") && words.contains(&"overview")
                    }),
                    "{width}x{height}: {help_text}"
                );
                assert!(
                    help_text.contains("copy the full plan"),
                    "{width}x{height}: {help_text}"
                );
                assert!(
                    help_text.contains("apply the full plan"),
                    "{width}x{height}: {help_text}"
                );
            }
            if (width, height) == (120, 40) {
                assert!(
                    help.cell((0, 0))
                        .expect("dimmed background")
                        .modifier
                        .contains(Modifier::DIM)
                );
            }
            snapshot(&format!("preview_{width}x{height}_help"), &help);
        }

        view.overlay_bottom();
        let bottom = render_to_buffer((80, 24), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let bottom_text = buffer_text(&bottom);
        assert!(bottom_text.contains("Exit"));
        assert!(bottom_text.contains("quit"));
        assert_eq!(bottom_text.matches("close").count(), 1);
        snapshot("preview_80x24_help_bottom", &bottom);

        let small_bottom = render_to_buffer((40, 16), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let small_bottom_text = buffer_text(&small_bottom);
        assert!(small_bottom_text.contains("Exit"));
        assert!(small_bottom_text.contains("quit"));
        assert_eq!(small_bottom_text.matches("close").count(), 1);
        snapshot("preview_40x16_help_bottom", &small_bottom);
    }

    #[test]
    fn confirmed_filter_help_explains_how_to_clear_the_filter() {
        let mut plan = review();
        plan.set_search_query("worker".to_owned());
        let state = review_state(plan);
        let mut view = PlanReviewViewState::default();
        view.apply_with_matches(PlanReviewInput::OpenHelp, Rect::default(), 0, 0, "", &[]);

        let help = render_to_buffer((120, 40), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let text = buffer_text(&help);

        assert!(text.contains("next or previous match"));
        assert!(text.contains("press Esc again to clear filter"));
    }

    #[test]
    fn renders_apply_help_and_context_with_only_confirmation_actions() {
        let plan = review().with_context(
            ExecutionContext::loading("/repo/environments/production/main")
                .with_launch_root("/repo")
                .with_workspace("default")
                .with_tool_version(Tool::Terraform, "1.9.0"),
        );
        let state = confirmation_state(plan);
        let mut view = ApplyConfirmationViewState::default();
        assert_eq!(view.apply(ApplyConfirmationInput::OpenHelp, "main"), None);
        for (width, height) in [(80, 24), (120, 40), (160, 60)] {
            let help = render_to_buffer((width, height), |frame| {
                render_apply_confirmation(frame, &state, &view);
            });
            let help_text = buffer_text(&help);
            assert!(
                help_text.contains("Apply help"),
                "{width}x{height}: {help_text}"
            );
            assert_eq!(
                help_text.matches("close").count(),
                1,
                "{width}x{height}: {help_text}"
            );
            if width >= 80 {
                assert!(
                    help_text.contains("confirm apply"),
                    "{width}x{height}: {help_text}"
                );
                assert!(
                    help_text.contains("show execution context"),
                    "{width}x{height}: {help_text}"
                );
            }
            snapshot(&format!("apply_confirmation_help_{width}x{height}"), &help);
        }

        view.close_overlay();
        assert_eq!(
            view.apply(ApplyConfirmationInput::OpenContext, "main"),
            None
        );
        let context = render_to_buffer((120, 40), |frame| {
            render_apply_confirmation(frame, &state, &view);
        });
        assert!(buffer_text(&context).contains("Execution directory"));
        assert!(buffer_text(&context).contains("/repo/environments/production/main"));
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
            plan_document_with_blocks(
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
                plan_document_with_blocks(
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
                plan_document_with_blocks(
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
                    ReviewNavigation::Standalone,
                    shell_layout::centered_width(area),
                ),
                shell_layout::centered_width(area),
                None,
            );

            assert!(!footer.is_empty());
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

            assert_eq!(
                layout.shell.content().bottom(),
                layout.shell.footer_separator().y
            );
            assert_eq!(
                layout.shell.footer_separator().bottom(),
                layout.shell.footer().y
            );
            assert_eq!(layout.shell.footer_separator().height, 1);
            assert!(layout.shell.content().height >= 2);
            assert!(buffer_text(&buffer).contains("q quit"));
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
        fn normal_body_keeps_the_final_summary_without_its_trailing_blank() {
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
                ["body", "Plan: 1 to add, 0 to change, 0 to destroy."]
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
                ["Plan: application text", "following body text"]
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
                    address: None,
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

            assert!(buffer_text(&buffer).contains("/terraform_data"));
            assert_text_segment_uses_style(
                &buffer,
                "/terraform_data",
                0,
                1,
                Color::Rgb(0xf4, 0x9e, 0x4c),
                Color::Reset,
                Modifier::empty(),
            );
            assert_text_segment_uses_style(
                &buffer,
                "/terraform_data",
                1,
                SEARCH_TERM.chars().count(),
                Color::Rgb(0xe9, 0xdb, 0xdb),
                Color::Reset,
                Modifier::empty(),
            );
            assert_text_segment_uses_style(
                &buffer,
                "/terraform_data",
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
            assert_eq!(normal + selected, 6);
            let footer = buffer_text(&first);
            assert!(footer.contains("Esc clear"));
            assert!(footer.contains("/ edit"));
            assert!(!footer.contains("a apply"));
            assert!(!footer.contains("y yank"));
            assert!(footer.contains("q quit"));

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
        fn selected_filter_match_does_not_block_manual_scrolling() {
            let mut plan = review();
            plan.set_search_query(SEARCH_TERM.to_owned());
            let state = review_state(plan);
            let area = Rect::new(0, 0, 80, 24);
            let layout = layout(area, false, &state);
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
            view.apply_with_matches(
                PlanReviewInput::Bottom,
                layout.body(),
                layout.max_vertical(),
                layout.max_horizontal(),
                SEARCH_TERM,
                layout.matches(),
            );
            assert!(layout.max_vertical() > 0);
            assert_eq!(view.selected(), Some(0));
            assert_eq!(view.scroll().0, layout.max_vertical());

            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let text = buffer_text(&buffer);

            assert!(text.contains("End of synthetic plan body."), "{text}");
            assert_eq!(search_match_style_counts(&buffer, SEARCH_TERM).1, 0);
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
                        .map(|status| status.0.starts_with(&expected_label)),
                    Some(true),
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
                assert!(text.contains("clear"), "case: {}", case.name);
                assert!(text.contains("/ edit"), "case: {}", case.name);
                assert!(text.contains("y copy all"), "case: {}", case.name);
                assert!(text.contains("? help"), "case: {}", case.name);
                assert!(!text.contains("a apply"), "case: {}", case.name);
                assert!(text.contains("q quit"), "case: {}", case.name);
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
                assert!(text.contains("/ edit"), "query: {query}");
                assert!(text.contains("copy all"), "query: {query}");
                assert!(text.contains("? help"), "query: {query}");
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
            assert!(!input_text.contains("Plan | Filter"));
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
            assert!(!confirmed_text.contains("Plan | Filter"));
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
            assert!(!cleared_text.contains("┌Plan"));
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

            assert!(!text.contains("┌Plan | Filter"));
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
        fn filtered_body_keeps_the_plan_summary() {
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
                lines.iter().any(|line| {
                    line.to_string() == "Plan: 1 to add, 0 to change, 0 to destroy."
                })
            );
        }
    }

    mod confirmation {
        use super::*;

        #[test]
        fn relative_directory_uses_the_launch_root_and_shows_dot_for_the_root() {
            assert_eq!(
                context::relative_directory(Path::new("/repo"), Some(Path::new("/repo"))),
                "."
            );
            assert_eq!(
                context::relative_directory(Path::new("/repo/infra"), Some(Path::new("/repo")),),
                "./infra"
            );
            assert_eq!(
                context::relative_directory(Path::new("/other"), Some(Path::new("/repo"))),
                "/other"
            );
        }

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
                view.apply(ApplyConfirmationInput::Character(character), "yes");
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
            assert!(text.contains("Target: staging"));
            assert!(compact.contains("Directory:"));
            assert!(compact.contains("/repo"));
            assert!(compact.contains("environments/production"));
            assert!(compact.contains("東京"));
            assert!(text.contains("Workspace: staging"));
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
            assert!(lines[usize::from(layout.header().y)].contains("main [PROD]"));
            assert!(!lines[usize::from(layout.header().y)].contains("Terminal too small"));
            assert!(lines[usize::from(layout.notice().y)].contains("Terminal too small"));
        }

        #[test]
        fn production_confirmation_uses_role_styles_for_labels_values_scope_and_warning() {
            let state = confirmation_state(review());
            let buffer = render_to_buffer((120, 40), |frame| {
                render_apply_confirmation(frame, &state, &ApplyConfirmationViewState::default());
            });
            let dialog_y = (buffer.area().y..buffer.area().bottom())
                .find(|&y| {
                    (buffer.area().x..buffer.area().right())
                        .map(|x| buffer.cell((x, y)).expect("dialog cell").symbol())
                        .collect::<String>()
                        .contains("Apply this reviewed plan?")
                })
                .expect("confirmation dialog title");
            assert_text_segment_uses_style_from(
                &buffer,
                dialog_y,
                "Target: main [PROD]",
                0,
                "Target: ".chars().count(),
                (
                    Color::Rgb(0xc0, 0xb8, 0xb8),
                    Color::Reset,
                    Modifier::empty(),
                ),
            );
            assert_text_segment_uses_style_from(
                &buffer,
                dialog_y,
                "Target: main [PROD]",
                "Target: ".chars().count(),
                "main [PROD]".chars().count(),
                (
                    Color::Rgb(0xe9, 0xdb, 0xdb),
                    Color::Reset,
                    Modifier::empty(),
                ),
            );
            assert_text_segment_uses_style_from(
                &buffer,
                dialog_y,
                "Workspace: default",
                0,
                "Workspace: ".chars().count(),
                (
                    Color::Rgb(0xc0, 0xb8, 0xb8),
                    Color::Reset,
                    Modifier::empty(),
                ),
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
                view.apply(ApplyConfirmationInput::Character(character), "yes");
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
                        address: None,
                        position: None,
                        source: DiagnosticSource::Terraform,
                    },
                    Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        summary: "Deprecated configuration".to_owned(),
                        detail: Some("warning detail line 1\nwarning detail line 2".to_owned()),
                        address: None,
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
            assert!(buffer_text(&narrow).contains("Quit? [Enter] quit [Esc] cancel"));
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
            assert!(
                position("Unique targets (replace once): +1 add")
                    < position("Error: Invalid configuration")
            );
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
            assert!(footer_text.contains("s overview"), "{footer_text}");
            assert!(!footer_text.contains("y copy plan"), "{footer_text}");
            assert!(footer_text.contains("q quit"), "{footer_text}");
        }

        #[test]
        fn narrow_normal_footer_keeps_required_actions_and_position_together() {
            let state = review_state(review_with_applyable(false));
            let view = PlanReviewViewState::default();
            let area = Rect::new(0, 0, 24, 24);
            let layout = layout(area, false, &state);
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let footer = layout.shell.footer();
            let footer_lines = (footer.y..footer.bottom())
                .map(|y| {
                    (footer.x..footer.right())
                        .map(|x| {
                            buffer
                                .cell((x, y))
                                .expect("footer cell")
                                .symbol()
                                .to_owned()
                        })
                        .collect::<String>()
                })
                .collect::<Vec<_>>();
            let position = layout
                .footer_status
                .as_ref()
                .expect("plan position")
                .0
                .as_str();

            assert!(footer_lines.iter().any(|line| line.contains("/ filter")));
            assert!(footer_lines.iter().any(|line| line.contains("? help")));
            assert!(
                footer_lines
                    .iter()
                    .any(|line| line.contains("q quit") && line.contains(position)),
                "{footer_lines:?}"
            );
            assert!(!footer_lines.iter().any(|line| line.contains("a apply")));
        }

        #[test]
        fn single_environment_footer_shows_overview_when_it_fits() {
            let wide = footer::layout_with_notice(
                footer_items(
                    false,
                    true,
                    0,
                    false,
                    ReviewNavigation::Standalone,
                    footer::available_width(80, Some("Line 1/43")),
                ),
                80,
                Some("Line 1/43"),
            );
            let wide_text = wide
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.content.as_ref())
                .collect::<String>();

            assert!(wide_text.contains("s overview"), "{wide_text}");
            assert!(wide_text.starts_with("s overview"), "{wide_text}");
            assert!(!wide_text.contains("y copy plan"), "{wide_text}");

            let narrow = footer::layout_with_notice(
                footer_items(
                    false,
                    true,
                    0,
                    false,
                    ReviewNavigation::Standalone,
                    footer::available_width(24, Some("L1/43")),
                ),
                24,
                Some("L1/43"),
            );
            let narrow_text = narrow
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.content.as_ref())
                .collect::<String>();

            assert!(!narrow_text.contains("s overview"), "{narrow_text}");
            assert!(narrow_text.contains("/ filter"), "{narrow_text}");
            assert!(narrow_text.contains("a apply"), "{narrow_text}");
            assert!(narrow_text.contains("? help"), "{narrow_text}");
            assert!(narrow_text.contains("q quit"), "{narrow_text}");
        }

        #[test]
        fn standalone_footer_prioritizes_overview_at_38_columns() {
            let state = review_state(review_with_applyable(true).with_apply_entry(true));
            let view = PlanReviewViewState::default();
            let area = Rect::new(0, 0, 38, 24);
            let layout = layout(area, false, &state);
            let buffer = render_to_buffer((area.width, area.height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let footer = layout.shell.footer();
            let footer_lines = (footer.y..footer.bottom())
                .map(|y| {
                    (footer.x..footer.right())
                        .map(|x| {
                            buffer
                                .cell((x, y))
                                .expect("footer cell")
                                .symbol()
                                .to_owned()
                        })
                        .collect::<String>()
                })
                .collect::<Vec<_>>();
            let position = layout
                .footer_status
                .as_ref()
                .expect("plan position")
                .0
                .as_str();

            assert!(
                footer_lines.iter().any(|line| line.contains("/ filter")),
                "{footer_lines:?}"
            );
            assert!(
                footer_lines.iter().any(|line| line.contains("a apply")),
                "{footer_lines:?}"
            );
            assert!(
                footer_lines.iter().any(|line| line.contains("? help")),
                "{footer_lines:?}"
            );
            assert!(footer_lines[0].contains("s overview"), "{footer_lines:?}");
            assert!(
                footer_lines.iter().any(|line| line.contains("q quit")),
                "{footer_lines:?}"
            );
            assert!(footer_lines.iter().any(|line| line.contains(position)));
            assert!(!footer_lines.iter().any(|line| line.contains("y copy plan")));
        }

        #[test]
        fn environment_help_shows_tab_navigation_at_supported_widths() {
            let sections = plan_help_sections(&review(), ReviewNavigation::Environments, false);
            for size in [(40, 16), (40, 24), (80, 24), (120, 40), (160, 60)] {
                let text = buffer_text(&render_to_buffer(size, |frame| {
                    help_dialog::render(frame, frame.area(), "Help", &sections, 0);
                }));
                let compact = text
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>();

                assert!(compact.contains("Tab"), "{size:?}: {text}");
                assert!(compact.contains("Shift-Tab"), "{size:?}: {text}");
                assert!(compact.contains("next"), "{size:?}: {text}");
                assert!(compact.contains("previous"), "{size:?}: {text}");
                assert!(compact.contains("environment"), "{size:?}: {text}");
            }
        }

        #[test]
        fn position_status_names_the_source_line_at_wide_and_narrow_widths() {
            assert_eq!(position_status(10, 47, 80), "Line 11/47");
            assert_eq!(position_status(10, 47, 40), "L11/47");
        }

        #[test]
        fn plan_entry_footer_hides_apply_even_when_plan_is_applyable() {
            let state = review_state(review_with_apply_allowed(true, false));
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
        }
    }
}
