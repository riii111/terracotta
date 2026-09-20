use std::time::Instant;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::app::{
    execution::DiagnosticSeverity,
    review::{FilteredPlan, PlanReview},
    session::{ApplyConfirmationState, ReviewSessionState},
};
use crate::ui::primitives::{atoms::scrollbar, molecules::terminal_notice};
use crate::ui::shell::{footer, header, layout as shell_layout};
use crate::ui::theme;

use super::{ApplyConfirmationViewState, PlanReviewViewState};

const MIN_WIDTH: u16 = 24;
const MIN_HEIGHT: u16 = 6;
const FLASH_BACKGROUND: Color = Color::Rgb(0xf4, 0x9e, 0x4c);
const FLASH_FOREGROUND: Color = Color::Rgb(0x11, 0x14, 0x19);

struct PreparedContent<'a> {
    lines: Vec<Line<'a>>,
    max_width: usize,
}

pub(crate) struct PlanReviewLayout {
    shell: shell_layout::ShellLayout,
    body: Rect,
    search: Option<Rect>,
    vertical_scrollbar: bool,
    horizontal_scrollbar: bool,
    max_vertical: u16,
    max_horizontal: u16,
}

impl PlanReviewLayout {
    pub(crate) const fn body(&self) -> Rect {
        self.body
    }

    pub(crate) const fn search(&self) -> Option<Rect> {
        self.search
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

pub(crate) fn layout(area: Rect, searching: bool, state: &ReviewSessionState) -> PlanReviewLayout {
    let content = prepare_content(state);
    layout_with_content(area, searching, state, &content)
}

fn layout_with_content(
    area: Rect,
    searching: bool,
    state: &ReviewSessionState,
    content: &PreparedContent<'_>,
) -> PlanReviewLayout {
    let panel = shell_layout::centered_area(area);
    let footer_lines = footer::layout(
        footer_items(searching, state.review().metadata().applyable()),
        panel.width,
    );
    let required = footer::layout(
        vec![
            footer::hint(&["↑", "↓"], "scroll"),
            footer::hint(&["q"], "quit"),
        ],
        panel.width,
    );
    let shell = shell_layout::layout(panel, footer_lines, required, 1);
    let inner = shell.content_inner();
    let search = searching.then(|| Rect::new(inner.x, inner.y, inner.width, 1));
    let search_height = u16::from(searching);
    let available = Rect::new(
        inner.x,
        inner.y.saturating_add(search_height),
        inner.width,
        inner.height.saturating_sub(search_height),
    );
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
    let (max_vertical, max_horizontal) = limits(content.lines.len(), content.max_width, body);
    PlanReviewLayout {
        shell,
        body,
        search,
        vertical_scrollbar,
        horizontal_scrollbar,
        max_vertical,
        max_horizontal,
    }
}

pub(crate) fn render_apply_confirmation(
    frame: &mut Frame<'_>,
    state: &ApplyConfirmationState,
    view: &ApplyConfirmationViewState,
) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        terminal_notice::render_wrapped(
            frame,
            area,
            "Terminal too small. Resize or press Esc to go back.",
        );
        return;
    }
    let panel = shell_layout::centered_area(area);
    let footer_lines = footer::layout(
        vec![
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "back"),
        ],
        panel.width,
    );
    let shell = shell_layout::layout(panel, footer_lines.clone(), footer_lines, 1);
    header::render_review(frame, shell.header(), state.review());
    let inner = shell_layout::render_content_block(frame, shell.content(), "Apply");
    let metadata = state.review().metadata();
    let mut lines = vec![
        Line::from("Apply this reviewed plan?"),
        Line::from(format!("Target: {}", state.review().root().display())),
        Line::from(format!("Workspace: {}", state.review().workspace())),
        Line::from(format!(
            "Plan: {} to add, {} to change, {} to destroy.",
            metadata.additions(),
            metadata.changes(),
            metadata.deletions()
        )),
        Line::default(),
    ];
    if metadata.deletions() > 0 {
        lines.push(Line::from("This plan includes resource deletion."));
        lines.push(Line::default());
    }
    let cursor = view.cursor().min(view.input().len());
    let before = view.input()[..cursor].to_owned();
    let after = view.input()[cursor..].to_owned();
    lines.push(Line::from(vec![
        Span::raw("Apply this plan? (yes/no): "),
        Span::styled(before, search_input_style()),
        Span::styled("|", search_input_style()),
        Span::styled(after, search_input_style()),
    ]));
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .wrap(Wrap { trim: false }),
        inner,
    );
    footer::render(frame, shell.footer(), shell.footer_lines().to_owned());
}

