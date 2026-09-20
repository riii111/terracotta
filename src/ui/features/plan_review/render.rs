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
    review::PlanReview,
    session::{ApplyConfirmationState, ReviewSessionState},
};
use crate::ui::primitives::{atoms::scrollbar, molecules::terminal_notice};
use crate::ui::shell::{footer, header, layout as shell_layout};
use crate::ui::theme;

use super::PlanReviewInput;

const MIN_WIDTH: u16 = 24;
const MIN_HEIGHT: u16 = 6;
const FLASH_BACKGROUND: Color = Color::Rgb(0xf4, 0x9e, 0x4c);
const FLASH_FOREGROUND: Color = Color::Rgb(0x11, 0x14, 0x19);

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchInputState {
    query: String,
    cursor: usize,
    previous_query: String,
    previous_vertical: u16,
    previous_horizontal: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PlanReviewViewState {
    vertical: u16,
    horizontal: u16,
    search: Option<SearchInputState>,
}

impl PlanReviewViewState {
    pub(crate) fn apply(
        &mut self,
        input: PlanReviewInput,
        body: Rect,
        state: &ReviewSessionState,
    ) -> Option<String> {
        let (max_vertical, max_horizontal) = limits(body, state);
        self.vertical = self.vertical.min(max_vertical);
        self.horizontal = self.horizontal.min(max_horizontal);
        if self.search.is_some() {
            return self.apply_search_input(input);
        }

        match input {
            PlanReviewInput::SearchStart => {
                let query = state.review().search_query().to_owned();
                self.search = Some(SearchInputState {
                    cursor: query.len(),
                    previous_query: query.clone(),
                    query,
                    previous_vertical: self.vertical,
                    previous_horizontal: self.horizontal,
                });
                None
            }
            PlanReviewInput::Up => self.scroll_vertical(-1, body, state),
            PlanReviewInput::Down => self.scroll_vertical(1, body, state),
            PlanReviewInput::Left => self.scroll_horizontal(-1, body, state),
            PlanReviewInput::Right => self.scroll_horizontal(1, body, state),
            PlanReviewInput::PageUp => {
                self.vertical = self.vertical.saturating_sub(body.height.max(1));
                None
            }
            PlanReviewInput::PageDown => {
                let (max_vertical, _) = limits(body, state);
                self.vertical = self
                    .vertical
                    .saturating_add(body.height.max(1))
                    .min(max_vertical);
                None
            }
            PlanReviewInput::Top => {
                self.vertical = 0;
                None
            }
            PlanReviewInput::Bottom => {
                self.vertical = limits(body, state).0;
                None
            }
            PlanReviewInput::LeftEdge => {
                self.horizontal = 0;
                None
            }
            PlanReviewInput::RightEdge => {
                self.horizontal = limits(body, state).1;
                None
            }
            PlanReviewInput::SearchChar(_)
            | PlanReviewInput::SearchBackspace
            | PlanReviewInput::SearchLeft
            | PlanReviewInput::SearchRight
            | PlanReviewInput::SearchHome
            | PlanReviewInput::SearchEnd
            | PlanReviewInput::SearchConfirm
            | PlanReviewInput::SearchCancel
            | PlanReviewInput::Apply
            | PlanReviewInput::Copy
            | PlanReviewInput::Quit => None,
        }
    }

    pub(crate) const fn searching(&self) -> bool {
        self.search.is_some()
    }

    #[cfg(test)]
    pub(crate) fn search_input(&self) -> Option<&str> {
        self.search.as_ref().map(|search| search.query.as_str())
    }

    pub(crate) const fn scroll(&self) -> (u16, u16) {
        (self.vertical, self.horizontal)
    }

    fn apply_search_input(&mut self, input: PlanReviewInput) -> Option<String> {
        let search = self.search.as_mut()?;
        match input {
            PlanReviewInput::SearchChar(character) => {
                search.query.insert(search.cursor, character);
                search.cursor += character.len_utf8();
                self.vertical = 0;
                self.horizontal = 0;
                Some(search.query.clone())
            }
            PlanReviewInput::SearchBackspace => {
                if search.cursor > 0 {
                    let previous = search.query[..search.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(index, _)| index);
                    search.query.drain(previous..search.cursor);
                    search.cursor = previous;
                    self.vertical = 0;
                    self.horizontal = 0;
                }
                Some(search.query.clone())
            }
            PlanReviewInput::SearchLeft => {
                search.cursor = search.query[..search.cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(index, _)| index);
                None
            }
            PlanReviewInput::SearchRight => {
                search.cursor = search.query[search.cursor..]
                    .char_indices()
                    .nth(1)
                    .map_or(search.query.len(), |(index, _)| search.cursor + index);
                None
            }
            PlanReviewInput::SearchHome => {
                search.cursor = 0;
                None
            }
            PlanReviewInput::SearchEnd => {
                search.cursor = search.query.len();
                None
            }
            PlanReviewInput::SearchConfirm => {
                self.search = None;
                None
            }
            PlanReviewInput::SearchCancel => {
                let (previous_query, previous_vertical, previous_horizontal) = (
                    search.previous_query.clone(),
                    search.previous_vertical,
                    search.previous_horizontal,
                );
                self.vertical = previous_vertical;
                self.horizontal = previous_horizontal;
                self.search = None;
                Some(previous_query)
            }
            _ => None,
        }
    }

    fn scroll_vertical(
        &mut self,
        delta: i16,
        body: Rect,
        state: &ReviewSessionState,
    ) -> Option<String> {
        let max = limits(body, state).0;
        self.vertical = if delta.is_negative() {
            self.vertical.saturating_sub(delta.unsigned_abs())
        } else {
            self.vertical.saturating_add(delta.unsigned_abs()).min(max)
        };
        None
    }

    fn scroll_horizontal(
        &mut self,
        delta: i16,
        body: Rect,
        state: &ReviewSessionState,
    ) -> Option<String> {
        let max = limits(body, state).1;
        self.horizontal = if delta.is_negative() {
            self.horizontal.saturating_sub(delta.unsigned_abs())
        } else {
            self.horizontal
                .saturating_add(delta.unsigned_abs())
                .min(max)
        };
        None
    }
}

pub(crate) struct PlanReviewLayout {
    shell: shell_layout::ShellLayout,
    body: Rect,
    search: Option<Rect>,
    vertical_scrollbar: bool,
    horizontal_scrollbar: bool,
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
}

pub(crate) fn layout(area: Rect, searching: bool, state: &ReviewSessionState) -> PlanReviewLayout {
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
        scrollbar_reservations(&review_lines_for_limits(state), available);
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
    PlanReviewLayout {
        shell,
        body,
        search,
        vertical_scrollbar,
        horizontal_scrollbar,
    }
}

pub(crate) fn render_apply_confirmation(
    frame: &mut Frame<'_>,
    state: &ApplyConfirmationState,
    input: &str,
    cursor: usize,
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
    let cursor = cursor.min(input.len());
    let before = input[..cursor].to_owned();
    let after = input[cursor..].to_owned();
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

    let layout = layout(area, view.searching(), state);
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

    let lines = review_lines(state);
    let (max_vertical, max_horizontal) = limits(layout.body(), state);
    let (vertical, horizontal) = view.scroll();
    let vertical = vertical.min(max_vertical);
    let horizontal = horizontal.min(max_horizontal);
    let lines = if state.copy_flash_active(now) {
        flash_lines(lines)
    } else {
        lines
    };
    frame.render_widget(
        Paragraph::new(lines.clone())
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
            lines.len(),
            usize::from(body.height),
            usize::from(vertical),
        );
    }
    if layout.horizontal_scrollbar() {
        scrollbar::render_horizontal(
            frame,
            scrollbar_area,
            max_line_width(&lines),
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

fn review_lines(state: &ReviewSessionState) -> Vec<Line<'static>> {
    let review = state.review();
    let mut lines = diagnostic_lines(review);
    if review.matching_block_count() == 0 && !review.search_query().is_empty() {
        lines.push(Line::from(Span::styled(
            "No matches.",
            theme::warning_style(),
        )));
        lines.push(Line::default());
    }
    lines.extend(visible_plan_lines(review));
    lines
}

fn diagnostic_lines(review: &PlanReview) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for diagnostic in review.diagnostics() {
        let style = match diagnostic.severity {
            DiagnosticSeverity::Error => theme::error_style(),
            _ => theme::warning_style(),
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{}: {}",
                severity_label(diagnostic.severity),
                diagnostic.summary
            ),
            style,
        )));
        if let Some(detail) = &diagnostic.detail {
            lines.extend(detail.lines().map(|line| Line::from(line.to_owned())));
        }
    }
    if !lines.is_empty() && !review.document().text().is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn plan_line(line: &str, query: &str) -> Line<'static> {
    if query.is_empty() {
        return Line::from(Span::styled(line.to_owned(), theme::plan_line_style(line)));
    }
    let mut result = Line::default();
    let mut rest = line;
    while let Some(index) = rest.find(query) {
        let (before, matched_and_after) = rest.split_at(index);
        if !before.is_empty() {
            result.push_span(Span::styled(
                before.to_owned(),
                theme::plan_line_style(line),
            ));
        }
        let (matched, after) = matched_and_after.split_at(query.len());
        result.push_span(Span::styled(matched.to_owned(), search_match_style()));
        rest = after;
    }
    if !rest.is_empty() {
        result.push_span(Span::styled(rest.to_owned(), theme::plan_line_style(line)));
    }
    result
}

