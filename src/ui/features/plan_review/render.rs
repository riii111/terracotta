use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{execution::DiagnosticSeverity, session::ReviewSessionState};
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{footer, header, layout as shell_layout};
use crate::ui::theme;

use super::PlanReviewInput;

const MIN_WIDTH: u16 = 24;
const MIN_HEIGHT: u16 = 6;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PlanReviewViewState {
    vertical: u16,
    horizontal: u16,
}

impl PlanReviewViewState {
    pub(crate) fn apply(&mut self, input: PlanReviewInput, body: Rect, state: &ReviewSessionState) {
        let (max_vertical, max_horizontal) = limits(body, state);
        match input {
            PlanReviewInput::Up => self.vertical = self.vertical.saturating_sub(1),
            PlanReviewInput::Down => {
                self.vertical = self.vertical.saturating_add(1).min(max_vertical);
            }
            PlanReviewInput::Left => self.horizontal = self.horizontal.saturating_sub(1),
            PlanReviewInput::Right => {
                self.horizontal = self.horizontal.saturating_add(1).min(max_horizontal);
            }
            PlanReviewInput::PageUp => {
                self.vertical = self.vertical.saturating_sub(body.height.max(1));
            }
            PlanReviewInput::PageDown => {
                self.vertical = self
                    .vertical
                    .saturating_add(body.height.max(1))
                    .min(max_vertical);
            }
            PlanReviewInput::Top => self.vertical = 0,
            PlanReviewInput::Bottom => self.vertical = max_vertical,
            PlanReviewInput::LeftEdge => self.horizontal = 0,
            PlanReviewInput::RightEdge => self.horizontal = max_horizontal,
            PlanReviewInput::Copy | PlanReviewInput::Quit => {}
        }
    }
}

pub(crate) fn render(frame: &mut Frame<'_>, state: &ReviewSessionState, view: PlanReviewViewState) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        terminal_notice::render_wrapped(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    let footer_lines = footer::layout(
        vec![
            footer::hint(&["↑", "↓", "←", "→"], "scroll"),
            footer::hint(&["y"], "yank"),
            footer::hint(&["q"], "quit"),
        ],
        area.width,
    );
    let required = footer::layout(
        vec![
            footer::hint(&["↑", "↓"], "scroll"),
            footer::hint(&["q"], "quit"),
        ],
        area.width,
    );
    let shell = shell_layout::layout(area, footer_lines, required, 1);
    header::render_review(frame, shell.header(), state.review());
    let body = shell_layout::render_content_block(frame, shell.content(), "Plan");

    let lines = review_lines(state);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .scroll((view.vertical, view.horizontal)),
        body,
    );
    if let Some(notice) = state.copy_notice() {
        let notice_area = Rect::new(body.x, body.y, body.width, body.height.min(1));
        frame.render_widget(
            Paragraph::new(notice.message()).style(theme::secondary_style()),
            notice_area,
        );
    }
    footer::render(frame, shell.footer(), shell.footer_lines().to_owned());
}

fn review_lines(state: &ReviewSessionState) -> Vec<Line<'static>> {
    let review = state.review();
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
    if !review.diagnostics().is_empty() && !review.document().text().is_empty() {
        lines.push(Line::default());
    }
    lines.extend(
        review
            .document()
            .text()
            .lines()
            .map(|line| Line::from(Span::styled(line.to_owned(), theme::plan_line_style(line)))),
    );
    if review.document().text().ends_with('\n') {
        lines.push(Line::default());
    }
    lines
}

fn limits(body: Rect, state: &ReviewSessionState) -> (u16, u16) {
    let lines = review_lines(state);
    let max_vertical =
        u16::try_from(lines.len().saturating_sub(usize::from(body.height))).unwrap_or(u16::MAX);
    let width = lines.iter().map(Line::width).max().unwrap_or(0);
    let max_horizontal =
        u16::try_from(width.saturating_sub(usize::from(body.width))).unwrap_or(u16::MAX);
    (max_vertical, max_horizontal)
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
        review::{PlanDocument, PlanMetadata, PlanReview},
        session::{Action, SessionState, update},
    };

    use super::*;

    #[test]
    fn full_text_keeps_terraform_order_and_change_markers() {
        let now = std::time::Instant::now();
        let mut session = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(
            &mut session,
            Action::ReviewCompleted(PlanReview::new(
                PathBuf::from("/project"),
                "default".to_owned(),
                PlanDocument::new("first\n~ change\nlast\n".to_owned()),
                PlanMetadata::new(Vec::new(), Vec::new(), 0, 1, 0, true),
                Vec::new(),
            )),
            now,
        );
        let SessionState::Review(review) = session else {
            panic!("review should be visible");
        };

        assert_eq!(
            review_lines(&review)
                .iter()
                .map(Line::to_string)
                .collect::<Vec<_>>(),
            vec!["first", "~ change", "last", ""]
        );
    }
}