pub(crate) fn render(
    frame: &mut Frame<'_>,
    state: &ReviewSessionState,
    view: &PlanReviewViewState,
    now: Instant,
) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        terminal_notice::render_wrapped(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    let content = prepare_content(state);
    let layout = layout_with_content(area, view.searching(), state, &content);
    if layout.body().width == 0 || layout.body().height == 0 {
        terminal_notice::render_wrapped(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }
    header::render_review(frame, layout.shell.header(), state.review());
    let inner = shell_layout::render_content_block(
        frame,
        layout.shell.content(),
        if view.searching() {
            "Plan | Search".to_owned()
        } else if state.review().search_query().is_empty() {
            "Plan".to_owned()
        } else {
            format!("Plan | Search: {}", state.review().search_query())
        },
    );
    debug_assert_eq!(inner, layout.shell.content_inner());

    if let Some(search_area) = layout.search()
        && let Some((line, horizontal)) = search_prompt(view, search_area.width)
    {
        frame.render_widget(
            Paragraph::new(line)
                .style(theme::body_style())
                .scroll((0, horizontal)),
            search_area,
        );
    }

    let line_count = content.lines.len();
    let max_line_width = content.max_width;
    let max_vertical = layout.max_vertical();
    let max_horizontal = layout.max_horizontal();
    let (vertical, horizontal) = view.scroll();
    let vertical = vertical.min(max_vertical);
    let horizontal = horizontal.min(max_horizontal);
    let lines = if state.copy_flash_active(now) {
        flash_lines(content.lines)
    } else {
        content.lines
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .scroll((vertical, horizontal)),
        layout.body(),
    );
    if let Some(notice) = state.copy_notice() {
        frame.render_widget(
            Paragraph::new(notice.message()).style(theme::secondary_style()),
            Rect::new(layout.body().x, layout.body().y, layout.body().width, 1),
        );
    }
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
        layout.shell.footer_lines().to_owned(),
    );
}

fn prepare_content(state: &ReviewSessionState) -> PreparedContent<'_> {
    let review = state.review();
    let filtered = review.filtered_document();
    let lines = review_lines(review, &filtered);
    let max_width = max_line_width(&lines);
    PreparedContent { lines, max_width }
}

fn review_lines<'a>(review: &'a PlanReview, filtered: &FilteredPlan<'a>) -> Vec<Line<'a>> {
    let mut lines = diagnostic_lines(review);
    if filtered.matching_blocks() == 0 && !review.search_query().is_empty() {
        lines.push(Line::from(Span::styled(
            "No matches.",
            theme::warning_style(),
        )));
        lines.push(Line::default());
    }
    lines.extend(visible_plan_lines(review, filtered));
    lines
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

fn plan_line<'a>(line: &'a str, query: &str) -> Line<'a> {
    if query.is_empty() {
        return Line::from(Span::styled(line, theme::plan_line_style(line)));
    }
    let mut result = Line::default();
    let mut rest = line;
    while let Some(index) = rest.find(query) {
        let (before, matched_and_after) = rest.split_at(index);
        if !before.is_empty() {
            result.push_span(Span::styled(before, theme::plan_line_style(line)));
        }
        let (matched, after) = matched_and_after.split_at(query.len());
        result.push_span(Span::styled(matched, search_match_style()));
        rest = after;
    }
    if !rest.is_empty() {
        result.push_span(Span::styled(rest, theme::plan_line_style(line)));
    }
    result
}

