use super::{EnvironmentDialog, EnvironmentView, sidebar};
use crate::{
    app::environments::{EnvironmentPlan, EnvironmentSession, EnvironmentState},
    ui::{
        features::{overview::matrix, plan_review},
        primitives::molecules::help_dialog,
        shell::{environments, footer},
        theme,
    },
};
use ratatui::{
    Frame,
    layout::{Rect, Size},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::time::Instant;

impl EnvironmentView {
    pub(super) fn overview_page_size(&self, size: Size, state: &EnvironmentSession) -> usize {
        if self.selection.raw.is_some() {
            return 1;
        }
        let area = Rect::new(0, 0, size.width, size.height);
        let sidebar_visible = self.sidebar_visible(size.width);
        let layout = environments::overview_layout(
            area,
            self.sidebar_width,
            sidebar_visible,
            self.maximized_for_width(size.width),
            !sidebar_visible && self.maximized_for_width(size.width).is_none(),
            false,
        );
        let content = self.matrix_content_layout(pane_inner(layout.matrix), state);
        usize::from(content.matrix.height.saturating_sub(2)).max(1)
    }

    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, state: &EnvironmentSession) {
        let area = frame.area();
        self.initialize(Size::new(area.width, area.height), state);
        self.sync(state);
        frame.render_widget(Block::new().style(theme::overview_background_style()), area);
        let layout = if self.selection.raw.is_some() {
            environments::overview_layout(area, self.sidebar_width, false, None, false, false)
        } else {
            let sidebar_visible = self.sidebar_visible(area.width);
            environments::overview_layout(
                area,
                self.sidebar_width,
                sidebar_visible,
                self.maximized_for_width(area.width),
                !sidebar_visible && self.maximized_for_width(area.width).is_none(),
                false,
            )
        };
        environments::render_header(frame, layout.header, state, &self.selection);
        if let Some(index) = self.selection.raw
            && let Some(review) = state.plans()[index].review()
        {
            let body = Rect::new(
                area.x,
                layout.header.bottom(),
                area.width,
                area.bottom().saturating_sub(layout.header.bottom()),
            );
            plan_review::render_environment(
                frame,
                body,
                review,
                &mut self.reviews[index],
                Instant::now(),
            );
        } else {
            self.render_overview(frame, &layout, state);
        }
        if self.confirming_quit {
            self.render_dialog(
                frame,
                area,
                "Stop acquiring environment plans?\nEnter stop and quit   Esc continue",
            );
        } else if let Some(dialog) = &self.dialog {
            match dialog {
                EnvironmentDialog::Help => render_help_dialog(frame, area, self.dialog_scroll),
                EnvironmentDialog::Message(text) => self.render_dialog(frame, area, text),
            }
        }
    }

    fn render_overview(
        &mut self,
        frame: &mut Frame<'_>,
        layout: &environments::EnvironmentLayout,
        state: &EnvironmentSession,
    ) {
        if layout.environments.width > 0 && layout.environments.height > 0 {
            sidebar::render(
                frame,
                layout.environments,
                state.plans(),
                self.selection.column,
                &self.compared_environments(state.plans().len()),
                self.active_pane(layout.body.width) == environments::EnvironmentPane::Environments,
            );
        }
        if layout.summary.height > 0 {
            render_environment_summary(frame, layout.summary, state, self.selection.column);
        }
        if layout.matrix.width > 0 && layout.matrix.height > 0 {
            self.render_matrix_panel(
                frame,
                layout.matrix,
                state,
                self.active_pane(layout.body.width) == environments::EnvironmentPane::Matrix,
            );
        }
        let matrix_state = if self.matrix.searching() {
            MatrixFooterState::Searching
        } else if self.matrix.filter().is_empty() {
            MatrixFooterState::Unfiltered
        } else {
            MatrixFooterState::Filtered
        };
        let focus = self.active_pane(layout.body.width);
        let footer_lines = overview_footer(OverviewFooterContext {
            width: layout.footer.width,
            focus,
            matrix: matrix_state,
            expanded: self.matrix.groups_expanded(),
            selected: state.plans().get(self.selection.column),
            maximized: self.maximized.is_some(),
            sidebar_available: layout.body.width >= 90,
            resize_guidance: layout.body.height < 3
                || (focus == environments::EnvironmentPane::Matrix && layout.matrix.width < 3),
        });
        frame.render_widget(
            Paragraph::new(footer_lines).style(theme::overview_text_style()),
            layout.footer,
        );
    }

    fn render_matrix_panel(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        state: &EnvironmentSession,
        focused: bool,
    ) {
        let block = pane_block(
            focused,
            if area.height < 3 || area.width < 3 {
                "Resize terminal"
            } else {
                "[2] Differs across envs"
            },
            overview_pane_border_style(focused),
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let layout = self.matrix_content_layout(inner, state);
        if layout.context_height > 0 {
            frame.render_widget(
                Paragraph::new(layout.context)
                    .wrap(Wrap { trim: false })
                    .style(theme::overview_muted_style()),
                Rect::new(inner.x, inner.y, inner.width, layout.context_height),
            );
        }
        if layout.detail_height > 0 {
            frame.render_widget(
                Paragraph::new(layout.detail)
                    .wrap(Wrap { trim: false })
                    .style(theme::overview_warning_style()),
                Rect::new(
                    inner.x,
                    inner.y.saturating_add(layout.context_height),
                    inner.width,
                    layout.detail_height,
                ),
            );
        }
        matrix::render(frame, layout.matrix, state, &mut self.matrix);
    }

    fn matrix_content_layout(&self, area: Rect, state: &EnvironmentSession) -> MatrixContentLayout {
        let context = overview_context(self, state);
        let detail = state
            .plans()
            .get(self.selection.column)
            .map(overview_detail)
            .unwrap_or_default();
        let (context_height, detail_height) = section_heights(area, &context, &detail);
        let y = area.y.saturating_add(context_height + detail_height);
        let matrix = Rect::new(area.x, y, area.width, area.bottom().saturating_sub(y));
        MatrixContentLayout {
            context,
            context_height,
            detail,
            detail_height,
            matrix,
        }
    }

    fn render_dialog(&self, frame: &mut Frame<'_>, area: Rect, text: &str) {
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

fn render_environment_summary(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &EnvironmentSession,
    selected: usize,
) {
    let all = state
        .plans()
        .iter()
        .map(environment_summary_line)
        .collect::<Vec<_>>();
    let mut line = Line::default();
    for (index, part) in all.iter().enumerate() {
        if index > 0 {
            line.push_span(Span::styled("  │  ", theme::overview_muted_style()));
        }
        line.extend(part.spans.clone());
    }
    if line.width() > usize::from(area.width) {
        line = state
            .plans()
            .get(selected)
            .map_or_else(Line::default, environment_summary_line);
    }
    frame.render_widget(
        Paragraph::new(line).style(theme::overview_text_style()),
        area,
    );
}

fn environment_summary_line(plan: &EnvironmentPlan) -> Line<'static> {
    let status_style = match plan.state() {
        EnvironmentState::Pending | EnvironmentState::Running => theme::overview_muted_style(),
        EnvironmentState::Ready { .. } => theme::overview_text_style(),
        EnvironmentState::Error => theme::overview_total_destroy_style(),
        EnvironmentState::ExcludedHcp => theme::overview_warning_style(),
    };
    let mut line = Line::from(vec![
        Span::styled(environments::name(plan), theme::overview_text_style()),
        Span::styled(" ", theme::overview_muted_style()),
        Span::styled(environments::status(plan), status_style),
    ]);
    if let Some(review) = plan.review() {
        let counts = review.review().metadata();
        for (count, label, style) in [
            (counts.additions(), "+", theme::overview_total_add_style()),
            (counts.changes(), "~", theme::overview_total_update_style()),
            (
                counts.deletions(),
                "-",
                theme::overview_total_destroy_style(),
            ),
        ] {
            if count > 0 {
                line.push_span(Span::styled(format!(" {label}{count}"), style));
            }
        }
        if counts.replacements() > 0 {
            line.push_span(Span::styled(
                format!(" {} replace", counts.replacements()),
                theme::overview_total_replace_style(),
            ));
        }
        if !counts.has_changes() && counts.nonstandard_changes() == 0 {
            line.push_span(Span::styled(" No changes", theme::overview_muted_style()));
        }
    }
    line
}

struct MatrixContentLayout {
    context: String,
    context_height: u16,
    detail: String,
    detail_height: u16,
    matrix: Rect,
}

fn pane_block(focused: bool, title: &str, border_style: Style) -> Block<'static> {
    let mark = if focused { "* " } else { "  " };
    Block::new()
        .borders(Borders::ALL)
        .title(Line::from(vec![
            Span::styled(
                mark,
                if focused {
                    Style::default().fg(Color::Cyan).bg(Color::Reset)
                } else {
                    theme::overview_muted_style()
                },
            ),
            Span::styled(title.to_owned(), theme::overview_header_accent_style()),
        ]))
        .border_style(border_style)
        .style(theme::overview_background_style())
}

fn pane_inner(area: Rect) -> Rect {
    pane_block(false, "", overview_pane_border_style(false)).inner(area)
}

fn overview_pane_border_style(focused: bool) -> Style {
    Style::default()
        .fg(if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        })
        .bg(Color::Reset)
}

