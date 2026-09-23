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
    let column_widths = column_widths(state, view);
    let column_budget =
        usize::from(area.width).saturating_sub(address_width + WHY_WIDTH + COLUMN_GAP);
    let columns = visible_columns(view, &column_widths, column_budget, selected_environment);

    let mut header = vec![Span::styled("Address", theme::secondary_style())];
    header.push(Span::raw(" ".repeat(address_width.saturating_sub(7))));
    for &(index, column_width) in &columns {
        let label = if index == selected_environment {
            format!("> {}", name(&state.plans()[index]))
        } else {
            name(&state.plans()[index])
        };
        let (label, padding) = fit_parts(&label, column_width - COLUMN_GAP, false);
        header.push(Span::styled(
            label,
            if index == selected_environment {
                theme::search_match_style()
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

    let partial = !matches!(state.overview().scope, ComparisonScope::All { .. });
    let mut lines = Vec::new();
    let mut row_lines = Vec::new();
    let mut section = None;
    for (index, row) in view.rows.iter().enumerate() {
        if !row.child && section != Some(row.difference.is_some()) {
            section = Some(row.difference.is_some());
            let title = if row.difference.is_some() {
                "Differs across envs"
            } else {
                "Same change across envs"
            };
            lines.push(Line::styled(
                format!("{title}{}", if partial { " (Ready only)" } else { "" }),
                theme::accent_style(),
            ));
        }
        row_lines.push(lines.len());
        lines.push(row_line(
            row,
            index == view.selected,
            view,
            &columns,
            address_width,
            selected_environment,
        ));
    }
    if lines.is_empty() {
        lines.push(Line::from(
            if matches!(state.overview().scope, ComparisonScope::Waiting) {
                "Waiting for environment plans."
            } else {
                "No matching resource changes. v opens the full plan."
            },
        ));
    }
    lines.extend(total_lines(state, &columns, address_width));
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    if let Some(&line) = row_lines.get(view.selected) {
        if line < view.vertical {
            view.vertical = line;
        }
        if line >= view.vertical + usize::from(body.height) {
            view.vertical = line + 1 - usize::from(body.height);
        }
    }
    view.vertical = view
        .vertical
        .min(lines.len().saturating_sub(usize::from(body.height)));
    frame.render_widget(
        Paragraph::new(lines).scroll((u16::try_from(view.vertical).unwrap_or(u16::MAX), 0)),
        body,
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
            3 + expansion + Line::from(row.address.as_str()).width()
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
    state
        .plans()
        .iter()
        .enumerate()
        .map(|(index, plan)| {
            let (first_total, second_total) = total_text(plan);
            let widest_cell = view
                .rows
                .iter()
                .map(|row| Line::from(cell_text(&row.cells[index], row.group.is_some())).width())
                .max()
                .unwrap_or(0);
            (Line::from(name(plan).as_str()).width() + 2)
                .max(widest_cell)
                .max(Line::from(first_total.as_str()).width())
                .max(Line::from(second_total.as_str()).width())
                .max(MIN_CELL_WIDTH)
                + COLUMN_GAP
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
            let totals = total_text(&state.plans()[index]);
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

fn row_line(
    row: &Row,
    selected: bool,
    view: &MatrixView,
    columns: &[(usize, usize)],
    address_width: usize,
    selected_environment: usize,
) -> Line<'static> {
    let marker = if selected { ">" } else { " " };
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
            "{marker} {} ",
            fit(
                &format!("{expansion}{}", row.address),
                address_width.saturating_sub(3),
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
        let style = if selected && index == selected_environment && !text.is_empty() {
            theme::body_style().add_modifier(Modifier::REVERSED)
        } else {
            theme::body_style()
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(format!("{} ", " ".repeat(padding))));
    }
    let reason = match row.difference {
        Some(DifferenceReason::Action) => "action",
        Some(DifferenceReason::Attrs) => "attrs",
        Some(DifferenceReason::Missing) => "missing",
        Some(DifferenceReason::Unknown) => "unknown",
        Some(DifferenceReason::Value) => "value",
        None => "",
    };
    spans.push(Span::raw(" "));
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
