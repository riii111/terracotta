use super::EnvironmentView;
use crate::{
    app::environments::{EnvironmentSession, EnvironmentState},
    ui::{
        features::{overview::matrix, plan_review},
        shell::environments,
        theme,
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Clear, Paragraph, Wrap},
};
use std::time::Instant;

impl EnvironmentView {
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, state: &EnvironmentSession) {
        self.sync(state);
        let area = frame.area();
        let layout = environments::layout(area, state, self.notice.as_deref());
        environments::render_tabs(frame, layout.tabs, state, &self.selection);
        frame.render_widget(
            Paragraph::new(environments::summary(state))
                .wrap(Wrap { trim: false })
                .style(theme::secondary_style()),
            layout.summary,
        );
        if let Some(notice) = &self.notice {
            frame.render_widget(
                Paragraph::new(notice.as_str())
                    .wrap(Wrap { trim: false })
                    .style(theme::warning_style()),
                layout.notice,
            );
        }
        if let Some(index) = self.selection.raw
            && let Some(review) = state.plans()[index].review()
        {
            plan_review::render_environment(
                frame,
                layout.body,
                review,
                &self.reviews[index],
                Instant::now(),
            );
        } else {
            self.render_overview(frame, layout.body, state);
        }
        if self.confirming_quit {
            self.render_dialog(
                frame,
                "Stop acquiring environment plans?\nEnter stop and quit   Esc continue",
            );
        } else if let Some(text) = &self.dialog {
            self.render_dialog(frame, text);
        }
    }

    fn render_overview(&mut self, frame: &mut Frame<'_>, area: Rect, state: &EnvironmentSession) {
        let Some(plan) = state.plans().get(self.selection.column) else {
            return;
        };
        let selected = format!(
            "{}: {}",
            environments::name(plan),
            environments::status(plan)
        );
        let status = if self.matrix.searching() || self.matrix.filtered() {
            format!("Filter: /{}   (display only)", self.matrix.filter())
        } else {
            selected
        };
        frame.render_widget(
            Paragraph::new(status),
            Rect::new(area.x, area.y, area.width, area.height.min(1)),
        );
        let detail = if matches!(plan.state(), EnvironmentState::Error) {
            plan.diagnostic().text().to_owned()
        } else if let Some(review) = plan
            .review()
            .filter(|review| review.review().metadata().nonstandard_changes() > 0)
        {
            format!(
                "Other changes: {} output/import/move or unsupported change(s). v opens the full plan.",
                review.review().metadata().nonstandard_changes()
            )
        } else {
            String::new()
        };
        let detail_widget = Paragraph::new(detail).wrap(Wrap { trim: false });
        let detail_height = u16::try_from(detail_widget.line_count(area.width.max(1)))
            .unwrap_or(u16::MAX)
            .min(3)
            .min(area.height.saturating_sub(7));
        frame.render_widget(
            detail_widget.style(theme::warning_style()),
            Rect::new(area.x, area.y.saturating_add(1), area.width, detail_height),
        );
        let body = Rect::new(
            area.x,
            area.y.saturating_add(1 + detail_height),
            area.width,
            area.height.saturating_sub(2 + detail_height),
        );
        matrix::render(frame, body, state, &mut self.matrix, self.selection.column);
        let footer = if self.matrix.searching() {
            "Enter confirm   Esc cancel"
        } else if area.width < 60 {
            "Enter open  / filter  ? help  q quit"
        } else {
            "Enter open diff   / filter   Space expand   ? help   q quit"
        };
        frame.render_widget(
            Paragraph::new(footer),
            Rect::new(
                area.x,
                area.bottom().saturating_sub(1),
                area.width,
                area.height.min(1),
            ),
        );
    }

    fn render_dialog(&self, frame: &mut Frame<'_>, text: &str) {
        let area = frame.area();
        let widget = Paragraph::new(text).wrap(Wrap { trim: false });
        let max = widget
            .line_count(area.width.max(1))
            .saturating_sub(usize::from(area.height));
        frame.render_widget(Clear, area);
        frame.render_widget(
            widget.scroll((
                self.dialog_scroll
                    .min(u16::try_from(max).unwrap_or(u16::MAX)),
                0,
            )),
            area,
        );
    }
}
