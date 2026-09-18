use ratatui::layout::Rect;

use crate::app::review::ReviewDiagnosticsState;

mod input;
mod render;

pub(crate) use input::{
    DiagnosticsInput, DiagnosticsScroll, key_to_input as key_to_diagnostics_input,
};
pub(crate) use render::{diagnostics_layout, render_diagnostics};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DiagnosticsViewState {
    scroll: u16,
}

impl DiagnosticsViewState {
    pub(crate) const fn scroll(self) -> u16 {
        self.scroll
    }

    pub(crate) const fn reset(&mut self) {
        self.scroll = 0;
    }
}

pub(crate) fn apply_diagnostics_scroll(
    view: &mut DiagnosticsViewState,
    scroll: DiagnosticsScroll,
    diagnostics: &ReviewDiagnosticsState,
    area: Rect,
) {
    let layout = diagnostics_layout(area);
    let content = render::diagnostic_content(diagnostics);
    let max = render::max_scroll(&content, layout.body().width, layout.body().height);
    let page = layout.body().height.max(1);
    view.scroll = match scroll {
        DiagnosticsScroll::Up => view.scroll.saturating_sub(1),
        DiagnosticsScroll::Down => view.scroll.saturating_add(1).min(max),
        DiagnosticsScroll::PageUp => view.scroll.saturating_sub(page),
        DiagnosticsScroll::PageDown => view.scroll.saturating_add(page).min(max),
    };
}

pub(crate) fn clamp_diagnostics_scroll(
    view: &mut DiagnosticsViewState,
    diagnostics: &ReviewDiagnosticsState,
    area: Rect,
) {
    let layout = diagnostics_layout(area);
    let content = render::diagnostic_content(diagnostics);
    view.scroll = view.scroll.min(render::max_scroll(
        &content,
        layout.body().width,
        layout.body().height,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::execution::{Diagnostic, DiagnosticSeverity, DiagnosticSource};

    fn diagnostics() -> ReviewDiagnosticsState {
        ReviewDiagnosticsState::new(vec![Diagnostic {
            severity: DiagnosticSeverity::Warning,
            summary: "warning".to_owned(),
            detail: Some("detail".to_owned()),
            position: None,
            source: DiagnosticSource::Terraform,
        }])
    }

    #[test]
    fn opening_starts_at_the_first_line_and_resize_clamps_offset() {
        let diagnostics = diagnostics();
        let mut view = DiagnosticsViewState { scroll: 100 };

        clamp_diagnostics_scroll(&mut view, &diagnostics, Rect::new(0, 0, 80, 12));

        assert!(view.scroll() < 100);
        view.reset();
        assert_eq!(view.scroll(), 0);
    }

    #[test]
    fn page_scroll_uses_the_panel_body_and_stops_at_the_last_line() {
        let diagnostics = ReviewDiagnosticsState::new(vec![Diagnostic {
            severity: DiagnosticSeverity::Warning,
            summary: "warning".to_owned(),
            detail: Some("long detail ".repeat(100)),
            position: None,
            source: DiagnosticSource::Terraform,
        }]);
        let area = Rect::new(0, 0, 60, 12);
        let mut view = DiagnosticsViewState::default();

        apply_diagnostics_scroll(&mut view, DiagnosticsScroll::PageDown, &diagnostics, area);
        assert!(view.scroll() > 0);
        let last = view.scroll();
        apply_diagnostics_scroll(&mut view, DiagnosticsScroll::PageDown, &diagnostics, area);
        assert!(view.scroll() >= last);
        clamp_diagnostics_scroll(&mut view, &diagnostics, area);
    }
}
