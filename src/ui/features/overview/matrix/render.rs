use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{MatrixCell, MatrixView, view::Row};
use crate::app::{
    environments::{
        EnvironmentPlan, EnvironmentSession,
        comparison::{CellState, ComparisonScope, DifferenceReason},
    },
    plan::{PlanAction, ResourceChangeKind},
};
use crate::ui::{
    shell::environments::{name, status},
    theme,
};

const WHY_WIDTH: usize = 7;
const MIN_CELL_WIDTH: usize = 9;
const COLUMN_GAP: usize = 1;
const MIN_ADDRESS_WIDTH: usize = 12;
const MAX_ADDRESS_WIDTH: usize = 52;

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &EnvironmentSession,
    view: &mut MatrixView,
    selected_environment: usize,
) {
    if area.width < 30 || area.height < 5 {
        frame.render_widget(
            Paragraph::new("Resize to view the matrix. ? help   q quit"),
            area,
        );
        return;
    }
    let address_width = address_width(area, view);
    let selected_column = view.selected_column(selected_environment);
    let column_widths = column_widths(state, view);
    let column_budget =
        usize::from(area.width).saturating_sub(address_width + WHY_WIDTH + COLUMN_GAP);
    let columns = visible_columns(view, &column_widths, column_budget, selected_column);

    render_column_headers(
        frame,
        area,
        state,
        view,
        &columns,
        address_width,
        selected_column,
    );

    render_content(
        frame,
        area,
        state,
        view,
        &columns,
        address_width,
        selected_column,
    );
}

fn render_content(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &EnvironmentSession,
    view: &mut MatrixView,
    columns: &[(usize, usize)],
    address_width: usize,
    selected_column: usize,
) {
    let filtered = view.environments.len() != state.plans().len();
    let partial = view
        .overview
        .as_ref()
        .is_none_or(|overview| !matches!(overview.scope, ComparisonScope::All { .. }));
    let mut lines = Vec::new();
    let mut section = None;
    for row in &view.rows {
        if !row.child && section != Some(row.difference.is_some()) {
            if section.is_some() {
                lines.push(Line::default());
            }
            section = Some(row.difference.is_some());
            let title = if filtered && partial {
                if row.difference.is_some() {
                    "Differs across selected envs (Ready only)"
                } else {
                    "Same change across selected envs (Ready only)"
                }
            } else if filtered {
                if row.difference.is_some() {
                    "Differs across selected envs"
                } else {
                    "Same change across selected envs"
                }
            } else if partial && row.difference.is_some() {
                "Differs across envs (Ready only)"
            } else if partial {
                "Same change across envs (Ready only)"
            } else if row.difference.is_some() {
                "Differs across envs"
            } else {
                "Same change across envs"
            };
            lines.push(Line::styled(title, theme::accent_style()));
        }
        lines.push(row_line(row, view, columns, address_width, selected_column));
    }
    if lines.is_empty() {
        let waiting = view
            .overview
            .as_ref()
            .is_none_or(|overview| matches!(overview.scope, ComparisonScope::Waiting));
        lines.push(Line::from(if waiting {
            "Waiting for environment plans."
        } else {
            "No matching resource changes. v opens the full plan."
        }));
    }
    lines.push(Line::styled(
        "─".repeat(usize::from(area.width)),
        theme::separator_style(),
    ));
    lines.extend(total_lines(state, view, columns, address_width));
    let legend = symbol_legend(area.width);
    let legend_height = u16::try_from(legend.len()).unwrap_or(u16::MAX);
    let body = Rect::new(
        area.x,
        area.y + 1 + legend_height,
        area.width,
        area.height.saturating_sub(1 + legend_height),
    );
    render_legend(frame, area, &legend);
    view.vertical = view
        .vertical
        .min(lines.len().saturating_sub(usize::from(body.height)));
    frame.render_widget(
        Paragraph::new(lines).scroll((u16::try_from(view.vertical).unwrap_or(u16::MAX), 0)),
        body,
    );
}

fn render_column_headers(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &EnvironmentSession,
    view: &MatrixView,
    columns: &[(usize, usize)],
    address_width: usize,
    selected_column: usize,
) {
    let mut header = vec![Span::styled("Address", theme::secondary_style())];
    header.push(Span::raw(" ".repeat(address_width.saturating_sub(7))));
    for &(column, column_width) in columns {
        let environment = view.environments[column];
        let label = name(&state.plans()[environment]);
        let selected = column == selected_column;
        let marker = if selected { "> " } else { "  " };
        let (label, padding) = fit_parts(
            &label,
            column_width.saturating_sub(COLUMN_GAP + marker.len()),
            false,
        );
        header.push(Span::styled(
            format!("{marker}{label}"),
            if selected {
                theme::accent_style().add_modifier(Modifier::BOLD)
            } else {
                theme::secondary_style()
            },
        ));
        header.push(Span::raw(format!("{} ", " ".repeat(padding))));
    }
    header.push(Span::raw(" "));
    header.push(Span::styled("why", theme::secondary_style()));
    header.push(Span::raw(" ".repeat(WHY_WIDTH.saturating_sub(3))));
    frame.render_widget(
        Paragraph::new(Line::from(header)),
        Rect::new(area.x, area.y, area.width, 1),
    );
}

