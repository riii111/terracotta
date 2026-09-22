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

use super::context::{relative_directory, target, truncate_middle};

const GAP: &str = "  ";

pub(crate) fn render(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>) {
    frame.render_widget(Paragraph::new(lines).style(theme::secondary_style()), area);
}

pub(crate) fn render_review(frame: &mut Frame<'_>, area: Rect, review: &PlanReview) {
    render(frame, area, vec![review_header_line(review, area.width)]);
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

fn review_header_line(review: &PlanReview, width: u16) -> Line<'static> {
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
    fit_header(
        &[
            target_name,
            format!("ws:{workspace}"),
            format!("{} {version}", context.tool_name()),
            relative_directory(context.cwd_path(), context.launch_root_path()),
        ],
        width,
    )
}

fn fit_header(parts: &[String], width: u16) -> Line<'static> {
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
                    truncate_header_part(part, allocation, index == parts.len().saturating_sub(1))
                })
                .collect::<Vec<_>>()
                .join(GAP)
        }
    };
    Line::from(Span::styled(value, theme::secondary_style()))
}

fn truncate_header_part(value: &str, max_width: usize, is_directory: bool) -> String {
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
    fn narrow_review_header_keeps_workspace_and_tool_fields() {
        let line = fit_header(
            &[
                "very-long-target-name-for-review [PROD]".to_owned(),
                "ws:default".to_owned(),
                "terraform 1.9.0".to_owned(),
                "./environments/production".to_owned(),
            ],
            50,
        );
        let value = line.to_string();

        assert!(value.contains("ws:default"));
        assert!(value.contains("terraform 1.9.0"));
        assert!(value.contains("./"));
    }
}