fn flash_lines(lines: Vec<Line<'_>>) -> Vec<Line<'static>> {
    let style = Style::default().fg(FLASH_FOREGROUND).bg(FLASH_BACKGROUND);
    lines
        .into_iter()
        .map(|line| Line::from(Span::styled(line.to_string(), style)))
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

fn visible_plan_lines<'a>(review: &'a PlanReview, filtered: &FilteredPlan<'a>) -> Vec<Line<'a>> {
    let query = review.search_query();
    let mut lines = Vec::new();
    for &line in filtered.lines() {
        if !query.is_empty() && line.starts_with("Plan:") {
            lines.push(Line::from(Span::styled(
                "Plan total (full plan):",
                theme::warning_style(),
            )));
        }
        lines.push(plan_line(line, query));
    }
    lines
}

fn max_line_width(lines: &[Line<'_>]) -> usize {
    lines.iter().map(Line::width).max().unwrap_or(0)
}

fn search_prompt(view: &PlanReviewViewState, width: u16) -> Option<(Line<'static>, u16)> {
    let query = view.search_query()?;
    let cursor = view.search_cursor()?;
    let before = query[..cursor].to_owned();
    let after = query[cursor..].to_owned();
    let line = Line::from(vec![
        Span::styled("/", theme::footer_key_style()),
        Span::styled(before.clone(), search_input_style()),
        Span::styled("|", search_input_style()),
        Span::styled(after, search_input_style()),
    ]);
    let cursor = 1 + Line::from(before).width();
    let horizontal = u16::try_from(
        cursor
            .saturating_sub(usize::from(width.saturating_sub(1)))
            .min(line.width().saturating_sub(usize::from(width))),
    )
    .unwrap_or(u16::MAX);
    Some((line, horizontal))
}

fn footer_items(searching: bool, applyable: bool) -> Vec<Line<'static>> {
    if searching {
        vec![
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "cancel"),
            footer::hint(&["Ctrl-A", "Ctrl-E"], "move"),
        ]
    } else {
        let mut items = vec![
            footer::hint(&["↑", "↓", "←", "→"], "scroll"),
            footer::hint(&["/"], "search"),
            footer::hint(&["y"], "yank"),
        ];
        if applyable {
            items.push(footer::hint(&["a"], "apply"));
        }
        items.push(footer::hint(&["q"], "quit"));
        items
    }
}

fn search_input_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0x88, 0xc0, 0xd0))
        .add_modifier(Modifier::BOLD)
}