fn render_legend(frame: &mut Frame<'_>, area: Rect, legend: &[Line<'static>]) {
    for (index, line) in legend.iter().cloned().enumerate() {
        frame.render_widget(
            Paragraph::new(line),
            Rect::new(
                area.x,
                area.y + 1 + u16::try_from(index).unwrap_or(u16::MAX),
                area.width,
                1,
            ),
        );
    }
}

fn address_width(area: Rect, view: &MatrixView) -> usize {
    let content_width = view
        .rows
        .iter()
        .map(|row| {
            let expansion = row
                .group
                .as_ref()
                .map_or(if row.child { 2 } else { 0 }, |_| 4);
            1 + expansion + Line::from(row.address.as_str()).width()
        })
        .max()
        .unwrap_or(0)
        .max(Line::from("Address").width());
    let max_width = usize::from(area.width)
        .saturating_sub(WHY_WIDTH + MIN_CELL_WIDTH + COLUMN_GAP * 2)
        .clamp(MIN_ADDRESS_WIDTH, MAX_ADDRESS_WIDTH);
    content_width.clamp(MIN_ADDRESS_WIDTH, max_width)
}

fn column_widths(state: &EnvironmentSession, view: &MatrixView) -> Vec<usize> {
    view.environments
        .iter()
        .enumerate()
        .map(|(column, environment)| {
            let plan = &state.plans()[*environment];
            let (first_total, second_total) = total_text(plan);
            let widest_cell = view
                .rows
                .iter()
                .map(|row| Line::from(cell_text(&row.cells[column], row.group.is_some())).width())
                .max()
                .unwrap_or(0);
            (Line::from(name(plan).as_str()).width() + 2)
                .max(widest_cell)
                .max(Line::from(first_total.as_str()).width())
                .max(Line::from(second_total.as_str()).width())
                .max(MIN_CELL_WIDTH)
                + COLUMN_GAP * 3
        })
        .collect()
}

fn visible_columns(
    view: &mut MatrixView,
    widths: &[usize],
    budget: usize,
    selected_environment: usize,
) -> Vec<(usize, usize)> {
    if widths.is_empty() || budget == 0 {
        view.first_column = 0;
        return Vec::new();
    }
    let selected = selected_environment.min(widths.len() - 1);
    let mut first = view.first_column.min(widths.len() - 1);
    if selected < first {
        first = selected;
    }
    let mut last = first;
    let mut used = 0;
    while last < widths.len() {
        let remaining = budget.saturating_sub(used);
        if remaining == 0 {
            break;
        }
        if last > first && widths[last] > remaining {
            break;
        }
        let width = widths[last].min(remaining);
        used += width;
        last += 1;
        if width < widths[last - 1] {
            break;
        }
    }
    if selected >= last {
        first = selected;
        used = widths[selected].min(budget);
        while first > 0 && widths[first - 1] <= budget.saturating_sub(used) {
            first -= 1;
            used += widths[first];
        }
        last = selected + 1;
        while last < widths.len() && widths[last] <= budget.saturating_sub(used) {
            used += widths[last];
            last += 1;
        }
    }
    view.first_column = first;
    (first..last)
        .map(|index| {
            let width = if index == first {
                widths[index].min(budget)
            } else {
                widths[index]
            };
            (index, width)
        })
        .collect()
}

fn total_text(plan: &EnvironmentPlan) -> (String, String) {
    plan.review().map_or_else(
        || {
            (
                status(plan).split(':').next().unwrap_or("?").to_owned(),
                String::new(),
            )
        },
        |review| {
            let counts = review.review().metadata();
            (
                format!(
                    "+{} ~{} -{}",
                    counts.additions(),
                    counts.changes(),
                    counts.deletions()
                ),
                format!("{} replace", counts.replacements()),
            )
        },
    )
}

