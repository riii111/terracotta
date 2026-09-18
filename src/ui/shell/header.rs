use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::execution::{ExecutionContext, ExecutionContextValue};
use crate::app::review::{PlanListContext, ReviewComparison};
use crate::ui::theme;

use super::context::{execution_target, review_target, truncate_middle};

const WORKSPACE_PREFIX: &str = " [workspace: ";
const WORKSPACE_SUFFIX: &str = "]";
const BRANCH_PREFIX: &str = " [branch: ";
const BRANCH_SUFFIX: &str = "]";

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
        .filter(|workspace| *workspace != "default");
    let branch = context
        .map(PlanListContext::git)
        .filter(|git| !git.is_empty() && *git != "unavailable");
    vec![
        header_line(
            &fit_workspace_line("Terracotta | ", &review_target(context), workspace, width),
            width,
        ),
        header_line(
            &fit_comparison_line("Git: ", &comparison.label(), branch, width),
            width,
        ),
    ]
}

fn execution_lines(context: &ExecutionContext, width: u16) -> Vec<Line<'static>> {
    let workspace = match context.workspace() {
        ExecutionContextValue::Known(workspace) if workspace != "default" => {
            Some(workspace.as_str())
        }
        ExecutionContextValue::Loading => Some("loading..."),
        ExecutionContextValue::Unavailable => Some("unavailable"),
        ExecutionContextValue::Known(_) => None,
    };
    let branch = match context.git() {
        ExecutionContextValue::Known(git) if !git.is_empty() => Some(git.as_str()),
        _ => None,
    };
    vec![
        header_line(
            &fit_workspace_line(
                "Terracotta | ",
                &execution_target(context),
                workspace,
                width,
            ),
            width,
        ),
        header_line(
            &fit_comparison_line("Git: ", context.comparison().as_str(), branch, width),
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

fn fit_workspace_line(prefix: &str, main: &str, workspace: Option<&str>, width: u16) -> String {
    let width = usize::from(width);
    let Some(workspace) = workspace else {
        return format!(
            "{prefix}{}",
            truncate_middle(main, width.saturating_sub(display_width(prefix)))
        );
    };
    let full = format!("{prefix}{main}{WORKSPACE_PREFIX}{workspace}{WORKSPACE_SUFFIX}");
    if display_width(&full) <= width {
        return full;
    }
    let fixed_width = display_width(prefix) + display_width(WORKSPACE_PREFIX) + 1;
    if width < fixed_width {
        return format!(
            "{prefix}{}",
            truncate_middle(main, width.saturating_sub(display_width(prefix)))
        );
    }
    let value_width = width - fixed_width;
    let main_width = display_width(main);
    let workspace_width = display_width(workspace);
    let (main_width, workspace_width) = if main_width + workspace_width <= value_width {
        (main_width, workspace_width)
    } else if main_width < value_width {
        (
            main_width,
            workspace_width.min(value_width - main_width).max(1),
        )
    } else {
        let workspace_width = workspace_width.min((value_width / 3).max(1));
        (value_width.saturating_sub(workspace_width), workspace_width)
    };
    format!(
        "{prefix}{}{WORKSPACE_PREFIX}{}{WORKSPACE_SUFFIX}",
        truncate_middle(main, main_width),
        truncate_middle(workspace, workspace_width)
    )
}

fn fit_comparison_line(prefix: &str, comparison: &str, branch: Option<&str>, width: u16) -> String {
    let width = usize::from(width);
    let Some(branch) = branch else {
        return format!(
            "{prefix}{}",
            truncate_middle(comparison, width.saturating_sub(display_width(prefix)))
        );
    };
    let full = format!("{prefix}{comparison}{BRANCH_PREFIX}{branch}{BRANCH_SUFFIX}");
    if display_width(&full) <= width {
        return full;
    }
    let branch_fixed_width = display_width(BRANCH_PREFIX) + display_width(BRANCH_SUFFIX);
    let available = width.saturating_sub(display_width(prefix));
    if display_width(comparison) + branch_fixed_width >= available {
        return format!("{prefix}{}", truncate_middle(comparison, available));
    }
    let branch_width = available - display_width(comparison) - branch_fixed_width;
    format!(
        "{prefix}{comparison}{BRANCH_PREFIX}{}{BRANCH_SUFFIX}",
        truncate_middle(branch, branch_width)
    )
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

    #[test]
    fn execution_header_keeps_loading_workspace_visible() {
        let context = ExecutionContext::loading("/repo/main", "working tree vs HEAD");
        let lines = execution_lines(&context, 80);

        assert_eq!(
            lines[0].to_string(),
            "Terracotta | main (Git loading) [workspace: loading...]"
        );
    }

    #[test]
    fn narrow_header_keeps_workspace_label_intact() {
        let context = ExecutionContext::known(
            "/a/very/long/terraform/target",
            "prod",
            "feature/ui",
            "working tree vs HEAD",
        );
        let line = execution_lines(&context, 48)[0].to_string();

        assert!(line.contains("[workspace: prod]"), "{line}");
    }
}
