use super::{CellPreview, EnvironmentDialog, EnvironmentView};
use crate::{
    app::environments::{EnvironmentPlan, EnvironmentSession, EnvironmentState},
    ui::{
        features::{overview::matrix, plan_review},
        primitives::{atoms::separator, molecules::help_dialog},
        shell::{environments, footer},
        theme,
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};
use std::time::Instant;

const MIN_MATRIX_HEIGHT: u16 = 5;
const MIN_PREVIEW_HEIGHT: u16 = 3;

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
        let context = overview_context(self);
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
        let regular_footer = overview_footer(
            area.width,
            self.preview_open,
            self.matrix.searching(),
            self.matrix.selected_group_expanded(),
        );
        let regular_footer_height = line_count(&regular_footer);
        let panel_height = self
            .preview_open
            .then(|| {
                let preview_space_height = area
                    .height
                    .saturating_sub(context_height + detail_height + regular_footer_height);
                available_preview_height(preview_space_height)
            })
            .flatten();
        let show_preview = panel_height.is_some();
        let footer = if self.preview_open && !show_preview {
            preview_unavailable_footer(area.width, self.matrix.searching())
        } else {
            regular_footer
        };
        let footer_height = line_count(&footer);
        let footer_separator_height = u16::from(show_boundaries && !show_preview);
        let matrix_height = area.height.saturating_sub(
            context_height
                + detail_height
                + footer_height
                + footer_separator_height
                + panel_height.unwrap_or(0),
        );
        let matrix_y = area.y.saturating_add(context_height + detail_height);
        matrix::render(
            frame,
            Rect::new(area.x, matrix_y, area.width, matrix_height),
            state,
            &mut self.matrix,
            self.selection.column,
        );
        if let Some(panel_height) = panel_height {
            let preview = self.selected_cell_preview(state);
            render_cell_preview(
                frame,
                Rect::new(
                    area.x,
                    matrix_y.saturating_add(matrix_height),
                    area.width,
                    panel_height,
                ),
                &preview,
            );
        }
        if footer_separator_height > 0 {
            frame.render_widget(
                separator::render(area.width),
                Rect::new(
                    area.x,
                    area.bottom().saturating_sub(footer_height + 1),
                    area.width,
                    1,
                ),
            );
        }
        render_footer(frame, area, &footer);
    }

    fn has_room_for_boundaries(&self, area: Rect, state: &EnvironmentSession) -> bool {
        let Some(plan) = state.plans().get(self.selection.column) else {
            return false;
        };
        let shell = environments::layout(area, state, self.notice.as_deref(), false);
        let context = overview_context(self);
        let detail = overview_detail(plan);
        let (context_height, detail_height) = section_heights(shell.body, &context, &detail);
        let footer_height = u16::try_from(
            overview_footer(
                area.width,
                self.preview_open,
                self.matrix.searching(),
                self.matrix.selected_group_expanded(),
            )
            .len(),
        )
        .unwrap_or(u16::MAX);
        let matrix_height = shell
            .body
            .height
            .saturating_sub(context_height + footer_height + detail_height);

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

fn overview_context(view: &EnvironmentView) -> String {
    if view.matrix.searching() || view.matrix.filtered() {
        format!("Filter: /{}   (display only)", view.matrix.filter())
    } else {
        String::new()
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
                    help_dialog::HelpAction::new(
                        "Enter / Esc",
                        "show / hide preview; Esc returns from full plan",
                    ),
                    help_dialog::HelpAction::new(
                        "1–9",
                        "open selected resource in the numbered environment's raw plan",
                    ),
                    help_dialog::HelpAction::new(
                        "Space",
                        "expand or collapse only on [+]/[-] group rows",
                    ),
                    help_dialog::HelpAction::new("/", "filter full addresses"),
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
                    help_dialog::HelpAction::new("+ / ~ / -", "create / update / delete"),
                    help_dialog::HelpAction::new(
                        "+/- / -/+",
                        "replace (create→delete / delete→create)",
                    ),
                    help_dialog::HelpAction::new("blank", "resource absent from this environment"),
                    help_dialog::HelpAction::new(".", "resource present, with no change"),
                    help_dialog::HelpAction::new("?", "plan unavailable; action unknown"),
                    help_dialog::HelpAction::new("read / move / import", "action shown by name"),
                    help_dialog::HelpAction::new(
                        "why: missing",
                        "resource present in only some Ready plans",
                    ),
                ],
            ),
            help_dialog::HelpSection::new(
                "Comparison",
                vec![
                    help_dialog::HelpAction::new(
                        "Same changes",
                        "no differences found in Ready plans; unknown values may differ",
                    ),
                    help_dialog::HelpAction::new("Scope", "only Ready environments are compared"),
                    help_dialog::HelpAction::new("Excluded", "environments are not retried"),
                ],
            ),
        ],
        scroll,
    );
}

fn available_preview_height(content_height: u16) -> Option<u16> {
    let available = content_height.checked_sub(MIN_MATRIX_HEIGHT)?;
    let preferred = content_height.saturating_sub(content_height.saturating_mul(2) / 5);
    (available >= MIN_PREVIEW_HEIGHT).then_some(available.min(preferred.max(MIN_PREVIEW_HEIGHT)))
}

