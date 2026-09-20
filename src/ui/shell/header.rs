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

use super::context::{target, truncate_middle};

const PREFIX: &str = "Terracotta | ";
const GAP: usize = 2;

pub(crate) fn render(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>) {
    frame.render_widget(Paragraph::new(lines).style(theme::secondary_style()), area);
}

pub(crate) fn render_review(frame: &mut Frame<'_>, area: Rect, review: &PlanReview) {
    render(
        frame,
        area,
        vec![header_line(
            review.root(),
            Some(review.workspace()),
            area.width,
        )],
    );
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

fn header_line(path: &Path, workspace: Option<&str>, width: u16) -> Line<'static> {
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
