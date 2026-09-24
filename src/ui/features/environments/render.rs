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
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};
use std::time::Instant;

impl EnvironmentView {
    pub(super) fn overview_page_size(
        &self,
        size: ratatui::layout::Size,
        state: &EnvironmentSession,
    ) -> usize {
        if self.selection.raw.is_some() {
            return 1;
        }
        let area = Rect::new(0, 0, size.width, size.height);
        let visible_environments = self.visible_environments(state.plans().len());
        let layout = environments::overview_layout(
            area,
            state,
            self.notice.as_deref(),
            self.selected_environments.is_some(),
            &self.selection,
            &visible_environments,
        );
        let content = self.overview_content_layout(layout.body, state);
        usize::from(content.matrix.height.saturating_sub(2)).max(1)
    }

    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, state: &EnvironmentSession) {
        self.sync(state);
        let area = frame.area();
        let visible_environments = self.visible_environments(state.plans().len());
        let is_overview = self.selection.raw.is_none();
        if is_overview {
            frame.render_widget(Block::new().style(theme::overview_background_style()), area);
        }
        let layout = environments::overview_layout(
            area,
            state,
            self.notice.as_deref(),
            self.selected_environments.is_some(),
            &self.selection,
            &visible_environments,
        );
        environments::render_overview_header(
            frame,
            &layout,
            state,
            &self.selection,
            &visible_environments,
            self.selected_environments.is_some(),
        );
        if is_overview {
            if let Some(notice) = &self.notice {
                frame.render_widget(
                    Paragraph::new(notice.as_str())
                        .wrap(Wrap { trim: false })
                        .style(theme::overview_header_warning_style()),
                    layout.notice,
                );
            }
        } else {
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
        if state.plans().get(self.selection.column).is_none() {
            return;
        }
        let layout = self.overview_content_layout(area, state);
        frame.render_widget(
            Paragraph::new(layout.context)
                .wrap(Wrap { trim: false })
                .style(theme::overview_muted_style()),
            Rect::new(
                layout.content_area.x,
                layout.content_area.y,
                layout.content_area.width,
                layout.context_height,
            ),
        );
        frame.render_widget(
            Paragraph::new(layout.detail)
                .wrap(Wrap { trim: false })
                .style(theme::overview_warning_style()),
            Rect::new(
                layout.content_area.x,
                layout.content_area.y.saturating_add(layout.context_height),
                layout.content_area.width,
                layout.detail_height,
            ),
        );
        matrix::render(
            frame,
            layout.matrix,
            state,
            &mut self.matrix,
            self.selection.column,
        );
        render_footer(frame, layout.content_area, &layout.footer);
    }

    fn overview_content_layout(
        &self,
        area: Rect,
        state: &EnvironmentSession,
    ) -> OverviewContentLayout {
        let content_area = overview_content_area(area);
        let context = overview_context(self);
        let detail = state
            .plans()
            .get(self.selection.column)
            .map(overview_detail)
            .unwrap_or_default();
        let (context_height, detail_height) = section_heights(content_area, &context, &detail);
        let footer = overview_footer(
            content_area.width,
            self.matrix.searching(),
            self.matrix.groups_expanded(),
        );
        let matrix_height = area.height.saturating_sub(
            context_height
                + detail_height
                + line_count(&footer)
                + content_area.y.saturating_sub(area.y),
        );
        let matrix_y = content_area
            .y
            .saturating_add(context_height + detail_height);
        let matrix = Rect::new(content_area.x, matrix_y, content_area.width, matrix_height);
        OverviewContentLayout {
            content_area,
            context,
            context_height,
            detail,
            detail_height,
            matrix,
            footer,
        }
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

struct OverviewContentLayout {
    content_area: Rect,
    context: String,
    context_height: u16,
    detail: String,
    detail_height: u16,
    matrix: Rect,
    footer: Vec<Line<'static>>,
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
    } else if let Some(review) = plan.review().filter(|review| {
        let metadata = review.review().metadata();
        metadata.nonstandard_changes() > 0 || !metadata.output_names().is_empty()
    }) {
        let metadata = review.review().metadata();
        let count = metadata.nonstandard_changes();
        let outputs = !metadata.output_names().is_empty();
        let detail = if count > 0 && outputs {
            format!("{count} other change(s) and output changes")
        } else if count > 0 {
            format!("{count} other change(s)")
        } else {
            "output changes".to_owned()
        };
        format!("Other changes: {detail}. v opens the full plan.")
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
    let sections = overview_help_sections();
    help_dialog::render(frame, area, "Help", &sections, scroll);
}

fn overview_help_sections() -> Vec<help_dialog::HelpSection> {
    vec![
        help_current_overview(),
        help_other_actions(),
        help_matrix_legend(),
        help_comparison(),
        help_totals(),
    ]
}

fn help_current_overview() -> help_dialog::HelpSection {
    help_dialog::HelpSection::new(
        "Current: Overview",
        vec![
            help_dialog::HelpAction::new(
                "↑ / ↓ / j / k",
                "scroll matrix; move through filter candidates",
            ),
            help_dialog::HelpAction::new("← / →", "select env; move in filter search"),
            help_dialog::HelpAction::new("[ / ]", "select previous or next environment"),
            help_dialog::HelpAction::new(
                "Enter",
                "open the selected environment's full plan; apply filter or accept search",
            ),
            help_dialog::HelpAction::new("Esc", "return from plan; cancel filter; restore search"),
            help_dialog::HelpAction::new("1–9", "open the numbered environment's full plan"),
            help_dialog::HelpAction::new(
                "Space",
                "expand or collapse groups; toggle a filter candidate",
            ),
            help_dialog::HelpAction::new(
                "/",
                "filter addresses; search environment names in filter",
            ),
            help_dialog::HelpAction::new("e", "filter compared environments"),
        ],
    )
}

fn help_other_actions() -> help_dialog::HelpSection {
    help_dialog::HelpSection::new(
        "Other",
        vec![
            help_dialog::HelpAction::new(
                "PgUp / PgDn",
                "move a page in the active view or candidate list",
            ),
            help_dialog::HelpAction::new(
                "Home / End",
                "go to an endpoint or the start/end of search text",
            ),
            help_dialog::HelpAction::new("a", "clear filter search and select all"),
            help_dialog::HelpAction::new("v", "show the full plan from the top"),
            help_dialog::HelpAction::new("y", "copy the selected environment's plan"),
            help_dialog::HelpAction::new("c", "show environment context"),
            help_dialog::HelpAction::new("r", "retry a selected Error environment"),
            help_dialog::HelpAction::new("q", "quit; confirms first while acquiring"),
        ],
    )
}

fn help_matrix_legend() -> help_dialog::HelpSection {
    help_dialog::HelpSection::new(
        "Matrix legend",
        vec![
            help_dialog::HelpAction::new("+ / ~ / -", "create / update / delete"),
            help_dialog::HelpAction::new("+/- / -/+", "replace (create→delete / delete→create)"),
            help_dialog::HelpAction::new("blank", "resource absent from this environment"),
            help_dialog::HelpAction::new(".", "resource present, with no change"),
            help_dialog::HelpAction::new("?", "plan unavailable; action unknown"),
            help_dialog::HelpAction::new("read / move / import", "action shown by name"),
            help_dialog::HelpAction::new(
                "why: missing",
                "resource present in only some Ready plans",
            ),
        ],
    )
}

fn help_comparison() -> help_dialog::HelpSection {
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
    )
}

fn help_totals() -> help_dialog::HelpSection {
    help_dialog::HelpSection::new(
        "Totals",
        vec![
            help_dialog::HelpAction::new(
                "replace",
                "counted once as a replacement; it is not added to create or delete",
            ),
            help_dialog::HelpAction::new(
                "No changes",
                "all resource operation counts are zero for a Ready environment",
            ),
        ],
    )
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
        rows.push(item);
        row_widths.push(item_width);
    }
    rows.into_iter()
        .filter(|row| !row.spans.is_empty())
        .collect()
}

