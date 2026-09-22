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
        let suffix = parts.last().map_or("", String::as_str);
        let prefix = parts[..parts.len().saturating_sub(1)].join(GAP);
        let prefix_width = Line::from(prefix.as_str()).width();
        let suffix_width = width.saturating_sub(prefix_width + separator_width);
        if suffix_width >= 4 {
            format!("{prefix}{GAP}{}", truncate_middle(suffix, suffix_width))
        } else {
            let prefix_width = width.saturating_sub(separator_width);
            format!(
                "{}{}{}",
                truncate_middle(&parts[0], prefix_width / 2),
                GAP,
                truncate_middle(suffix, prefix_width.saturating_sub(prefix_width / 2)),
            )
        }
    };
    Line::from(Span::styled(value, theme::secondary_style()))
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
