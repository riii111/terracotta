use std::path::Path;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{
    execution::{ExecutionContext, ExecutionContextValue},
    review::PlanReview,
};
use crate::ui::theme;

use super::context::{display_width, relative_directory, take_from_start, target, truncate_middle};

const REVIEW_HEADER_SEPARATOR: &str = " ";
const PRODUCTION_SUFFIX: &str = " [PROD]";
const GAP: &str = "  ";

struct HeaderField {
    label: &'static str,
    value: String,
    minimum_value_width: usize,
    kind: HeaderFieldKind,
}

#[derive(Clone, Copy)]
enum HeaderFieldKind {
    Target,
    Workspace,
    Tool,
    Directory,
}

pub(crate) fn render(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>) {
    frame.render_widget(Paragraph::new(lines).style(theme::secondary_style()), area);
}

pub(crate) fn render_review(frame: &mut Frame<'_>, area: Rect, review: &PlanReview) {
    render(
        frame,
        area,
        vec![compact_review_header_line(review, area.width)],
    );
}

pub(crate) fn render_plan_review(frame: &mut Frame<'_>, area: Rect, review: &PlanReview) {
    let mut lines = vec![plan_review_header_line(review, area.width)];
    if area.height > 1 {
        lines.push(plan_review_changes_line(review));
    }
    render(frame, area, lines);
}

pub(crate) fn render_execution(frame: &mut Frame<'_>, area: Rect, context: &ExecutionContext) {
    let workspace = match context.workspace() {
        ExecutionContextValue::Known(workspace) => Some(workspace.as_str()),
        ExecutionContextValue::Loading => Some("loading..."),
    };
    render(
        frame,
        area,
        vec![header_line(context.cwd_path(), workspace, area.width)],
    );
}

fn plan_review_header_line(review: &PlanReview, width: u16) -> Line<'static> {
    let context = review.context();
    let target_name = match context.display_name() {
        ExecutionContextValue::Known(name) => {
            if context.is_production() == Some(true) {
                format!("{name} [PROD]")
            } else {
                name.clone()
            }
        }
        ExecutionContextValue::Loading => target(review.root()),
    };
    let workspace = match context.workspace() {
        ExecutionContextValue::Known(value) => value.as_str(),
        ExecutionContextValue::Loading => review.workspace(),
    };
    let version = match context.tool_version() {
        ExecutionContextValue::Known(value) => value.as_str(),
        ExecutionContextValue::Loading => "loading...",
    };
    let tool = format!("{} {version}", context.tool_name());
    let directory = relative_directory(context.cwd_path(), context.launch_root_path());
    fit_header(
        &[
            HeaderField {
                label: "Target: ",
                minimum_value_width: 8,
                value: target_name,
                kind: HeaderFieldKind::Target,
            },
            HeaderField {
                label: "Workspace: ",
                minimum_value_width: display_width(workspace).min(7),
                value: workspace.to_owned(),
                kind: HeaderFieldKind::Workspace,
            },
            HeaderField {
                label: "Tool: ",
                minimum_value_width: display_width(&tool),
                value: tool,
                kind: HeaderFieldKind::Tool,
            },
            HeaderField {
                label: "Dir: ",
                minimum_value_width: display_width(&directory).min(7),
                value: directory,
                kind: HeaderFieldKind::Directory,
            },
        ],
        width,
    )
}

fn plan_review_changes_line(review: &PlanReview) -> Line<'static> {
    let counts = review.metadata();
    let mut line = Line::from(Span::styled("Changes", theme::secondary_style()));
    let mut append = |text: String, style| {
        line.push_span(Span::styled("  ", theme::secondary_style()));
        line.push_span(Span::styled(text, style));
    };
    if counts.additions() > 0 {
        append(
            format!("+{} add", counts.additions()),
            theme::success_style(),
        );
    }
    if counts.changes() > 0 {
        append(
            format!("~{} update", counts.changes()),
            theme::warning_style(),
        );
    }
    if counts.replacements() > 0 {
        append(
            format!("{} replace", counts.replacements()),
            theme::overview_total_replace_style(),
        );
    }
    if counts.deletions() > 0 {
        append(
            format!("-{} destroy", counts.deletions()),
            theme::error_style(),
        );
    }
    if counts.additions() == 0
        && counts.changes() == 0
        && counts.replacements() == 0
        && counts.deletions() == 0
    {
        line.push_span(Span::styled("  none", theme::secondary_style()));
    }
    line
}