fn total_lines(
    state: &EnvironmentSession,
    view: &MatrixView,
    columns: &[(usize, usize)],
    address_width: usize,
) -> [Line<'static>; 2] {
    let mut lines = [Line::default(), Line::default()];
    for (total_line, line) in lines.iter_mut().enumerate() {
        let mut spans = vec![Span::styled(
            fit(
                if total_line == 0 { "Total" } else { "" },
                address_width,
                false,
            ),
            theme::accent_style(),
        )];
        for &(index, column_width) in columns {
            let environment = view.environments[index];
            let totals = total_text(&state.plans()[environment]);
            let text = if total_line == 0 { totals.0 } else { totals.1 };
            spans.push(Span::styled(
                fit(&text, column_width - COLUMN_GAP, false),
                theme::secondary_style(),
            ));
            spans.push(Span::raw(" "));
        }
        *line = Line::from(spans);
    }
    lines
}

fn symbol_legend(width: u16) -> Vec<Line<'static>> {
    if width < 50 {
        vec![
            Line::styled("blank: absent   .: unchanged", theme::secondary_style()),
            Line::styled("?: plan unavailable", theme::secondary_style()),
        ]
    } else {
        vec![Line::styled(
            "blank: absent   .: unchanged   ?: plan unavailable",
            theme::secondary_style(),
        )]
    }
}

fn row_line(
    row: &Row,
    view: &MatrixView,
    columns: &[(usize, usize)],
    address_width: usize,
    selected_environment: usize,
) -> Line<'static> {
    let expansion = row
        .group
        .as_ref()
        .map_or(if row.child { "  " } else { "" }, |id| {
            if view.expanded.contains(id) {
                "[-] "
            } else {
                "[+] "
            }
        });
    let mut spans = vec![Span::styled(
        format!(
            "{} ",
            fit(
                &format!("{expansion}{}", row.address),
                address_width.saturating_sub(1),
                true
            )
        ),
        theme::body_style(),
    )];
    for &(index, column_width) in columns {
        let cell = &row.cells[index];
        let (text, padding) = fit_parts(
            &cell_text(cell, row.group.is_some()),
            column_width - COLUMN_GAP,
            false,
        );
        let style = if index == selected_environment {
            theme::selected_row_style()
        } else {
            theme::body_style()
        };
        spans.push(Span::styled(
            format!("{text}{} ", " ".repeat(padding)),
            style,
        ));
    }
    let reason = match row.difference {
        Some(DifferenceReason::Action) => "action",
        Some(DifferenceReason::Attrs) => "attrs",
        Some(DifferenceReason::Missing) => "missing",
        Some(DifferenceReason::Unknown) => "unknown",
        Some(DifferenceReason::Value) => "value",
        None => "",
    };
    spans.push(Span::styled(" ", theme::body_style()));
    spans.push(Span::styled(
        fit(reason, WHY_WIDTH, false),
        theme::secondary_style(),
    ));
    Line::from(spans)
}

fn cell_text(cell: &MatrixCell, grouped: bool) -> String {
    let symbol = match &cell.state {
        CellState::NoOp => ".",
        CellState::Missing => "",
        CellState::Unavailable => "?",
        CellState::Change { actions, kind } => match kind {
            ResourceChangeKind::Create => "+",
            ResourceChangeKind::Update => "~",
            ResourceChangeKind::Delete => "-",
            ResourceChangeKind::Replace
                if actions.starts_with(&[PlanAction::Create, PlanAction::Delete]) =>
            {
                "+/-"
            }
            ResourceChangeKind::Replace => "-/+",
            ResourceChangeKind::Read => "read",
            ResourceChangeKind::Move => "move",
            ResourceChangeKind::Import => "import",
            ResourceChangeKind::NoOp => ".",
            ResourceChangeKind::Unknown | ResourceChangeKind::Unsupported => "?",
        },
    };
    if grouped && matches!(cell.state, CellState::Change { .. }) {
        if cell.members.is_empty() {
            String::new()
        } else {
            format!("{symbol} {}", cell.members.len())
        }
    } else {
        symbol.to_owned()
    }
}

fn fit(text: &str, width: usize, suffix: bool) -> String {
    let (value, padding) = fit_parts(text, width, suffix);
    format!("{value}{}", " ".repeat(padding))
}

fn fit_parts(text: &str, width: usize, suffix: bool) -> (String, usize) {
    let mut value = text.to_owned();
    if Line::from(value.as_str()).width() > width {
        let limit = width.saturating_sub(1);
        while Line::from(value.as_str()).width() > limit {
            if suffix {
                value.remove(0);
            } else {
                value.pop();
            }
        }
        if width > 0 {
            value = if suffix {
                format!("…{value}")
            } else {
                format!("{value}…")
            };
        }
    }
    let padding = width.saturating_sub(Line::from(value.as_str()).width());
    (value, padding)
}
