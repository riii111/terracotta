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
            Paragraph::new("Resize to view the matrix. ? help   q quit")
                .style(theme::overview_muted_style()),
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
            lines.push(Line::styled(
                title,
                if row.difference.is_some() {
                    theme::overview_accent_style()
                } else {
                    theme::overview_text_style()
                },
            ));
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
    lines.push(Line::default());
    lines.extend(total_lines(state, view, columns, address_width));
    let legend = symbol_legend(area.width);
    lines.extend(legend);
    let body = Rect::new(
        area.x,
        area.y.saturating_add(2),
        area.width,
        area.height.saturating_sub(2),
    );
    view.vertical = view
        .vertical
        .min(lines.len().saturating_sub(usize::from(body.height)));
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::overview_text_style())
            .scroll((u16::try_from(view.vertical).unwrap_or(u16::MAX), 0)),
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
    let mut header = vec![Span::styled("Address", theme::overview_muted_style())];
    header.push(Span::styled(
        " ".repeat(address_width.saturating_sub(7)),
        theme::overview_muted_style(),
    ));
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
        let style = if selected {
            theme::overview_header_selected_style().add_modifier(Modifier::BOLD)
        } else {
            theme::overview_muted_style()
        };
        header.push(Span::styled(
            format!("{marker}{label}{} ", " ".repeat(padding)),
            style,
        ));
    }
    header.push(Span::styled(" ", theme::overview_muted_style()));
    header.push(Span::styled("why", theme::overview_muted_style()));
    header.push(Span::styled(
        " ".repeat(WHY_WIDTH.saturating_sub(3)),
        theme::overview_muted_style(),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(header)).style(theme::overview_text_style()),
        Rect::new(area.x, area.y, area.width, 1),
    );
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
            let total = total_text(plan);
            let widest_cell = view
                .rows
                .iter()
                .map(|row| Line::from(cell_text(&row.cells[column], row.group.is_some())).width())
                .max()
                .unwrap_or(0);
            (Line::from(name(plan).as_str()).width() + 2)
                .max(widest_cell)
                .max(Line::from(total.as_str()).width())
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

fn total_text(plan: &EnvironmentPlan) -> String {
    plan.review().map_or_else(
        || status(plan).split(':').next().unwrap_or("?").to_owned(),
        |review| {
            let counts = review.review().metadata();
            let mut parts = Vec::new();
            if counts.additions() > 0 {
                parts.push(format!("+{}", counts.additions()));
            }
            if counts.changes() > 0 {
                parts.push(format!("~{}", counts.changes()));
            }
            if counts.deletions() > 0 {
                parts.push(format!("-{}", counts.deletions()));
            }
            if counts.replacements() > 0 {
                parts.push(format!("{} replace", counts.replacements()));
            }
            if parts.is_empty() {
                if counts.nonstandard_changes() > 0 {
                    "Other changes".to_owned()
                } else if counts.has_changes() {
                    "Outputs changed".to_owned()
                } else {
                    "No changes".to_owned()
                }
            } else {
                parts.join(" ")
            }
        },
    )
}

fn total_lines(
    state: &EnvironmentSession,
    view: &MatrixView,
    columns: &[(usize, usize)],
    address_width: usize,
) -> [Line<'static>; 1] {
    let style = theme::overview_total_style();
    let mut spans = vec![Span::styled(fit("Total", address_width, false), style)];
    for &(index, column_width) in columns {
        let environment = view.environments[index];
        let text = total_text(&state.plans()[environment]);
        let text_width = column_width.saturating_sub(COLUMN_GAP);
        spans.extend(total_spans(&state.plans()[environment], &text, text_width));
        spans.push(Span::styled(" ".repeat(COLUMN_GAP), style));
    }
    spans.push(Span::styled(" ".repeat(WHY_WIDTH + 1), style));
    let table_width = address_width
        .saturating_add(1)
        .saturating_add(columns.iter().map(|(_, width)| *width).sum::<usize>())
        .saturating_add(WHY_WIDTH);
    let line_width = spans.iter().map(Span::width).sum::<usize>();
    if line_width < table_width {
        spans.push(Span::styled(" ".repeat(table_width - line_width), style));
    }
    [Line::from(spans)]
}

fn total_spans(plan: &EnvironmentPlan, text: &str, width: usize) -> Vec<Span<'static>> {
    let Some(review) = plan.review() else {
        return vec![Span::styled(
            fit(text, width, false),
            theme::overview_total_muted_style(),
        )];
    };
    let counts = review.review().metadata();
    if counts.additions() == 0
        && counts.changes() == 0
        && counts.deletions() == 0
        && counts.replacements() == 0
    {
        return vec![Span::styled(
            fit(text, width, false),
            theme::overview_total_muted_style(),
        )];
    }
    let mut spans = Vec::new();
    let mut used = 0;
    for (value, style) in [
        (
            (counts.additions() > 0).then(|| format!("+{}", counts.additions())),
            theme::overview_total_add_style(),
        ),
        (
            (counts.changes() > 0).then(|| format!("~{}", counts.changes())),
            theme::overview_total_update_style(),
        ),
        (
            (counts.deletions() > 0).then(|| format!("-{}", counts.deletions())),
            theme::overview_total_destroy_style(),
        ),
        (
            (counts.replacements() > 0).then(|| format!("{} replace", counts.replacements())),
            theme::overview_total_replace_style(),
        ),
    ] {
        let Some(value) = value else { continue };
        if used >= width {
            break;
        }
        if !spans.is_empty() {
            spans.push(Span::styled(" ", theme::overview_total_style()));
            used += 1;
        }
        let (value, _) = fit_parts(&value, width.saturating_sub(used), false);
        used += Line::from(value.as_str()).width();
        spans.push(Span::styled(value, style));
    }
    if used < width {
        spans.push(Span::styled(
            " ".repeat(width - used),
            theme::overview_total_style(),
        ));
    }
    spans
}

fn symbol_legend(width: u16) -> Vec<Line<'static>> {
    if width < 50 {
        vec![
            Line::styled(
                "blank: absent   .: unchanged",
                theme::overview_muted_style(),
            ),
            Line::styled("?: plan unavailable", theme::overview_muted_style()),
        ]
    } else {
        vec![Line::styled(
            "blank: absent   .: unchanged   ?: plan unavailable",
            theme::overview_muted_style(),
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
        theme::overview_text_style(),
    )];
    for &(index, column_width) in columns {
        let cell = &row.cells[index];
        let (text, padding) = fit_parts(
            &cell_text(cell, row.group.is_some()),
            column_width - COLUMN_GAP,
            false,
        );
        let style = if index == selected_environment {
            theme::overview_selected_column_style()
        } else {
            theme::overview_text_style()
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
    spans.push(Span::styled(" ", theme::overview_text_style()));
    spans.push(Span::styled(
        fit(reason, WHY_WIDTH, false),
        theme::overview_muted_style(),
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