fn compact_review_header_line(review: &PlanReview, width: u16) -> Line<'static> {
    let context = review.context();
    let target_name = match context.display_name() {
        ExecutionContextValue::Known(name) => {
            if context.is_production() == Some(true) {
                format!("{name} [PROD]")
            } else {
                name.clone()
            }
        }
        ExecutionContextValue::Loading => target(review.root()),
    };
    let workspace = match context.workspace() {
        ExecutionContextValue::Known(value) => value.as_str(),
        ExecutionContextValue::Loading => review.workspace(),
    };
    let version = match context.tool_version() {
        ExecutionContextValue::Known(value) => value.as_str(),
        ExecutionContextValue::Loading => "loading...",
    };
    fit_compact_header(
        &[
            target_name,
            format!("ws:{workspace}"),
            format!("{} {version}", context.tool_name()),
            relative_directory(context.cwd_path(), context.launch_root_path()),
        ],
        width,
    )
}

fn fit_header(fields: &[HeaderField], width: u16) -> Line<'static> {
    let width = usize::from(width);
    let separator_width = Line::from(REVIEW_HEADER_SEPARATOR).width();
    let full_width = fields
        .iter()
        .map(header_field_width)
        .sum::<usize>()
        .saturating_add(separator_width.saturating_mul(fields.len().saturating_sub(1)));
    let allocations = if full_width <= width {
        fields
            .iter()
            .map(|field| (field, header_field_width(field)))
            .collect::<Vec<_>>()
    } else {
        allocate_header_fields(fields, width, separator_width)
    };
    let value = allocations
        .into_iter()
        .map(|(field, allocation)| format_header_field(field, allocation))
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>()
        .join(REVIEW_HEADER_SEPARATOR);
    Line::from(Span::styled(value, theme::secondary_style()))
}

fn allocate_header_fields(
    fields: &[HeaderField],
    width: usize,
    separator_width: usize,
) -> Vec<(&HeaderField, usize)> {
    let Some(target) = fields.first() else {
        return Vec::new();
    };
    let mut selected = vec![target];
    for candidate in fields.iter().skip(1) {
        let candidate_minimum = selected
            .iter()
            .map(|field| header_field_minimum_width(field))
            .sum::<usize>()
            .saturating_add(header_field_minimum_width(candidate))
            .saturating_add(separator_width.saturating_mul(selected.len()));
        if candidate_minimum <= width {
            selected.push(candidate);
        }
    }

    let available = width.saturating_sub(separator_width.saturating_mul(selected.len() - 1));
    let mut allocations = selected
        .iter()
        .map(|field| header_field_minimum_width(field).min(width))
        .collect::<Vec<_>>();
    let mut remaining = available.saturating_sub(allocations.iter().sum());
    for (allocation, field) in allocations.iter_mut().zip(&selected) {
        let extra = remaining.min(header_field_width(field).saturating_sub(*allocation));
        *allocation += extra;
        remaining -= extra;
    }
    selected.into_iter().zip(allocations).collect()
}

fn header_field_minimum_width(field: &HeaderField) -> usize {
    display_width(field.label)
        .saturating_add(field.minimum_value_width.min(display_width(&field.value)))
}

fn header_field_width(field: &HeaderField) -> usize {
    display_width(field.label).saturating_add(display_width(&field.value))
}