fn overview_context(view: &EnvironmentView, state: &EnvironmentSession) -> String {
    if let Some(notice) = &view.notice {
        return notice.clone();
    }
    if view.matrix.searching() || view.matrix.filtered() {
        return format!("Filter: /{}   (display only)", view.matrix.filter());
    }
    if !view
        .compared_environments(state.plans().len())
        .contains(&view.selection.column)
    {
        return "Selected environment is excluded from the comparison.".to_owned();
    }
    String::new()
}

fn overview_detail(plan: &EnvironmentPlan) -> String {
    if matches!(plan.state(), EnvironmentState::Error) {
        plan.diagnostic()
            .text()
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
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
    .min(area.height.saturating_sub(5));
    let detail_height = u16::try_from(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .line_count(area.width.max(1)),
    )
    .unwrap_or(u16::MAX)
    .min(3)
    .min(area.height.saturating_sub(context_height + 5));
    (context_height, detail_height)
}

fn render_help_dialog(frame: &mut Frame<'_>, area: Rect, scroll: u16) {
    help_dialog::render(frame, area, "Help", &overview_help_sections(), scroll);
}

fn overview_help_sections() -> Vec<help_dialog::HelpSection> {
    vec![
        help_dialog::HelpSection::new(
            "Current: Multi-environment Overview",
            vec![
                help_dialog::HelpAction::new(
                    "↑ / ↓ / j / k",
                    "select environments in [1] or scroll [2]",
                ),
                help_dialog::HelpAction::new("Space", "include or exclude an environment in [1]"),
                help_dialog::HelpAction::new("Space", "expand or collapse groups in [2]"),
                help_dialog::HelpAction::new(
                    "o / a",
                    "compare only the selected environment / all environments in [1]",
                ),
                help_dialog::HelpAction::new("[ / ]", "select the previous or next environment"),
                help_dialog::HelpAction::new("1 / 2", "focus Envs / Differs; 1 opens Envs"),
                help_dialog::HelpAction::new("b", "toggle the Envs sidebar"),
                help_dialog::HelpAction::new("f", "maximize or restore the focused pane"),
                help_dialog::HelpAction::new("Enter", "open the selected environment's full plan"),
                help_dialog::HelpAction::new("/", "filter matrix addresses; display only"),
                help_dialog::HelpAction::new("r", "retry the selected Error environment"),
            ],
        ),
        help_dialog::HelpSection::new(
            "Other",
            vec![
                help_dialog::HelpAction::new("← / →", "select an environment while [2] is focused"),
                help_dialog::HelpAction::new("v", "open the full plan from the top"),
                help_dialog::HelpAction::new("y", "copy the selected environment's plan"),
                help_dialog::HelpAction::new("c", "show environment context"),
                help_dialog::HelpAction::new("?", "show or close this help"),
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
            ],
        ),
        help_dialog::HelpSection::new(
            "Comparison",
            vec![
                help_dialog::HelpAction::new(
                    "Same changes",
                    "no differences found in Ready plans; unknown values may differ",
                ),
                help_dialog::HelpAction::new(
                    "Excluded",
                    "environments remain selectable and are not retried",
                ),
                help_dialog::HelpAction::new("Scope", "only Ready plans are compared"),
                help_dialog::HelpAction::new(
                    "why: missing",
                    "resource is present in only some Ready plans",
                ),
            ],
        ),
    ]
}

#[derive(Clone, Copy)]
enum MatrixFooterState {
    Searching,
    Filtered,
    Unfiltered,
}

#[derive(Clone, Copy)]
struct OverviewFooterContext<'a> {
    width: u16,
    focus: environments::EnvironmentPane,
    matrix: MatrixFooterState,
    expanded: Option<bool>,
    selected: Option<&'a EnvironmentPlan>,
    maximized: bool,
    sidebar_available: bool,
    resize_guidance: bool,
}

