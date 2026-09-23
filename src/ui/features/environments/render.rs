use super::{EnvironmentDialog, EnvironmentView, PlanPreview};
use crate::{
    app::environments::{EnvironmentPlan, EnvironmentSession, EnvironmentState},
    ui::{
        features::{overview::matrix, plan_review},
        primitives::{
            atoms::{scrollbar, separator},
            molecules::help_dialog,
        },
        shell::environments,
        theme,
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};
use std::time::Instant;

const MIN_MATRIX_HEIGHT: u16 = 5;
const MIN_PREVIEW_HEIGHT: u16 = 4;

impl EnvironmentView {
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, state: &EnvironmentSession) {
        self.sync(state);
        let area = frame.area();
        let visible_environments = self.visible_environments(state.plans().len());
        let is_overview = self.selection.raw.is_none();
        if is_overview {
            frame.render_widget(Block::new().style(theme::overview_background_style()), area);
        }
        let layout = if is_overview {
            environments::overview_layout(
                area,
                state,
                self.notice.as_deref(),
                self.selected_environments.is_some(),
                &self.selection,
                &visible_environments,
            )
        } else {
            environments::layout(
                area,
                state,
                self.notice.as_deref(),
                self.selected_environments.is_some(),
                false,
            )
        };
        if is_overview {
            environments::render_overview_header(
                frame,
                &layout,
                state,
                &self.selection,
                &visible_environments,
                self.selected_environments.is_some(),
            );
            if let Some(notice) = &self.notice {
                frame.render_widget(
                    Paragraph::new(notice.as_str())
                        .wrap(Wrap { trim: false })
                        .style(theme::overview_header_warning_style()),
                    layout.notice,
                );
            }
        } else {
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
        }
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
            self.render_overview(frame, layout.body, state);
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

    fn render_overview(&mut self, frame: &mut Frame<'_>, area: Rect, state: &EnvironmentSession) {
        let Some(plan) = state.plans().get(self.selection.column) else {
            return;
        };
        let context = overview_context(self);
        let detail = overview_detail(plan);
        let content_area = overview_content_area(area);
        let (context_height, detail_height) = section_heights(content_area, &context, &detail);
        frame.render_widget(
            Paragraph::new(context)
                .wrap(Wrap { trim: false })
                .style(theme::overview_muted_style()),
            Rect::new(
                content_area.x,
                content_area.y,
                content_area.width,
                context_height,
            ),
        );
        frame.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: false })
                .style(theme::overview_warning_style()),
            Rect::new(
                content_area.x,
                content_area.y.saturating_add(context_height),
                content_area.width,
                detail_height,
            ),
        );
        let regular_footer = overview_footer(
            content_area.width,
            self.preview_open,
            self.preview_focused,
            self.matrix.searching(),
            self.matrix.groups_expanded(),
        );
        let regular_footer_height = line_count(&regular_footer);
        let panel_height = self
            .preview_open
            .then(|| {
                let preview_space_height = area
                    .height
                    .saturating_sub(context_height + detail_height + regular_footer_height)
                    .saturating_sub(content_area.y.saturating_sub(area.y));
                available_preview_height(preview_space_height)
            })
            .flatten();
        let show_preview = panel_height.is_some();
        if !show_preview {
            self.preview_focused = false;
        }
        let footer = if self.preview_open && !show_preview {
            preview_unavailable_footer(content_area.width, self.matrix.searching())
        } else {
            regular_footer
        };
        let footer_height = line_count(&footer);
        let matrix_height = area.height.saturating_sub(
            context_height
                + detail_height
                + footer_height
                + content_area.y.saturating_sub(area.y)
                + panel_height.unwrap_or(0),
        );
        let matrix_y = content_area
            .y
            .saturating_add(context_height + detail_height);
        matrix::render(
            frame,
            Rect::new(content_area.x, matrix_y, content_area.width, matrix_height),
            state,
            &mut self.matrix,
            self.selection.column,
        );
        if let Some(panel_height) = panel_height {
            let preview = self.selected_plan_preview(state);
            render_plan_preview(
                frame,
                Rect::new(
                    content_area.x,
                    matrix_y.saturating_add(matrix_height),
                    content_area.width,
                    panel_height,
                ),
                &preview,
                &mut self.preview_vertical,
                &mut self.preview_horizontal,
                self.preview_focused,
            );
        }
        render_footer(frame, content_area, &footer);
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