fn format_header_field(field: &HeaderField, allocation: usize) -> String {
    let label_width = display_width(field.label);
    if allocation < label_width {
        return truncate_middle(field.label.trim_end(), allocation);
    }
    let value_width = allocation.saturating_sub(label_width);
    let value = match field.kind {
        HeaderFieldKind::Target => truncate_target(&field.value, value_width),
        HeaderFieldKind::Workspace => truncate_middle(&field.value, value_width),
        HeaderFieldKind::Tool => truncate_tool(&field.value, value_width),
        HeaderFieldKind::Directory => truncate_directory(&field.value, value_width),
    };
    format!("{}{value}", field.label)
}

fn truncate_tool(value: &str, max_width: usize) -> String {
    let Some((name, version)) = value.split_once(' ') else {
        return truncate_middle(value, max_width);
    };
    let name_width = display_width(name);
    if max_width <= name_width {
        return take_from_start(name, max_width);
    }
    let version_width = max_width.saturating_sub(name_width).saturating_sub(1);
    format!("{name} {}", truncate_middle(version, version_width))
}

fn truncate_target(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_owned();
    }
    if let Some(prefix) = value.strip_suffix(PRODUCTION_SUFFIX) {
        let suffix_width = display_width(PRODUCTION_SUFFIX);
        if max_width >= suffix_width {
            return format!(
                "{}{PRODUCTION_SUFFIX}",
                truncate_middle(prefix, max_width - suffix_width)
            );
        }
    }
    truncate_middle(value, max_width)
}

fn truncate_directory(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_owned();
    }
    if let Some(relative) = value.strip_prefix("./") {
        if max_width <= 2 {
            return take_from_start("./", max_width);
        }
        return format!("./{}", truncate_middle(relative, max_width - 2));
    }
    truncate_middle(value, max_width)
}

fn fit_compact_header(parts: &[String], width: u16) -> Line<'static> {
    let width = usize::from(width);
    let separator_width = Line::from(GAP).width();
    let full = parts.join(GAP);
    let value = if Line::from(full.as_str()).width() <= width {
        full
    } else {
        let separators = separator_width.saturating_mul(parts.len().saturating_sub(1));
        let available = width.saturating_sub(separators);
        let minimums = parts
            .iter()
            .enumerate()
            .map(|(index, part)| match index {
                0 => Line::from(part.as_str()).width().min(12),
                index if index == parts.len().saturating_sub(1) => 5,
                1 => Line::from(part.as_str()).width().min(10),
                2 => Line::from(part.as_str()).width().min(15),
                _ => 4,
            })
            .collect::<Vec<_>>();
        if available < minimums.iter().sum() {
            truncate_middle(&full, width)
        } else {
            let mut allocations = minimums;
            let mut remaining = available.saturating_sub(allocations.iter().sum());
            for (allocation, part) in allocations.iter_mut().zip(parts) {
                let extra = remaining.min(
                    Line::from(part.as_str())
                        .width()
                        .saturating_sub(*allocation),
                );
                *allocation += extra;
                remaining -= extra;
            }
            parts
                .iter()
                .zip(allocations)
                .enumerate()
                .map(|(index, (part, allocation))| {
                    truncate_compact_header_part(
                        part,
                        allocation,
                        index == parts.len().saturating_sub(1),
                    )
                })
                .collect::<Vec<_>>()
                .join(GAP)
        }
    };
    Line::from(Span::styled(value, theme::secondary_style()))
}

fn truncate_compact_header_part(value: &str, max_width: usize, is_directory: bool) -> String {
    if is_directory && value.starts_with("./") && Line::from(value).width() > max_width {
        if max_width <= 2 {
            return truncate_middle(value, max_width);
        }
        return format!("./{}", truncate_middle(&value[2..], max_width - 2));
    }
    truncate_middle(value, max_width)
}