fn search_match_style() -> Style {
    Style::default()
        .fg(FLASH_FOREGROUND)
        .bg(FLASH_BACKGROUND)
        .add_modifier(Modifier::BOLD)
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

    use ratatui::buffer::Buffer;

    use crate::app::{
        execution::{ExecutionContext, ExecutionState},
        review::{
            PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata, test_support::plan_document,
        },
        session::{self, Action, SessionState},
    };
    use crate::ui::{
        features::plan_review::PlanReviewInput,
        test_support::{
            assert_shell_frame_and_footer, buffer_text, render_to_buffer, write_buffer_captures,
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

    fn review() -> PlanReview {
        PlanReview::new(
            PathBuf::from("/repo/environments/production/main"),
            "default".to_owned(),
            PlanDocument::with_blocks(
                PLAN_TEXT.to_owned(),
                vec![
                    PlanBlock::new(0..2, PlanBlockKind::Common),
                    PlanBlock::new(
                        2..8,
                        PlanBlockKind::Resource("terraform_data.api".to_owned()),
                    ),
                    PlanBlock::new(8..9, PlanBlockKind::Common),
                    PlanBlock::new(
                        9..15,
                        PlanBlockKind::Resource("terraform_data.worker".to_owned()),
                    ),
                    PlanBlock::new(15..16, PlanBlockKind::Common),
                    PlanBlock::new(
                        16..20,
                        PlanBlockKind::Resource("terraform_data.old".to_owned()),
                    ),
                    PlanBlock::new(20..21, PlanBlockKind::Common),
                    PlanBlock::new(
                        21..26,
                        PlanBlockKind::Resource("terraform_data.new".to_owned()),
                    ),
                    PlanBlock::new(26..43, PlanBlockKind::Common),
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
                true,
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

    fn assert_text_prefix_uses_style(
        buffer: &Buffer,
        text: &str,
        styled_prefix: &str,
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
            for offset in 0..styled_prefix.chars().count() {
                let cell = buffer
                    .cell((
                        area.x + u16::try_from(start + offset).expect("search offset"),
                        y,
                    ))
                    .expect("search cell");
                assert_eq!(cell.fg, foreground, "{styled_prefix}");
                assert_eq!(cell.bg, background, "{styled_prefix}");
                assert!(cell.modifier.contains(modifier), "{styled_prefix}");
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
    fn renders_plan_review_search_at_all_supported_sizes() {
        for &(width, height) in &SIZES {
            let mut plan = review();
            plan.set_search_query(SEARCH_TERM.to_owned());
            let state = review_state(plan);
            let mut view = PlanReviewViewState::default();
            view.apply(
                PlanReviewInput::SearchStart,
                Rect::new(0, 0, width, height),
                0,
                0,
                SEARCH_TERM,
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
        assert!(text.contains("Terraform will perform the following actions:"));
        assert!(layout.vertical_scrollbar());
        assert!(layout.horizontal_scrollbar());
        let body = layout.body();
        let vertical_x = body.x.saturating_add(body.width);
        let horizontal_y = body.y.saturating_add(body.height);
        let horizontal_end_x = vertical_x;
        assert_eq!(buffer[(vertical_x, body.y)].symbol(), "↑");
        assert_eq!(
            buffer[(vertical_x, body.y)].fg,
            Color::Rgb(0x50, 0x52, 0x5e)
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
        assert_text_color(
            &buffer,
            "~ resource \"terraform_data\" \"api\"",
            Color::Rgb(0xeb, 0xcb, 0x8b),
        );
        assert_text_color(&buffer, "- old_checksum", Color::Rgb(0xbf, 0x61, 0x6a));
        assert_text_color(&buffer, "+ new_checksum", Color::Rgb(0xa3, 0xbe, 0x8c));
    }

    #[test]
    fn production_search_render_draws_search_input_and_match_color() {
        let mut plan = review();
        plan.set_search_query(SEARCH_TERM.to_owned());
        let state = review_state(plan);
        let mut view = PlanReviewViewState::default();
        view.apply(
            PlanReviewInput::SearchStart,
            Rect::new(0, 0, 80, 24),
            0,
            0,
            SEARCH_TERM,
        );
        let buffer = render_to_buffer((80, 24), |frame| {
            render(frame, &state, &view, Instant::now());
        });

        assert!(buffer_text(&buffer).contains("/terraform_data|"));
        assert_text_prefix_uses_style(
            &buffer,
            "terraform_data.api",
            SEARCH_TERM,
            Color::Rgb(0x11, 0x14, 0x19),
            Color::Rgb(0xf4, 0x9e, 0x4c),
            Modifier::BOLD,
        );
    }

    #[test]
    fn production_confirmation_render_draws_deletion_warning_and_footer() {
        let state = confirmation_state(review());
        let view = ApplyConfirmationViewState::default();
        let area = Rect::new(0, 0, 80, 24);
        let buffer = render_to_buffer((area.width, area.height), |frame| {
            render_apply_confirmation(frame, &state, &view);
        });

        assert!(buffer_text(&buffer).contains("This plan includes resource deletion."));
        assert!(buffer_text(&buffer).contains("Enter confirm"));
    }

    #[test]
    fn search_prompt_keeps_the_cursor_visible() {
        let mut view = PlanReviewViewState::default();
        let body = Rect::new(0, 0, 10, 10);
        view.apply(PlanReviewInput::SearchStart, body, 0, 0, "");
        for character in "abcdefgh".chars() {
            view.apply(PlanReviewInput::SearchChar(character), body, 0, 0, "");
        }
        let Some((line, horizontal)) = search_prompt(&view, 6) else {
            panic!("search prompt should be visible");
        };
        assert_eq!(line.to_string(), "/abcdefgh|");
        assert_eq!(horizontal, 4);
    }

    #[test]
    fn search_labels_the_plan_total_as_unfiltered() {
        let mut review = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document("Plan: 1 to add, 0 to change, 0 to destroy.\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 1, 0, 0, true),
            Vec::new(),
        );
        review.set_search_query("api".to_owned());

        let filtered = review.filtered_document();
        let lines = visible_plan_lines(&review, &filtered);
        assert_eq!(lines[0].to_string(), "Plan total (full plan):");
        assert_eq!(
            lines[1].to_string(),
            "Plan: 1 to add, 0 to change, 0 to destroy."
        );
    }
}