fn overview_footer(
    width: u16,
    preview_open: bool,
    searching: bool,
    expanded: Option<bool>,
) -> Vec<Line<'static>> {
    if searching {
        return footer::layout(
            vec![
                footer::hint(&["Enter"], "confirm"),
                footer::hint(&["Esc"], "cancel"),
            ],
            width,
        );
    }
    if width < 45 {
        return compact_overview_footer(preview_open, expanded);
    }
    let (preview_action, plan_action) = if width < 56 {
        if preview_open {
            ("close preview", "plan")
        } else {
            ("preview", "plan")
        }
    } else {
        if preview_open {
            ("close preview", "full plan")
        } else {
            ("preview", "full plan")
        }
    };
    let mut items = vec![
        footer::hint(&["↑↓"], "row"),
        footer::hint(&["←→"], "env"),
        footer::hint(
            &[if preview_open { "Esc" } else { "Enter" }],
            preview_action,
        ),
        footer::hint(&["/"], "filter"),
    ];
    if let Some(expanded) = expanded {
        items.push(footer::hint(
            &["Space"],
            if expanded { "collapse" } else { "expand" },
        ));
    }
    if preview_open || !(width < 45 && expanded == Some(true)) {
        items.push(footer::hint(&["v"], plan_action));
    }
    items.extend([footer::hint(&["?"], "help"), footer::hint(&["q"], "quit")]);
    footer::layout(items, width)
}

fn compact_overview_footer(preview_open: bool, expanded: Option<bool>) -> Vec<Line<'static>> {
    let movement = [footer::hint(&["↑↓"], "row"), footer::hint(&["←→"], "env")];
    if preview_open {
        vec![
            join_footer_items(
                [
                    movement[0].clone(),
                    movement[1].clone(),
                    footer::hint(&["Esc"], "close preview"),
                ],
                "  ",
            ),
            join_footer_items(
                [
                    footer::hint(&["v"], "plan"),
                    footer::hint(&["/"], "filter"),
                    footer::hint(&["?"], "help"),
                    footer::hint(&["q"], "quit"),
                ],
                "  ",
            ),
        ]
    } else {
        let mut actions = Vec::new();
        if let Some(expanded) = expanded {
            actions.push(footer::hint(
                &["Space"],
                if expanded { "collapse" } else { "expand" },
            ));
        }
        if expanded != Some(true) {
            actions.push(footer::hint(&["v"], "plan"));
        }
        actions.extend([footer::hint(&["?"], "help"), footer::hint(&["q"], "quit")]);
        vec![
            join_footer_items(
                [
                    movement[0].clone(),
                    movement[1].clone(),
                    footer::hint(&["Enter"], "preview"),
                    footer::hint(&["/"], "filter"),
                ],
                "  ",
            ),
            join_footer_items(actions, "  "),
        ]
    }
}

fn join_footer_items(
    items: impl IntoIterator<Item = Line<'static>>,
    separator: &'static str,
) -> Line<'static> {
    let mut line = Line::default();
    for item in items {
        if !line.spans.is_empty() {
            line.push_span(Span::styled(separator, theme::footer_text_style()));
        }
        line.extend(item.spans);
    }
    line
}

fn preview_unavailable_footer(width: u16, searching: bool) -> Vec<Line<'static>> {
    if searching {
        return footer::layout(
            vec![
                footer::hint(&["Enter"], "confirm"),
                footer::hint(&["Esc"], "cancel"),
                Line::from("Resize for preview"),
            ],
            width,
        );
    }
    if width < 45 {
        return vec![
            join_footer_items(
                [footer::hint(&["↑↓"], "row"), footer::hint(&["←→"], "env")],
                "  ",
            ),
            join_footer_items(
                [
                    Line::from("Resize for preview"),
                    footer::hint(&["v"], "plan"),
                    footer::hint(&["q"], "quit"),
                ],
                "  ",
            ),
        ];
    }
    let preview_message = if width >= 56 {
        "Preview needs more room"
    } else {
        "Resize for preview"
    };
    footer::layout(
        vec![
            footer::hint(&["↑↓"], "row"),
            footer::hint(&["←→"], "env"),
            Line::from(preview_message),
            footer::hint(&["v"], if width < 56 { "plan" } else { "full plan" }),
            footer::hint(&["q"], "quit"),
            footer::hint(&["?"], "help"),
        ],
        width,
    )
}

fn line_count(lines: &[Line<'static>]) -> u16 {
    u16::try_from(lines.len()).unwrap_or(u16::MAX)
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, lines: &[Line<'static>]) {
    frame.render_widget(
        Paragraph::new(lines.to_owned()).style(theme::footer_text_style()),
        Rect::new(
            area.x,
            area.bottom().saturating_sub(line_count(lines)),
            area.width,
            line_count(lines),
        ),
    );
}

fn render_cell_preview(frame: &mut Frame<'_>, area: Rect, preview: &CellPreview) {
    frame.render_widget(
        separator::render(area.width),
        Rect::new(area.x, area.y, area.width, 1),
    );
    if area.height < MIN_PREVIEW_HEIGHT {
        return;
    }
    frame.render_widget(
        Paragraph::new(preview.title.as_str()).style(theme::accent_style()),
        Rect::new(area.x, area.y.saturating_add(1), area.width, 1),
    );
    let body_area = Rect::new(
        area.x,
        area.y.saturating_add(2),
        area.width,
        area.height.saturating_sub(2),
    );
    if preview.is_raw {
        frame.render_widget(
            Paragraph::new(preview.text.as_str()).style(theme::body_style()),
            body_area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(preview.text.as_str())
                .wrap(Wrap { trim: false })
                .style(theme::warning_style()),
            body_area,
        );
    }
}
