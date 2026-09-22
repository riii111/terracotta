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
        EnvironmentSession,
        comparison::{CellState, ComparisonScope, DifferenceReason},
    },
    plan::{PlanAction, ResourceChangeKind},
};
use crate::ui::{
    shell::environments::{name, status},
    theme,
};

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
    let address_width = (usize::from(area.width) * 2 / 5).clamp(14, 52);
    let available = usize::from(area.width).saturating_sub(address_width + 8);
    let count = (available / 12).max(1).min(state.plans().len().max(1));
    let cell_width = available / count;
    if selected_environment < view.first_column {
        view.first_column = selected_environment;
    }
    if selected_environment >= view.first_column + count {
        view.first_column = selected_environment + 1 - count;
    }
    view.first_column = view
        .first_column
        .min(state.plans().len().saturating_sub(count));
    let columns = view.first_column..(view.first_column + count).min(state.plans().len());
    let mut header = vec![Span::styled(
        fit("Address", address_width, false),
        theme::secondary_style(),
    )];
    for index in columns.clone() {
        header.push(Span::styled(
            fit(&name(&state.plans()[index]), cell_width, false),
            if index == selected_environment {
                theme::search_match_style()
            } else {
                theme::secondary_style()
            },
        ));
    }
    header.push(Span::styled("why", theme::secondary_style()));
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
            columns.clone(),
            address_width,
            cell_width,
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
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(3),
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
    render_totals(frame, area, state, columns, address_width, cell_width);
}

fn render_totals(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &EnvironmentSession,
    columns: std::ops::Range<usize>,
    address_width: usize,
    cell_width: usize,
) {
    for total_line in 0..2 {
        let mut spans = vec![Span::styled(
            fit(
                if total_line == 0 { "Total" } else { "" },
                address_width,
                false,
            ),
            theme::accent_style(),
        )];
        for index in columns.clone() {
            let plan = &state.plans()[index];
            let text = plan.review().map_or_else(
                || {
                    if total_line == 0 {
                        status(plan).split(':').next().unwrap_or("?").to_owned()
                    } else {
                        String::new()
                    }
                },
                |review| {
                    let counts = review.review().metadata();
                    if total_line == 0 {
                        format!(
                            "+{} ~{} -{}",
                            counts.additions(),
                            counts.changes(),
                            counts.deletions()
                        )
                    } else {
                        format!("{} replace", counts.replacements())
                    }
                },
            );
            spans.push(Span::styled(
                fit(&text, cell_width, false),
                theme::secondary_style(),
            ));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(area.x, area.bottom() - 2 + total_line, area.width, 1),
        );
    }
}

fn row_line(
    row: &Row,
    selected: bool,
    view: &MatrixView,
    columns: std::ops::Range<usize>,
    address_width: usize,
    cell_width: usize,
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
    for index in columns {
        let cell = &row.cells[index];
        let text = cell_text(cell, row.group.is_some());
        let style = if selected && index == selected_environment {
            theme::body_style().add_modifier(Modifier::REVERSED)
        } else {
            theme::body_style()
        };
        spans.push(Span::styled(fit(&text, cell_width, false), style));
    }
    let reason = match row.difference {
        Some(DifferenceReason::Action) => "action",
        Some(DifferenceReason::Attrs) => "attrs",
        Some(DifferenceReason::Missing) => "missing",
        Some(DifferenceReason::Unknown) => "unknown",
        Some(DifferenceReason::Value) => "value",
        None => "",
    };
    spans.push(Span::styled(reason, theme::secondary_style()));
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
    format!("{value}{}", " ".repeat(padding))
}