fn header_line(path: &Path, workspace: Option<&str>, width: u16) -> Line<'static> {
    const PREFIX: &str = "Terracotta | ";
    const GAP: usize = 2;
    let width = usize::from(width);
    let right = workspace
        .filter(|workspace| *workspace != "default")
        .map(|workspace| format!("workspace: {workspace}"));
    let right_width = right
        .as_deref()
        .map_or(0, |value| Line::from(value).width());
    let left_width = width.saturating_sub(right_width + usize::from(right.is_some()) * GAP);
    let prefix_width = Line::from(PREFIX).width();
    let left = if left_width <= prefix_width {
        truncate_middle(PREFIX, left_width)
    } else {
        format!(
            "{PREFIX}{}",
            truncate_middle(&target(path), left_width - prefix_width)
        )
    };
    let value = right.map_or_else(
        || left.clone(),
        |right| {
            let gap = width.saturating_sub(Line::from(left.as_str()).width() + right_width);
            format!("{left}{}{right}", " ".repeat(gap))
        },
    );
    Line::from(Span::styled(value, theme::secondary_style()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_header_labels_context_and_preserves_target_identity_at_eighty_columns() {
        let fields = [
            HeaderField {
                label: "Target: ",
                minimum_value_width: 8,
                value: "very-long-target-name-for-review [PROD]".to_owned(),
                kind: HeaderFieldKind::Target,
            },
            HeaderField {
                label: "Workspace: ",
                minimum_value_width: 7,
                value: "default".to_owned(),
                kind: HeaderFieldKind::Workspace,
            },
            HeaderField {
                label: "Tool: ",
                minimum_value_width: 15,
                value: "terraform 1.9.0".to_owned(),
                kind: HeaderFieldKind::Tool,
            },
            HeaderField {
                label: "Dir: ",
                minimum_value_width: 7,
                value: "./environments/production".to_owned(),
                kind: HeaderFieldKind::Directory,
            },
        ];
        let line = fit_header(&fields, 80);
        let value = line.to_string();

        assert!(value.starts_with("Target: "), "{value}");
        assert!(value.contains("[PROD]"), "{value}");
        assert!(value.contains("view [PROD]"), "{value}");
        assert!(value.contains("Workspace:"), "{value}");
        assert!(value.contains("Tool: terraform"), "{value}");
        assert!(value.contains("Dir: ./"), "{value}");
        assert!(line.width() <= 80, "{value}");
    }

    #[test]
    fn narrow_review_header_keeps_target_label_when_context_fields_do_not_fit() {
        let line = fit_header(
            &[
                HeaderField {
                    label: "Target: ",
                    minimum_value_width: 8,
                    value: "production [PROD]".to_owned(),
                    kind: HeaderFieldKind::Target,
                },
                HeaderField {
                    label: "Workspace: ",
                    minimum_value_width: 7,
                    value: "default".to_owned(),
                    kind: HeaderFieldKind::Workspace,
                },
                HeaderField {
                    label: "Tool: ",
                    minimum_value_width: 15,
                    value: "terraform 1.9.0".to_owned(),
                    kind: HeaderFieldKind::Tool,
                },
                HeaderField {
                    label: "Dir: ",
                    minimum_value_width: 7,
                    value: "./environments/production".to_owned(),
                    kind: HeaderFieldKind::Directory,
                },
            ],
            24,
        );
        let value = line.to_string();

        assert!(value.starts_with("Target: "), "{value}");
        assert!(value.contains("PROD"), "{value}");
        assert!(line.width() <= 24, "{value}");
    }

    #[test]
    fn truncated_production_targets_keep_a_distinguishing_suffix() {
        let first = truncate_target("very-long-target-name-alpha [PROD]", 20);
        let second = truncate_target("very-long-target-name-bravo [PROD]", 20);

        assert_ne!(first, second);
        assert!(first.ends_with(PRODUCTION_SUFFIX), "{first}");
        assert!(second.ends_with(PRODUCTION_SUFFIX), "{second}");
    }

    #[test]
    fn very_narrow_review_header_stays_within_the_available_width() {
        let line = fit_header(
            &[HeaderField {
                label: "Target: ",
                minimum_value_width: 8,
                value: "production [PROD]".to_owned(),
                kind: HeaderFieldKind::Target,
            }],
            10,
        );

        assert!(line.width() <= 10, "{line}");
        assert!(line.to_string().starts_with("Target:"));
    }
}
