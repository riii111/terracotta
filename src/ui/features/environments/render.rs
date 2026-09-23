use super::{EnvironmentDialog, EnvironmentView};
use crate::{
    app::environments::{EnvironmentPlan, EnvironmentSession, EnvironmentState},
    ui::{
        features::{overview::matrix, plan_review},
        primitives::{atoms::separator, molecules::help_dialog},
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
        let show_boundaries =
            self.selection.raw.is_none() && self.has_room_for_boundaries(area, state);
        let layout = environments::layout(area, state, self.notice.as_deref(), show_boundaries);
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
        frame.render_widget(
            separator::render(layout.header_separator.width),
            layout.header_separator,
        );
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
            self.render_overview(frame, layout.body, state, show_boundaries);
        }
        if self.confirming_quit {
            self.render_dialog(
                frame,
                "Stop acquiring environment plans?\nEnter stop and quit   Esc continue",
            );
        } else if let Some(dialog) = &self.dialog {
            match dialog {
                EnvironmentDialog::Help => {
                    render_help_dialog(frame, frame.area(), self.dialog_scroll);
                }
                EnvironmentDialog::Message(text) => self.render_dialog(frame, text),
            }
        }
    }

    fn render_overview(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        state: &EnvironmentSession,
        show_boundaries: bool,
    ) {
        let Some(plan) = state.plans().get(self.selection.column) else {
            return;
        };
        let context = overview_context(self, plan);
        let detail = overview_detail(plan);
        let (context_height, detail_height) = section_heights(area, &context, &detail);
        frame.render_widget(
            Paragraph::new(context).wrap(Wrap { trim: false }),
            Rect::new(area.x, area.y, area.width, context_height),
        );
        frame.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: false })
                .style(theme::warning_style()),
            Rect::new(
                area.x,
                area.y.saturating_add(context_height),
                area.width,
                detail_height,
            ),
        );
        let body = Rect::new(
            area.x,
            area.y.saturating_add(context_height + detail_height),
            area.width,
            area.height
                .saturating_sub(context_height + 1 + detail_height + u16::from(show_boundaries)),
        );
        matrix::render(frame, body, state, &mut self.matrix, self.selection.column);
        let footer = if self.matrix.searching() {
            "Enter confirm   Esc cancel"
        } else if area.width < 60 {
            "Enter open  / filter  ? help  q quit"
        } else {
            "Enter open diff   / filter   Space expand   ? help   q quit"
        };
        if show_boundaries {
            frame.render_widget(
                separator::render(area.width),
                Rect::new(area.x, area.bottom().saturating_sub(2), area.width, 1),
            );
        }
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

    fn has_room_for_boundaries(&self, area: Rect, state: &EnvironmentSession) -> bool {
        let Some(plan) = state.plans().get(self.selection.column) else {
            return false;
        };
        let shell = environments::layout(area, state, self.notice.as_deref(), false);
        let context = overview_context(self, plan);
        let detail = overview_detail(plan);
        let (context_height, detail_height) = section_heights(shell.body, &context, &detail);
        let matrix_height = shell
            .body
            .height
            .saturating_sub(context_height + 1 + detail_height);

        matrix_height >= 9
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

fn overview_context(view: &EnvironmentView, plan: &EnvironmentPlan) -> String {
    let selection = format!(
        "{} · {}",
        environments::name(plan),
        plan.tool.display_name()
    );
    if view.matrix.searching() || view.matrix.filtered() {
        format!(
            "Filter: /{}   (display only)\n{selection}",
            view.matrix.filter()
        )
    } else {
        selection
    }
}

fn overview_detail(plan: &EnvironmentPlan) -> String {
    if matches!(plan.state(), EnvironmentState::Error) {
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
    }
}

fn section_heights(area: Rect, context: &str, detail: &str) -> (u16, u16) {
    let context_height = u16::try_from(
        Paragraph::new(context)
            .wrap(Wrap { trim: false })
            .line_count(area.width.max(1)),
    )
    .unwrap_or(u16::MAX)
    .min(area.height.saturating_sub(6));
    let detail_height = u16::try_from(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .line_count(area.width.max(1)),
    )
    .unwrap_or(u16::MAX)
    .min(3)
    .min(area.height.saturating_sub(context_height + 6));

    (context_height, detail_height)
}

fn render_help_dialog(frame: &mut Frame<'_>, area: Rect, scroll: u16) {
    help_dialog::render(
        frame,
        area,
        "Help",
        &[
            help_dialog::HelpSection::new(
                "Navigation",
                vec![
                    help_dialog::HelpAction::new("↑ / ↓ / j / k", "select a resource row"),
                    help_dialog::HelpAction::new("← / → / [ / ]", "select an environment"),
                    help_dialog::HelpAction::new("PgUp / PgDn", "move one page"),
                    help_dialog::HelpAction::new("Home / End", "go to the first or last row"),
                ],
            ),
            help_dialog::HelpSection::new(
                "Review",
                vec![
                    help_dialog::HelpAction::new("Enter", "open the selected resource"),
                    help_dialog::HelpAction::new("1–9", "open the resource in that environment"),
                    help_dialog::HelpAction::new("Space", "expand or collapse a group"),
                    help_dialog::HelpAction::new("/", "filter full addresses"),
                    help_dialog::HelpAction::new("v", "show the full plan from the top"),
                ],
            ),
            help_dialog::HelpSection::new(
                "Actions",
                vec![
                    help_dialog::HelpAction::new("y", "copy the selected environment's plan"),
                    help_dialog::HelpAction::new("c", "show environment context"),
                    help_dialog::HelpAction::new("r", "retry a selected Error environment"),
                ],
            ),
            help_dialog::HelpSection::new(
                "Comparison",
                vec![
                    help_dialog::HelpAction::note("Same changes compare patterns."),
                    help_dialog::HelpAction::note("blank: resource not in this environment."),
                    help_dialog::HelpAction::note(".: resource present, with no change."),
                    help_dialog::HelpAction::note("?: environment plan not fetched yet."),
                    help_dialog::HelpAction::note(
                        "why: missing = present in only some environments.",
                    ),
                    help_dialog::HelpAction::note("Unknown values remain unknown."),
                    help_dialog::HelpAction::note("Only Ready environments are compared."),
                    help_dialog::HelpAction::note("Excluded environments are not retried."),
                ],
            ),
            help_dialog::HelpSection::new(
                "Exit",
                vec![help_dialog::HelpAction::new(
                    "q",
                    "quit; confirms first while acquiring",
                )],
            ),
        ],
        scroll,
    );
}
