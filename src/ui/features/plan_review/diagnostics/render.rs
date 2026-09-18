use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::DiagnosticsViewState;
use crate::app::execution::{DiagnosticPosition, DiagnosticSeverity};
use crate::app::review::{PlanListState, ReviewDiagnosticsState};
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{footer, header, layout as shell_layout};
use crate::ui::theme;

const MIN_HEIGHT: u16 = 8;
const MIN_WIDTH: u16 = 48;

pub(crate) struct DiagnosticsLayout {
    shell: shell_layout::ShellLayout,
    body: Rect,
}

impl DiagnosticsLayout {
    pub(crate) const fn body(&self) -> Rect {
        self.body
    }

    pub(crate) const fn footer(&self) -> Rect {
        self.shell.footer()
    }
}

pub(crate) fn diagnostics_layout(area: Rect) -> DiagnosticsLayout {
    let footer_lines = footer::layout(
        vec![
            footer::hint(&["q"], "quit"),
            footer::hint(&["Esc", "w"], "back"),
            footer::hint(&["↑", "↓", "j", "k"], "scroll"),
            footer::hint(&["PgUp", "PgDn"], "page"),
        ],
        area.width,
    );
    let required_footer_lines = footer::layout(
        vec![
            footer::hint(&["q"], "quit"),
            footer::hint(&["Esc", "w"], "back"),
        ],
        area.width,
    );
    let shell = shell_layout::layout(area, footer_lines, required_footer_lines, 1);
    DiagnosticsLayout {
        body: shell.content_inner(),
        shell,
    }
}

pub(crate) fn render_diagnostics(
    frame: &mut Frame<'_>,
    list: &PlanListState,
    diagnostics: &ReviewDiagnosticsState,
    view: DiagnosticsViewState,
) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    let layout = diagnostics_layout(area);
    if layout.body.height == 0 {
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }
    header::render_review(
        frame,
        layout.shell.header(),
        list.context(),
        list.comparison(),
    );
    let content_area = shell_layout::render_content_block(
        frame,
        layout.shell.content(),
        format!("Diagnostics ({})", diagnostics.count()),
    );
    debug_assert_eq!(content_area, layout.shell.content_inner());
    let content = diagnostic_content(diagnostics);
    let scroll = view
        .scroll()
        .min(max_scroll(&content, layout.body.width, layout.body.height));
    frame.render_widget(
        Paragraph::new(content)
            .style(theme::body_style())
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        layout.body,
    );
    footer::render(
        frame,
        layout.footer(),
        layout.shell.footer_lines().to_owned(),
    );
}

pub(super) fn diagnostic_content(state: &ReviewDiagnosticsState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, diagnostic) in state.diagnostics().iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(
            format!("Diagnostic {}/{}", index + 1, state.count()),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled("Severity: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(severity_label(diagnostic.severity)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Summary: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(diagnostic.summary.clone()),
        ]));
        if let Some(detail) = &diagnostic.detail {
            append_multiline(&mut lines, "Detail: ", detail);
        }
        if let Some(position) = &diagnostic.position {
            lines.push(Line::from(format_location(position)));
        }
    }
    lines
}

fn append_multiline(lines: &mut Vec<Line<'static>>, prefix: &str, text: &str) {
    let mut detail_lines = text.lines();
    if let Some(first) = detail_lines.next() {
        lines.push(Line::from(format!("{prefix}{first}")));
    }
    for line in detail_lines {
        lines.push(Line::from(line.to_owned()));
    }
}

fn format_location(position: &DiagnosticPosition) -> String {
    format!(
        "Location: {}:{}:{}-{}:{}",
        position.filename,
        position.start.line,
        position.start.column,
        position.end.line,
        position.end.column,
    )
}

const fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Unknown => "unknown",
    }
}

pub(super) fn max_scroll(content: &[Line<'static>], width: u16, height: u16) -> u16 {
    let lines = wrapped_line_count(content, width.max(1));
    u16::try_from(lines.saturating_sub(usize::from(height.max(1)))).unwrap_or(u16::MAX)
}

fn wrapped_line_count(content: &[Line<'static>], width: u16) -> usize {
    content
        .iter()
        .map(|line| super::super::wrap::wrapped_line_count_for_line(line, width))
        .sum()
}

#[cfg(test)]
mod tests {
    use crate::app::execution::{Diagnostic, DiagnosticPoint, DiagnosticSource};
    use crate::app::plan::{Plan, PlanSummary};
    use crate::app::review::ReviewComparison;
    use crate::ui::test_support::{buffer_text, render_to_buffer};

    use super::*;

    fn diagnostics() -> ReviewDiagnosticsState {
        ReviewDiagnosticsState::new(vec![Diagnostic {
            severity: DiagnosticSeverity::Warning,
            summary: "警告🙂".to_owned(),
            detail: Some("long detail with Unicode 日本語".to_owned()),
            position: Some(DiagnosticPosition {
                filename: "main.tf".to_owned(),
                start: DiagnosticPoint {
                    line: 4,
                    column: 2,
                    byte: None,
                },
                end: DiagnosticPoint {
                    line: 4,
                    column: 8,
                    byte: None,
                },
            }),
            source: DiagnosticSource::Terraform,
        }])
    }

    #[test]
    fn renders_structured_fields() {
        let state = diagnostics();
        let text = buffer_text(&render_to_buffer((80, 20), |frame| {
            let list = empty_list();
            render_diagnostics(frame, &list, &state, DiagnosticsViewState::default());
        }));

        assert!(text.contains("Diagnostics (1)"), "{text}");
        assert!(text.contains("warning"), "{text}");
        assert!(
            text.contains('警') && text.contains('告') && text.contains('🙂'),
            "{text}"
        );
        assert!(text.contains("main.tf:4:2-4:8"), "{text}");
    }

    #[test]
    fn narrow_panel_prioritizes_terminal_notice() {
        let state = diagnostics();
        let text = buffer_text(&render_to_buffer((47, 7), |frame| {
            let list = empty_list();
            render_diagnostics(frame, &list, &state, DiagnosticsViewState::default());
        }));

        assert!(text.contains("Terminal too small"), "{text}");
    }

    #[test]
    fn layout_exposes_a_shared_body_rect() {
        let layout = diagnostics_layout(Rect::new(0, 0, 80, 20));

        assert!(layout.body().height > 0);
    }

    #[test]
    fn fitting_trailing_space_does_not_create_a_diagnostics_scroll_offset() {
        assert_eq!(max_scroll(&[Line::from("12345 ")], 5, 1), 0);
    }

    fn empty_list() -> PlanListState {
        PlanListState::from_plan(
            Plan {
                changes: Vec::new(),
                summary: PlanSummary::default(),
                unsupported_changes: Vec::new(),
            },
            Vec::new(),
            ReviewComparison::working_tree(),
        )
        .expect("empty plan should create a list state")
    }
}