fn overview_content_area(area: Rect) -> Rect {
    let margin = if area.width >= 50 {
        2
    } else {
        u16::from(area.width >= 34)
    };
    let header_gap = u16::from(area.height > 18);
    Rect::new(
        area.x.saturating_add(margin),
        area.y.saturating_add(header_gap),
        area.width.saturating_sub(margin.saturating_mul(2)),
        area.height.saturating_sub(header_gap),
    )
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
                    help_dialog::HelpAction::new("↑ / ↓ / j / k", "scroll the matrix or preview"),
                    help_dialog::HelpAction::new("← / →", "select env in matrix; scroll preview"),
                    help_dialog::HelpAction::new("[ / ]", "select previous or next environment"),
                    help_dialog::HelpAction::new(
                        "Enter",
                        "open the selected environment's full plan preview",
                    ),
                    help_dialog::HelpAction::new("Tab", "switch input between matrix and preview"),
                    help_dialog::HelpAction::new("Esc", "close preview; return from full plan"),
                    help_dialog::HelpAction::new(
                        "1–9",
                        "open the numbered environment's full plan",
                    ),
                    help_dialog::HelpAction::new(
                        "Space",
                        "expand all collapsed groups, or collapse all groups",
                    ),
                    help_dialog::HelpAction::new("/", "filter full addresses"),
                    help_dialog::HelpAction::new("e", "filter compared environments"),
                ],
            ),
            help_dialog::HelpSection::new(
                "Other",
                vec![
                    help_dialog::HelpAction::new("PgUp / PgDn", "move one page"),
                    help_dialog::HelpAction::new(
                        "Home / End",
                        "go to the start or end of the matrix or preview",
                    ),
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

fn overview_footer_hint(
    alternative_keys: &[&'static str],
    description: &'static str,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, key) in alternative_keys.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("/", theme::overview_footer_separator_style()));
        }
        spans.push(Span::styled(*key, theme::overview_footer_key_style()));
    }
    spans.push(Span::styled(
        format!(" {description}"),
        theme::overview_footer_text_style(),
    ));
    Line::from(spans)
}

fn overview_footer_layout(items: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    let mut rows = vec![Line::default()];
    let mut row_widths = vec![0usize];
    for item in items {
        let item_width = item.width();
        if item_width == 0 || item_width > width {
            continue;
        }
        let row_index = rows.len() - 1;
        let row = &mut rows[row_index];
        let separator_width = usize::from(!row.spans.is_empty()) * 3;
        if row_widths[row_index] + separator_width + item_width <= width {
            if !row.spans.is_empty() {
                row.push_span(Span::styled(
                    " | ",
                    theme::overview_footer_separator_style(),
                ));
            }
            row.extend(item.spans);
            row_widths[row_index] += separator_width + item_width;
            continue;
        }
        if rows.len() == 2 {
            continue;
        }
        rows.push(item);
        row_widths.push(item_width);
    }
    rows.into_iter()
        .filter(|row| !row.spans.is_empty())
        .collect()
}

fn available_preview_height(content_height: u16) -> Option<u16> {
    let minimum_height = MIN_MATRIX_HEIGHT.saturating_add(MIN_PREVIEW_HEIGHT);
    (content_height >= minimum_height).then_some(content_height / 2)
}

fn overview_footer(
    width: u16,
    preview_open: bool,
    preview_focused: bool,
    searching: bool,
    expanded: Option<bool>,
) -> Vec<Line<'static>> {
    if searching {
        return overview_footer_layout(
            vec![
                overview_footer_hint(&["Enter"], "confirm"),
                overview_footer_hint(&["Esc"], "cancel"),
            ],
            width,
        );
    }
    if width < 45 {
        return compact_overview_footer(preview_open, preview_focused, expanded);
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
        overview_footer_hint(
            &["↑↓"],
            if preview_focused {
                "scroll preview"
            } else {
                "scroll rows"
            },
        ),
        overview_footer_hint(
            &["←→"],
            if preview_focused {
                "scroll preview"
            } else {
                "select env"
            },
        ),
        overview_footer_hint(
            &[if preview_open { "Esc" } else { "Enter" }],
            preview_action,
        ),
        if preview_open {
            overview_footer_hint(&["Tab"], if preview_focused { "matrix" } else { "preview" })
        } else {
            overview_footer_hint(&["[ ]"], "select env")
        },
        overview_footer_hint(&["/"], "filter"),
        overview_footer_hint(&["e"], "env filter"),
    ];
    if let Some(expanded) = expanded {
        items.push(overview_footer_hint(
            &["Space"],
            if expanded {
                "collapse all"
            } else {
                "expand all"
            },
        ));
    }
    if preview_open || !(width < 45 && expanded == Some(true)) {
        items.push(overview_footer_hint(&["v"], plan_action));
    }
    items.extend([
        overview_footer_hint(&["?"], "help"),
        overview_footer_hint(&["q"], "quit"),
    ]);
    overview_footer_layout(items, width)
}

