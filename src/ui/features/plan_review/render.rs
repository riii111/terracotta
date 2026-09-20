use std::time::Instant;

use ratatui::{
    Frame,
    layout::Rect,
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
struct PreparedContent<'a> {
    lines: Vec<Line<'a>>,
    max_width: usize,
}

pub(crate) struct PlanReviewLayout {
    shell: shell_layout::ShellLayout,
    body: Rect,
    search: Option<Rect>,
    filter_details: Option<Rect>,
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

    pub(crate) const fn filter_details(&self) -> Option<Rect> {
        self.filter_details
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
    let filter_visible = filter_active(searching, state);
    let search = filter_visible.then(|| Rect::new(inner.x, inner.y, inner.width, 1));
    let filter_details_height = if filter_visible {
        u16::try_from(filter_details_lines(state, inner.width).len()).unwrap_or(u16::MAX)
    } else {
        0
    };
    let filter_details = filter_visible.then(|| {
        Rect::new(
            inner.x,
            inner.y.saturating_add(1),
            inner.width,
            filter_details_height,
        )
    });
    let filter_height = u16::from(filter_visible).saturating_add(filter_details_height);
    let available = Rect::new(
        inner.x,
        inner.y.saturating_add(filter_height),
        inner.width,
        inner.height.saturating_sub(filter_height),
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
        filter_details,
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
        Span::styled(before, theme::body_style()),
        Span::styled("|", theme::accent_style()),
        Span::styled(after, theme::body_style()),
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
    let title = if filter_active(view.searching(), state) {
        Line::from("Plan | Filter")
    } else {
        Line::from("Plan")
    };
    let inner = shell_layout::render_content_block_line(frame, layout.shell.content(), title);
    debug_assert_eq!(inner, layout.shell.content_inner());

    render_filter_header(frame, &layout, state, view);

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

fn render_filter_header(
    frame: &mut Frame<'_>,
    layout: &PlanReviewLayout,
    state: &ReviewSessionState,
    view: &PlanReviewViewState,
) {
    if !filter_active(view.searching(), state) {
        return;
    }
    if let Some(search_area) = layout.search()
        && let Some((line, horizontal)) =
            filter_prompt(view, state, view.searching(), search_area.width)
    {
        frame.render_widget(
            Paragraph::new(line)
                .style(theme::body_style())
                .scroll((0, horizontal)),
            search_area,
        );
    }
    if let Some(filter_details) = layout.filter_details() {
        frame.render_widget(
            Paragraph::new(filter_details_lines(state, filter_details.width))
                .style(theme::secondary_style()),
            filter_details,
        );
    }
}

fn review_lines<'a>(review: &'a PlanReview, filtered: &FilteredPlan<'a>) -> Vec<Line<'a>> {
    let mut lines = diagnostic_lines(review);
    if filtered.matching_resources() == 0
        && filtered.matching_outputs() == 0
        && !review.search_query().is_empty()
    {
        lines.push(Line::from(Span::styled(
            "No matching resources or outputs.",
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
        result.push_span(Span::styled(matched, theme::search_match_style()));
        rest = after;
    }
    if !rest.is_empty() {
        result.push_span(Span::styled(rest, theme::plan_line_style(line)));
    }
    result
}

fn flash_lines(lines: Vec<Line<'_>>) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| Line::from(Span::styled(line.to_string(), theme::copy_flash_style())))
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
                theme::secondary_style(),
            )));
        }
        lines.push(plan_line(line, query));
    }
    lines
}

fn max_line_width(lines: &[Line<'_>]) -> usize {
    lines.iter().map(Line::width).max().unwrap_or(0)
}

fn filter_active(searching: bool, state: &ReviewSessionState) -> bool {
    searching || !state.review().search_query().is_empty()
}

fn filter_details_lines(state: &ReviewSessionState, width: u16) -> Vec<Line<'static>> {
    let filtered = state.review().filtered_document();
    let matches = format!(
        "Filter matches: resources {}/{} | outputs {}/{}",
        filtered.matching_resources(),
        filtered.resource_count(),
        filtered.matching_outputs(),
        filtered.output_count(),
    );
    let mut lines = wrap_filter_line(&matches, width);
    lines.extend(wrap_filter_line("Scope: full plan (apply / yank)", width));
    lines
}

fn wrap_filter_line(text: &str, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if word.chars().count() > width {
            if !current.is_empty() {
                chunks.push(current);
                current = String::new();
            }
            chunks.extend(
                word.chars()
                    .collect::<Vec<_>>()
                    .chunks(width)
                    .map(|chunk| chunk.iter().collect()),
            );
        } else if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            chunks.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
        .into_iter()
        .map(|chunk| Line::from(Span::styled(chunk, theme::secondary_style())))
        .collect()
}

fn filter_prompt(
    view: &PlanReviewViewState,
    state: &ReviewSessionState,
    searching: bool,
    width: u16,
) -> Option<(Line<'static>, u16)> {
    if searching {
        return search_prompt(view, width);
    }
    let query = state.review().search_query();
    if query.is_empty() {
        return None;
    }
    Some((
        Line::from(vec![
            Span::styled("/", theme::secondary_style()),
            Span::styled(query.to_owned(), theme::secondary_style()),
        ]),
        0,
    ))
}

fn search_prompt(view: &PlanReviewViewState, width: u16) -> Option<(Line<'static>, u16)> {
    let query = view.search_query()?;
    let cursor = view.search_cursor()?;
    let before = query[..cursor].to_owned();
    let after = query[cursor..].to_owned();
    let line = Line::from(vec![
        Span::styled("/", theme::accent_style()),
        Span::styled(before.clone(), theme::body_style()),
        Span::styled("|", theme::accent_style()),
        Span::styled(after, theme::body_style()),
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
            footer::hint(&["/"], "filter"),
            footer::hint(&["y"], "yank"),
        ];
        if applyable {
            items.push(footer::hint(&["a"], "apply"));
        }
        items.push(footer::hint(&["q"], "quit"));
        items
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

    use ratatui::{
        buffer::Buffer,
        style::{Color, Modifier},
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
        features::plan_review::{ApplyConfirmationInput, PlanReviewInput},
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

    fn review() -> PlanReview {
        review_with_applyable(true)
    }

    fn review_with_applyable(applyable: bool) -> PlanReview {
        PlanReview::new(
            PathBuf::from("/repo/environments/production/main"),
            "default".to_owned(),
            PlanDocument::with_blocks(
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

    fn diagnostic_review() -> PlanReview {
        PlanReview::new(
            PathBuf::from("/repo/environments/production/main"),
            "default".to_owned(),
            plan_document("Plan: 1 to add, 0 to change, 0 to destroy.\n".to_owned()),
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

    fn zero_match_review() -> PlanReview {
        PlanReview::new(
            PathBuf::from("/repo/environments/production/main"),
            "default".to_owned(),
            PlanDocument::with_blocks(
                "Warning: synthetic diagnostic\nCommon context stays visible\n  # terraform_data.api will be created\n  + resource \"terraform_data\" \"api\" {\n  + endpoint = (known after apply)\nPlan: 1 to add, 0 to change, 0 to destroy.\n"
                    .to_owned(),
                vec![
                    PlanBlock::new(0..2, PlanBlockKind::Common),
                    PlanBlock::new(2..4, PlanBlockKind::Resource),
                    PlanBlock::new(4..5, PlanBlockKind::Output),
                    PlanBlock::new(5..7, PlanBlockKind::Common),
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
            "Terraform will perform the following actions:",
            Color::Rgb(0xe9, 0xdb, 0xdb),
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
        let available = layout(area, false, &state).shell.content_inner();
        let vertical_state = review_state(review_with_content(
            available.height.saturating_add(1),
            available.width.saturating_sub(1),
        ));
        let (vertical_layout, vertical_buffer) = review_buffer_at(area, &vertical_state, 1, 0);
        assert_eq!(vertical_layout.max_vertical(), 1);
        assert!(!vertical_layout.horizontal_scrollbar());
        assert_scrollbar_positions(&vertical_buffer, &vertical_layout, 1, 0);

        let horizontal_state = review_state(review_with_content(
            available.height.saturating_sub(1),
            available.width.saturating_add(1),
        ));
        let (horizontal_layout, horizontal_buffer) =
            review_buffer_at(area, &horizontal_state, 0, 1);
        assert_eq!(horizontal_layout.max_horizontal(), 1);
        assert!(!horizontal_layout.vertical_scrollbar());
        assert_scrollbar_positions(&horizontal_buffer, &horizontal_layout, 0, 1);
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
        assert_text_segment_uses_style(
            &buffer,
            "/terraform_data|",
            0,
            1,
            Color::Rgb(0xf4, 0x9e, 0x4c),
            Color::Reset,
            Modifier::empty(),
        );
        assert_text_segment_uses_style(
            &buffer,
            "/terraform_data|",
            1,
            SEARCH_TERM.chars().count(),
            Color::Rgb(0xe9, 0xdb, 0xdb),
            Color::Reset,
            Modifier::empty(),
        );
        assert_text_segment_uses_style(
            &buffer,
            "/terraform_data|",
            1 + SEARCH_TERM.chars().count(),
            1,
            Color::Rgb(0xf4, 0x9e, 0x4c),
            Color::Reset,
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
            "Filter matches: resources 4/4 | outputs 0/2",
            "Filter matches: resources 4/4 | outputs 0/2",
            Color::Rgb(0xc0, 0xb8, 0xb8),
            Color::Reset,
            Modifier::empty(),
        );
        let capture = buffer_terminal_capture(&buffer);
        assert!(capture.contains("\x1b[48;2;244;158;76m"));
        assert!(capture.contains("\x1b[48;2;244;158;76m\x1b[1m"));
    }

    #[test]
    fn production_filter_states_show_fixed_scope_and_kind_counts() {
        let mut input_view = PlanReviewViewState::default();
        input_view.apply(
            PlanReviewInput::SearchStart,
            Rect::new(0, 0, 120, 40),
            0,
            0,
            "",
        );
        let input_buffer = render_to_buffer((120, 40), |frame| {
            render(frame, &review_state(review()), &input_view, Instant::now());
        });
        let input_text = buffer_text(&input_buffer);
        write_buffer_captures("ux02-filter-input", &input_buffer);
        assert!(input_text.contains("Plan | Filter"));
        assert!(input_text.contains("/|"));
        assert!(input_text.contains("Filter matches: resources 4/4 | outputs 2/2"));
        assert!(input_text.contains("Scope: full plan (apply / yank)"));

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
        assert!(confirmed_text.contains("/worker"));
        assert!(confirmed_text.contains("Filter matches: resources 1/4 | outputs 0/2"));
        assert!(confirmed_text.contains("Scope: full plan (apply / yank)"));
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
        assert!(!cleared_text.contains("Filter matches:"));
        assert!(!cleared_text.contains("Scope: full plan"));
    }

    #[test]
    fn production_filter_keeps_common_content_and_reports_zero_matches() {
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

        assert!(text.contains("No matching resources or outputs."));
        assert!(text.contains("Warning: Synthetic diagnostic"));
        assert!(text.contains("Common context stays visible"));
        assert!(text.contains("Plan total (full plan):"));
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
    fn production_filter_layout_wraps_details_and_keeps_scope_at_bottom() {
        let mut plan = review();
        plan.set_search_query("worker".to_owned());
        let state = review_state(plan);
        let area = Rect::new(0, 0, 40, 20);
        let layout = layout(area, false, &state);
        let search = layout.search().expect("filter query should be visible");
        let details = layout
            .filter_details()
            .expect("filter details should be visible");
        assert!(details.height > 2);
        assert_eq!(details.y, search.y + search.height);
        assert_eq!(layout.body().y, details.y + details.height);
        assert!(layout.body().height > 0);

        let mut view = PlanReviewViewState::default();
        for _ in 0..layout.max_vertical() {
            view.apply(
                PlanReviewInput::Down,
                layout.body(),
                layout.max_vertical(),
                layout.max_horizontal(),
                "worker",
            );
        }
        let buffer = render_to_buffer((area.width, area.height), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let text = buffer_text(&buffer);
        write_buffer_captures("ux02-filter-narrow", &buffer);
        assert!(text.contains("Filter matches:"));
        assert!(text.contains("Scope: full plan"));

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
    fn production_search_uses_support_style_for_full_plan_total() {
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
            "Plan total (full plan):",
            "Plan total (full plan):",
            Color::Rgb(0xc0, 0xb8, 0xb8),
            Color::Reset,
            Modifier::empty(),
        );
    }

    #[test]
    fn copy_flash_styles_plan_cells_without_changing_the_review_shell() {
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
        assert_area_unchanged(&before, &flash, layout.shell.footer());
        assert_frame_unchanged(&before, &flash, layout.shell.content());
        assert_area_unchanged(
            &before,
            &flash,
            layout.search().expect("search input should be visible"),
        );
        assert_area_unchanged(
            &before,
            &flash,
            layout
                .filter_details()
                .expect("filter details should be visible"),
        );
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
        view.apply(
            PlanReviewInput::Down,
            area,
            scroll_layout.max_vertical(),
            scroll_layout.max_horizontal(),
            SEARCH_TERM,
        );
        view.apply(
            PlanReviewInput::Right,
            area,
            scroll_layout.max_vertical(),
            scroll_layout.max_horizontal(),
            SEARCH_TERM,
        );
        view.apply(
            PlanReviewInput::SearchStart,
            area,
            scroll_layout.max_vertical(),
            scroll_layout.max_horizontal(),
            SEARCH_TERM,
        );
        assert_eq!(view.scroll(), (1, 1));

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
        assert!(position("error detail line 2") < position("Warning: Deprecated configuration"));
        assert!(position("Warning: Deprecated configuration") < position("warning detail line 1"));
        assert!(position("warning detail line 2") < position("Plan: 1 to add"));
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
        assert!(footer_text.contains("y yank"), "{footer_text}");
        assert!(footer_text.contains("q quit"), "{footer_text}");
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

    fn review_buffer_at(
        area: Rect,
        state: &ReviewSessionState,
        vertical: u16,
        horizontal: u16,
    ) -> (PlanReviewLayout, Buffer) {
        let layout = layout(area, false, state);
        let mut view = PlanReviewViewState::default();
        for _ in 0..vertical {
            view.apply(
                PlanReviewInput::Down,
                layout.body(),
                layout.max_vertical(),
                layout.max_horizontal(),
                "",
            );
        }
        for _ in 0..horizontal {
            view.apply(
                PlanReviewInput::Right,
                layout.body(),
                layout.max_vertical(),
                layout.max_horizontal(),
                "",
            );
        }
        let buffer = render_to_buffer((area.width, area.height), |frame| {
            render(frame, state, &view, Instant::now());
        });
        (layout, buffer)
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
}