fn overview_footer(context: OverviewFooterContext<'_>) -> Vec<Line<'static>> {
    let OverviewFooterContext {
        width,
        focus,
        matrix,
        expanded,
        selected,
        maximized,
        sidebar_available,
        resize_guidance,
    } = context;
    if resize_guidance {
        return footer::layout(
            vec![
                Line::from("Resize terminal to view pane content"),
                overview_footer_hint(&["q"], "quit"),
            ],
            width,
        );
    }
    if matches!(matrix, MatrixFooterState::Searching) {
        return footer::layout(
            vec![
                overview_footer_hint(&["Enter"], "confirm"),
                overview_footer_hint(&["Esc"], "cancel"),
            ],
            width,
        );
    }
    if width < 45 {
        return compact_overview_footer(width, matrix, expanded);
    }
    let mut items = vec![overview_footer_hint(&["[", "]"], "env")];
    if focus == environments::EnvironmentPane::Environments {
        items.extend([
            overview_footer_hint(&["Enter"], "open plan"),
            overview_footer_hint(&["Space"], "include/exclude"),
            overview_footer_hint(&["o"], "only"),
            overview_footer_hint(&["a"], "all"),
        ]);
        if selected.is_some_and(|plan| matches!(plan.state(), EnvironmentState::Error)) {
            items.push(overview_footer_hint(&["r"], "retry"));
        }
    } else {
        items.push(overview_footer_hint(&["Enter"], "open plan"));
        items.push(overview_footer_hint(&["v"], "full plan"));
        items.push(overview_footer_hint(&["/"], "filter"));
        if matches!(matrix, MatrixFooterState::Unfiltered)
            && let Some(expanded) = expanded
        {
            items.push(overview_footer_hint(
                &["Space"],
                if expanded {
                    "collapse all"
                } else if width < 45 {
                    "expand"
                } else {
                    "expand all"
                },
            ));
        }
    }
    if sidebar_available && !maximized {
        items.push(overview_footer_hint(&["b"], "toggle envs"));
    }
    if width < 45 {
        items.push(overview_footer_hint(&["1", "2"], "focus"));
        items.push(overview_footer_hint(&["?", "q"], "help/quit"));
    } else {
        items.push(overview_footer_hint(&["?"], "help"));
        items.push(overview_footer_hint(&["q"], "quit"));
        items.push(overview_footer_hint(&["1", "2"], "focus"));
    }
    items.push(if maximized {
        overview_footer_hint(&["f", "Esc"], "restore")
    } else {
        overview_footer_hint(&["f"], "maximize")
    });
    footer::layout(items, width)
}

fn compact_overview_footer(
    width: u16,
    matrix: MatrixFooterState,
    expanded: Option<bool>,
) -> Vec<Line<'static>> {
    let mut items = vec![
        overview_footer_hint(&["[", "]"], "env"),
        overview_footer_hint(&["Enter"], "open plan"),
        overview_footer_hint(&["v"], "full plan"),
        overview_footer_hint(&["/"], "filter"),
    ];
    if matches!(matrix, MatrixFooterState::Unfiltered)
        && let Some(expanded) = expanded
    {
        items.push(overview_footer_hint(
            &["Space"],
            if expanded { "collapse all" } else { "expand" },
        ));
    }
    items.push(overview_footer_hint(&["?", "q"], "help/quit"));
    footer::layout(items, width)
}

fn overview_footer_hint(keys: &[&'static str], description: &'static str) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, key) in keys.iter().enumerate() {
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