fn compact_overview_footer(
    preview_open: bool,
    preview_focused: bool,
    expanded: Option<bool>,
) -> Vec<Line<'static>> {
    let movement = [
        overview_footer_hint(&["↑↓"], if preview_focused { "preview" } else { "rows" }),
        overview_footer_hint(&["←→"], if preview_focused { "preview" } else { "env" }),
    ];
    if preview_open {
        vec![
            join_footer_items(
                [
                    movement[0].clone(),
                    movement[1].clone(),
                    overview_footer_hint(&["Esc"], "close"),
                    overview_footer_hint(
                        &["Tab"],
                        if preview_focused { "matrix" } else { "preview" },
                    ),
                ],
                "  ",
            ),
            join_footer_items(
                [
                    overview_footer_hint(&["/"], "filter"),
                    overview_footer_hint(&["e"], "env filter"),
                    overview_footer_hint(&["v"], "plan"),
                ],
                "  ",
            ),
            join_footer_items(
                [
                    overview_footer_hint(&["?"], "help"),
                    overview_footer_hint(&["q"], "quit"),
                ],
                "  ",
            ),
        ]
    } else {
        let mut actions = Vec::new();
        if let Some(expanded) = expanded {
            actions.push(overview_footer_hint(
                &["Space"],
                if expanded {
                    "collapse all"
                } else {
                    "expand all"
                },
            ));
        }
        actions.extend([
            overview_footer_hint(&["?"], "help"),
            overview_footer_hint(&["q"], "quit"),
        ]);
        vec![
            join_footer_items(
                [
                    movement[0].clone(),
                    movement[1].clone(),
                    overview_footer_hint(&["Enter"], "preview"),
                ],
                "  ",
            ),
            join_footer_items(
                [
                    overview_footer_hint(&["/"], "filter"),
                    overview_footer_hint(&["e"], "env filter"),
                    overview_footer_hint(&["v"], "plan"),
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
            line.push_span(Span::styled(
                separator,
                theme::overview_footer_separator_style(),
            ));
        }
        line.extend(item.spans);
    }
    line
}

fn preview_unavailable_footer(width: u16, searching: bool) -> Vec<Line<'static>> {
    if searching {
        return overview_footer_layout(
            vec![
                overview_footer_hint(&["Enter"], "confirm"),
                overview_footer_hint(&["Esc"], "cancel"),
                Line::from("Resize for preview"),
            ],
            width,
        );
    }
    if width < 45 {
        return vec![
            join_footer_items(
                [
                    overview_footer_hint(&["↑↓"], "row"),
                    overview_footer_hint(&["←→"], "env"),
                ],
                "  ",
            ),
            join_footer_items(
                [
                    Line::from("Resize for preview"),
                    overview_footer_hint(&["v"], "plan"),
                ],
                "  ",
            ),
            join_footer_items(
                [
                    overview_footer_hint(&["e"], "env filter"),
                    overview_footer_hint(&["?"], "help"),
                    overview_footer_hint(&["q"], "quit"),
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
    overview_footer_layout(
        vec![
            overview_footer_hint(&["↑↓"], "row"),
            overview_footer_hint(&["←→"], "env"),
            Line::from(preview_message),
            overview_footer_hint(&["v"], if width < 56 { "plan" } else { "full plan" }),
            overview_footer_hint(&["q"], "quit"),
            overview_footer_hint(&["?"], "help"),
        ],
        width,
    )
}

fn line_count(lines: &[Line<'static>]) -> u16 {
    u16::try_from(lines.len()).unwrap_or(u16::MAX)
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, lines: &[Line<'static>]) {
    frame.render_widget(
        Paragraph::new(lines.to_owned()).style(theme::overview_footer_text_style()),
        Rect::new(
            area.x,
            area.bottom().saturating_sub(line_count(lines)),
            area.width,
            line_count(lines),
        ),
    );
}

fn render_plan_preview(
    frame: &mut Frame<'_>,
    area: Rect,
    preview: &PlanPreview,
    vertical: &mut usize,
    horizontal: &mut usize,
    focused: bool,
) {
    if area.height < MIN_PREVIEW_HEIGHT {
        return;
    }
    let title = if focused {
        format!("> {}", preview.title)
    } else {
        preview.title.clone()
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            title,
            theme::overview_preview_title_style(focused),
        ))
        .style(theme::overview_total_style()),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let body_area = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    );
    let lines = preview
        .text
        .split('\n')
        .map(|line| {
            Line::styled(
                line.to_owned(),
                if preview.is_raw {
                    theme::overview_plan_line_style(line)
                } else {
                    theme::overview_warning_style()
                },
            )
        })
        .collect::<Vec<_>>();
    let line_width = lines.iter().map(Line::width).max().unwrap_or(0);
    let line_count = lines.len();
    let (vertical_scrollbar, horizontal_scrollbar) =
        preview_scrollbars(line_count, line_width, body_area);
    let content = Rect::new(
        body_area.x,
        body_area.y,
        body_area
            .width
            .saturating_sub(u16::from(vertical_scrollbar)),
        body_area
            .height
            .saturating_sub(u16::from(horizontal_scrollbar)),
    );
    *vertical = (*vertical).min(line_count.saturating_sub(usize::from(content.height)));
    *horizontal = (*horizontal).min(line_width.saturating_sub(usize::from(content.width)));
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::overview_text_style())
            .scroll((
                u16::try_from(*vertical).unwrap_or(u16::MAX),
                u16::try_from(*horizontal).unwrap_or(u16::MAX),
            )),
        content,
    );
    if vertical_scrollbar {
        scrollbar::render_vertical(
            frame,
            Rect::new(content.right(), content.y, 1, content.height),
            line_count,
            usize::from(content.height),
            *vertical,
        );
    }
    if horizontal_scrollbar {
        scrollbar::render_horizontal(
            frame,
            Rect::new(content.x, content.bottom(), content.width, 1),
            line_width,
            usize::from(content.width),
            *horizontal,
        );
    }
}

fn preview_scrollbars(line_count: usize, line_width: usize, area: Rect) -> (bool, bool) {
    let mut vertical = line_count > usize::from(area.height);
    let mut horizontal = line_width > usize::from(area.width);
    for _ in 0..2 {
        let width = usize::from(area.width).saturating_sub(usize::from(vertical));
        let height = usize::from(area.height).saturating_sub(usize::from(horizontal));
        horizontal = line_width > width;
        vertical = line_count > height;
    }
    (vertical, horizontal)
}