fn flash_lines(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let style = Style::default().fg(FLASH_FOREGROUND).bg(FLASH_BACKGROUND);
    lines
        .into_iter()
        .map(|line| Line::from(Span::styled(line.to_string(), style)))
        .collect()
}

fn limits(body: Rect, state: &ReviewSessionState) -> (u16, u16) {
    let lines = review_lines_for_limits(state);
    let max_vertical =
        u16::try_from(lines.len().saturating_sub(usize::from(body.height))).unwrap_or(u16::MAX);
    let max_horizontal =
        u16::try_from(max_line_width(&lines).saturating_sub(usize::from(body.width)))
            .unwrap_or(u16::MAX);
    (max_vertical, max_horizontal)
}

fn scrollbar_reservations(lines: &[Line<'static>], area: Rect) -> (bool, bool) {
    let mut vertical = false;
    let mut horizontal = false;
    let line_count = lines.len();
    let line_width = max_line_width(lines);
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

fn review_lines_for_limits(state: &ReviewSessionState) -> Vec<Line<'static>> {
    let mut lines = diagnostic_lines(state.review());
    if state.review().matching_block_count() == 0 && !state.review().search_query().is_empty() {
        lines.push(Line::from("No matches."));
        lines.push(Line::default());
    }
    lines.extend(visible_plan_lines(state.review()));
    lines
}

fn visible_plan_lines(review: &PlanReview) -> Vec<Line<'static>> {
    let query = review.search_query();
    let mut lines = Vec::new();
    for line in review.visible_document_lines() {
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

fn max_line_width(lines: &[Line<'static>]) -> usize {
    lines.iter().map(Line::width).max().unwrap_or(0)
}

fn search_prompt(view: &PlanReviewViewState, width: u16) -> Option<(Line<'static>, u16)> {
    let search = view.search.as_ref()?;
    let before = search.query[..search.cursor].to_owned();
    let after = search.query[search.cursor..].to_owned();
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

    use crate::app::{
        execution::{ExecutionContext, ExecutionState},
        review::{PlanDocument, PlanMetadata},
        session::{Action, SessionState, update},
    };

    use super::*;

    fn review() -> ReviewSessionState {
        let now = Instant::now();
        let mut session = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(
            &mut session,
            Action::ReviewCompleted(PlanReview::new(
                PathBuf::from("/project"),
                "default".to_owned(),
                PlanDocument::new("first\nterraform_data.api = secret\nlast\n".to_owned()),
                PlanMetadata::new(Vec::new(), Vec::new(), 0, 1, 0, true),
                Vec::new(),
            )),
            now,
        );
        let SessionState::Review(review) = session else {
            panic!("review should be visible");
        };
        *review
    }

    #[test]
    fn search_input_changes_query_and_resets_scroll() {
        let state = review();
        let mut view = PlanReviewViewState::default();
        let body = Rect::new(0, 0, 40, 10);
        view.apply(PlanReviewInput::SearchStart, body, &state);
        assert_eq!(
            view.apply(PlanReviewInput::SearchChar('a'), body, &state),
            Some("a".to_owned())
        );
        assert_eq!(view.search_input(), Some("a"));
        assert_eq!(view.scroll(), (0, 0));
    }

    #[test]
    fn escape_restores_query_and_position() {
        let state = review();
        let mut view = PlanReviewViewState::default();
        let body = Rect::new(0, 0, 40, 10);
        view.apply(PlanReviewInput::SearchStart, body, &state);
        view.apply(PlanReviewInput::SearchChar('a'), body, &state);
        assert_eq!(
            view.apply(PlanReviewInput::SearchCancel, body, &state),
            Some(String::new())
        );
        assert!(!view.searching());
    }

    #[test]
    fn search_key_input_stays_in_the_editor() {
        assert_eq!(
            super::super::input::key_to_input(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('j'),
                    crossterm::event::KeyModifiers::NONE,
                ),
                true,
            ),
            Some(PlanReviewInput::SearchChar('j'))
        );
    }

    #[test]
    fn search_right_moves_to_the_next_character_boundary() {
        let state = review();
        let mut view = PlanReviewViewState::default();
        let body = Rect::new(0, 0, 40, 10);
        view.apply(PlanReviewInput::SearchStart, body, &state);
        view.apply(PlanReviewInput::SearchChar('a'), body, &state);
        view.apply(PlanReviewInput::SearchChar('b'), body, &state);
        view.apply(PlanReviewInput::SearchChar('c'), body, &state);
        view.apply(PlanReviewInput::SearchHome, body, &state);
        view.apply(PlanReviewInput::SearchRight, body, &state);
        view.apply(PlanReviewInput::SearchChar('X'), body, &state);
        assert_eq!(view.search_input(), Some("aXbc"));

        view.apply(PlanReviewInput::SearchHome, body, &state);
        view.apply(PlanReviewInput::SearchRight, body, &state);
        view.apply(PlanReviewInput::SearchChar('あ'), body, &state);
        assert_eq!(view.search_input(), Some("aあXbc"));
    }

    #[test]
    fn search_prompt_keeps_the_cursor_visible() {
        let state = review();
        let mut view = PlanReviewViewState::default();
        let body = Rect::new(0, 0, 10, 10);
        view.apply(PlanReviewInput::SearchStart, body, &state);
        for character in "abcdefgh".chars() {
            view.apply(PlanReviewInput::SearchChar(character), body, &state);
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
            PlanDocument::new("Plan: 1 to add, 0 to change, 0 to destroy.\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 1, 0, 0, true),
            Vec::new(),
        );
        review.set_search_query("api".to_owned());

        let lines = visible_plan_lines(&review);
        assert_eq!(lines[0].to_string(), "Plan total (full plan):");
        assert_eq!(
            lines[1].to_string(),
            "Plan: 1 to add, 0 to change, 0 to destroy."
        );
    }
}