fn overview_footer(width: u16, searching: bool, expanded: Option<bool>) -> Vec<Line<'static>> {
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
        return compact_overview_footer(expanded, width);
    }
    let mut items = vec![
        overview_footer_hint(&["[ ]"], "environment"),
        overview_footer_hint(&["Enter"], "open"),
        overview_footer_hint(&["/"], "filter"),
        overview_footer_hint(&["e"], "environments"),
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
    items.extend([
        overview_footer_hint(&["?"], "help"),
        overview_footer_hint(&["q"], "quit"),
    ]);
    overview_footer_layout(items, width)
}

fn compact_overview_footer(expanded: Option<bool>, width: u16) -> Vec<Line<'static>> {
    if width < 32 {
        let mut items = vec![
            overview_footer_hint(&["[ ]"], "env"),
            overview_footer_hint(&["Enter"], "open"),
            overview_footer_hint(&["/"], "filter"),
        ];
        items.push(overview_footer_hint(&["e"], "env filter"));
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
        items.extend([
            overview_footer_hint(&["?"], "help"),
            overview_footer_hint(&["q"], "quit"),
        ]);
        return overview_footer_layout(items, width);
    }

    let environment = if width < 40 { "env" } else { "environment" };
    let items = vec![
        overview_footer_hint(&["[ ]"], environment),
        overview_footer_hint(&["Enter"], "open"),
        overview_footer_hint(&["/"], "filter"),
    ];
    let mut row_two = vec![overview_footer_hint(&["e"], "env filter")];
    if let Some(expanded) = expanded {
        row_two.push(overview_footer_hint(
            &["Space"],
            if expanded {
                "collapse all"
            } else {
                "expand all"
            },
        ));
    }
    vec![
        join_footer_items(items),
        join_footer_items(row_two),
        join_footer_items([
            overview_footer_hint(&["?"], "help"),
            overview_footer_hint(&["q"], "quit"),
        ]),
    ]
}

fn join_footer_items(items: impl IntoIterator<Item = Line<'static>>) -> Line<'static> {
    let mut line = Line::default();
    for item in items {
        if !line.spans.is_empty() {
            line.push_span(Span::styled("  ", theme::overview_footer_separator_style()));
        }
        line.extend(item.spans);
    }
    line
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
