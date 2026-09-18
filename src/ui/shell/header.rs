use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::execution::{ExecutionContext, ExecutionContextValue};
use crate::app::review::{PlanListContext, ReviewComparison};
use crate::ui::theme;

use super::context::{execution_target, review_target, truncate_middle};

pub(crate) fn render(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>) {
    frame.render_widget(Paragraph::new(lines).style(theme::secondary_style()), area);
}

pub(crate) fn render_review(
    frame: &mut Frame<'_>,
    area: Rect,
    context: Option<&PlanListContext>,
    comparison: &ReviewComparison,
) {
    render(frame, area, review_lines(context, comparison, area.width));
}

pub(crate) fn render_execution(frame: &mut Frame<'_>, area: Rect, context: &ExecutionContext) {
    render(frame, area, execution_lines(context, area.width));
}

fn review_lines(
    context: Option<&PlanListContext>,
    comparison: &ReviewComparison,
    width: u16,
) -> Vec<Line<'static>> {
    let workspace = context
        .map(PlanListContext::workspace)
        .filter(|workspace| *workspace != "default")
        .map(|workspace| format!(" [workspace: {workspace}]"))
        .unwrap_or_default();
    let branch = context
        .map(PlanListContext::git)
        .filter(|git| !git.is_empty() && *git != "unavailable")
        .map(|git| format!(" [branch: {git}]"))
        .unwrap_or_default();
    vec![
        header_line(
            &fit_line(
                "Terracotta | ",
                &review_target(context),
                &workspace,
                width,
                true,
            ),
            width,
        ),
        header_line(
            &fit_line("Git: ", &comparison.label(), &branch, width, false),
            width,
        ),
    ]
}

fn execution_lines(context: &ExecutionContext, width: u16) -> Vec<Line<'static>> {
    let workspace = match context.workspace() {
        ExecutionContextValue::Known(workspace) if workspace != "default" => {
            format!(" [workspace: {workspace}]")
        }
        _ => String::new(),
    };
    let branch = match context.git() {
        ExecutionContextValue::Known(git) if !git.is_empty() => format!(" [branch: {git}]"),
        _ => String::new(),
    };
    vec![
        header_line(
            &fit_line(
                "Terracotta | ",
                &execution_target(context),
                &workspace,
                width,
                true,
            ),
            width,
        ),
        header_line(
            &fit_line(
                "Git: ",
                context.comparison().as_str(),
                &branch,
                width,
                false,
            ),
            width,
        ),
    ]
}

fn header_line(value: &str, width: u16) -> Line<'static> {
    Line::from(Span::styled(
        truncate_middle(value, usize::from(width)),
        theme::secondary_style(),
    ))
}

fn fit_line(prefix: &str, main: &str, suffix: &str, width: u16, preserve_suffix: bool) -> String {
    let width = usize::from(width);
    let full = format!("{prefix}{main}{suffix}");
    if display_width(&full) <= width {
        return full;
    }
    if !preserve_suffix || suffix.is_empty() {
        return format!(
            "{prefix}{}",
            truncate_middle(main, width.saturating_sub(display_width(prefix)))
        );
    }

    let prefix_width = display_width(prefix);
    let available = width.saturating_sub(prefix_width);
    if available == 0 {
        return truncate_middle(&full, width);
    }
    let suffix_width = display_width(suffix).min(available.saturating_sub(1) / 3);
    let suffix = truncate_middle(suffix, suffix_width);
    let main_width = available.saturating_sub(display_width(&suffix));
    format!("{prefix}{}{suffix}", truncate_middle(main, main_width))
}

fn display_width(value: &str) -> usize {
    Line::from(value).width()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::review::{ReviewComparisonBasis, ReviewComparisonStatus};

    fn comparison() -> ReviewComparison {
        ReviewComparison::new(
            ReviewComparisonBasis::WorkingTreeVsHead,
            None,
            ReviewComparisonStatus::Complete,
        )
    }

    #[test]
    fn review_header_keeps_two_lines_without_context() {
        let lines = review_lines(None, &comparison(), 80);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].to_string(), "Terracotta | Target unavailable");
        assert_eq!(lines[1].to_string(), "Git: working tree vs HEAD");
    }

    #[test]
    fn execution_header_omits_default_workspace() {
        let context = ExecutionContext::known(
            "/repo/main",
            "default",
            "feature/ui",
            "working tree vs HEAD",
        );
        let lines = execution_lines(&context, 80);

        assert_eq!(lines[0].to_string(), "Terracotta | main (Git unavailable)");
        assert_eq!(
            lines[1].to_string(),
            "Git: working tree vs HEAD [branch: feature/ui]"
        );
    }
}
