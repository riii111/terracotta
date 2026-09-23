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
        let visible_environments = self.visible_environments(state.plans().len());
        let show_boundaries =
            self.selection.raw.is_none() && self.has_room_for_boundaries(area, state);
        let layout = environments::layout(
            area,
            state,
            self.notice.as_deref(),
            self.selected_environments.is_some(),
            show_boundaries,
        );
        environments::render_tabs(
            frame,
            layout.tabs,
            state,
            &self.selection,
            &visible_environments,
        );
        frame.render_widget(
            Paragraph::new(environments::summary(
                state,
                self.selected_environments.is_some(),
            ))
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
                &mut self.reviews[index],
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
        } else if let Some(dialog) = &self.filter_dialog {
            dialog.render(frame, frame.area(), state.plans());
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
        let footer = overview_footer(
            area.width,
            self.matrix.searching(),
            self.matrix.selected_group_expanded(),
        );
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
        let shell = environments::layout(
            area,
            state,
            self.notice.as_deref(),
            self.selected_environments.is_some(),
            false,
        );
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

fn overview_footer(width: u16, searching: bool, expanded: Option<bool>) -> String {
    if searching {
        return "Enter confirm   Esc cancel".to_owned();
    }
    let toggle = expanded.map(|expanded| {
        if expanded {
            "Space collapse"
        } else {
            "Space expand"
        }
    });
    let filter_hint = "e env filter";
    match (width, toggle) {
        (..45, Some(toggle)) => format!("Enter open  {toggle}  e filter"),
        (..45, None) => "Enter open  e env filter  q quit".to_owned(),
        (45..56, Some(toggle)) => format!("Enter open  {toggle}  e filter  ? help"),
        (45..56, None) => "Enter open selected  e env filter  ? help".to_owned(),
        (56..64, Some(toggle)) => format!("Enter open  / filter  {toggle}  e filter"),
        (56..64, None) => "Enter open selected  / filter  e filter".to_owned(),
        (64..80, Some(toggle)) => {
            format!("Enter open selected  / filter  {toggle}  {filter_hint}  ? help")
        }
        (64..80, None) => {
            format!("Enter open selected raw plan  / filter  {filter_hint}  ? help")
        }
        (_, Some(toggle)) => {
            format!("Enter open selected  / filter  {toggle}  {filter_hint}  ? help  q quit")
        }
        (_, None) => {
            format!("Enter open selected raw plan  / filter  {filter_hint}  ? help  q quit")
        }
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
                "Current: Overview",
                vec![
                    help_dialog::HelpAction::new("↑ / ↓ / j / k", "select a resource row"),
                    help_dialog::HelpAction::new("← / → / [ / ]", "select an environment"),
                    help_dialog::HelpAction::new("Enter", "open selected resource in raw plan"),
                    help_dialog::HelpAction::new(
                        "1–9",
                        "open selected resource in the numbered environment",
                    ),
                    help_dialog::HelpAction::new(
                        "Space",
                        "expand or collapse only on [+]/[-] group rows",
                    ),
                    help_dialog::HelpAction::new("/", "filter full addresses"),
                    help_dialog::HelpAction::new("e", "filter compared environments"),
                ],
            ),
            help_dialog::HelpSection::new(
                "Other",
                vec![
                    help_dialog::HelpAction::new("PgUp / PgDn", "move one page"),
                    help_dialog::HelpAction::new("Home / End", "go to the first or last row"),
                    help_dialog::HelpAction::new("v", "show the full plan from the top"),
                    help_dialog::HelpAction::new("y", "copy the selected environment's plan"),
                    help_dialog::HelpAction::new("c", "show environment context"),
                    help_dialog::HelpAction::new("r", "retry a selected Error environment"),
                    help_dialog::HelpAction::new("q", "quit; confirms first while acquiring"),
                ],
            ),
            help_dialog::HelpSection::new(
                "Matrix legend",
                vec![
                    help_dialog::HelpAction::new(
                        "Same changes",
                        "no difference detected among Ready plans; unknown values may differ",
                    ),
                    help_dialog::HelpAction::new("+ / ~ / -", "create / update / delete"),
                    help_dialog::HelpAction::new(
                        "+/- / -/+",
                        "replace (create→delete / delete→create)",
                    ),
                    help_dialog::HelpAction::new("blank", "resource absent from this environment"),
                    help_dialog::HelpAction::new(".", "resource present, with no change"),
                    help_dialog::HelpAction::new(
                        "?",
                        "plan unavailable; action unknown or unsupported",
                    ),
                    help_dialog::HelpAction::new("read / move / import", "action shown by name"),
                    help_dialog::HelpAction::new(
                        "why: missing",
                        "resource present in only some Ready plans",
                    ),
                    help_dialog::HelpAction::note("Only Ready environments are compared."),
                    help_dialog::HelpAction::note("Excluded environments are not retried."),
                ],
            ),
        ],
        scroll,
    );
}
