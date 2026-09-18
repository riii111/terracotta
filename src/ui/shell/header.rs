use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::execution::{ExecutionContext, ExecutionContextValue};
use crate::app::review::{PlanListContext, ReviewComparison};
use crate::ui::theme;

use super::context::{execution_target, review_target, truncate_middle};

const TARGET_PREFIX: &str = "Terracotta | ";
const WORKSPACE_LABEL: &str = "workspace: ";
const BRANCH_LABEL: &str = "branch: ";
const HEADER_GAP: usize = 2;

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
        header_line(&fit_target_line(
            &review_target(context),
            workspace,
            branch,
            width,
        )),
        header_line(&fit_comparison_line(&comparison.label(), width)),
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
        header_line(&fit_target_line(
            &execution_target(context),
            workspace,
            branch,
            width,
        )),
        header_line(&fit_comparison_line(context.comparison().as_str(), width)),
    ]
}

fn header_line(value: &str) -> Line<'static> {
    Line::from(Span::styled(value.to_owned(), theme::secondary_style()))
}

fn fit_target_line(
    target: &str,
    workspace: Option<&str>,
    branch: Option<&str>,
    width: u16,
) -> String {
    let width = usize::from(width);
    let left = format!("{TARGET_PREFIX}{target}");
    if let Some(right) = full_right_context(workspace, branch)
        && display_width(&left) + HEADER_GAP + display_width(&right) <= width
    {
        let gap = width - display_width(&left) - display_width(&right);
        return format!("{left}{}{right}", " ".repeat(gap));
    }
    let Some(right) = fit_right_context(workspace, branch, width / 2) else {
        return truncate_middle(&left, width);
    };
    let right_width = display_width(&right);
    let left_width = width.saturating_sub(right_width + HEADER_GAP);
    let target_prefix_width = display_width(TARGET_PREFIX);
    if left_width < target_prefix_width {
        return truncate_middle(&left, width);
    }
    let left = format!(
        "{TARGET_PREFIX}{}",
        truncate_middle(target, left_width - target_prefix_width)
    );
    let gap = width.saturating_sub(display_width(&left) + right_width);
    format!("{left}{}{right}", " ".repeat(gap))
}

fn full_right_context(workspace: Option<&str>, branch: Option<&str>) -> Option<String> {
    match (workspace, branch) {
        (Some(workspace), Some(branch)) => Some(format!(
            "{WORKSPACE_LABEL}{workspace} | {BRANCH_LABEL}{branch}"
        )),
        (Some(workspace), None) => Some(format!("{WORKSPACE_LABEL}{workspace}")),
        (None, Some(branch)) => Some(format!("{BRANCH_LABEL}{branch}")),
        (None, None) => None,
    }
}

fn fit_right_context(
    workspace: Option<&str>,
    branch: Option<&str>,
    max_width: usize,
) -> Option<String> {
    match (workspace, branch) {
        (Some(workspace), Some(branch)) => {
            let full = full_right_context(Some(workspace), Some(branch))
                .expect("workspace or branch is present");
            if display_width(&full) <= max_width {
                Some(full)
            } else {
                Some(fit_labeled_value(WORKSPACE_LABEL, workspace, max_width))
            }
        }
        (Some(workspace), None) => Some(fit_labeled_value(WORKSPACE_LABEL, workspace, max_width)),
        (None, Some(branch)) => Some(fit_labeled_value(BRANCH_LABEL, branch, max_width)),
        (None, None) => None,
    }
}

fn fit_labeled_value(label: &str, value: &str, max_width: usize) -> String {
    let label_width = display_width(label);
    if max_width < label_width {
        return truncate_middle(label, max_width);
    }
    format!("{label}{}", truncate_middle(value, max_width - label_width))
}

fn fit_comparison_line(comparison: &str, width: u16) -> String {
    const PREFIX: &str = "Git: ";
    let width = usize::from(width);
    if width < display_width(PREFIX) {
        return truncate_middle(PREFIX, width);
    }
    format!(
        "{PREFIX}{}",
        truncate_middle(comparison, width - display_width(PREFIX))
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
    fn execution_header_aligns_branch_and_omits_default_workspace() {
        let context = ExecutionContext::known(
            "/repo/main",
            "default",
            "feature/ui",
            "working tree vs HEAD",
        );
        let lines = execution_lines(&context, 80);

        let left = "Terracotta | main (Git unavailable)";
        let right = "branch: feature/ui";
        assert_eq!(
            lines[0].to_string(),
            format!(
                "{left}{}{right}",
                " ".repeat(80 - display_width(left) - display_width(right))
            )
        );
        assert_eq!(lines[1].to_string(), "Git: working tree vs HEAD");
    }

    #[test]
    fn execution_header_keeps_loading_workspace_visible() {
        let context = ExecutionContext::loading("/repo/main", "working tree vs HEAD");
        let lines = execution_lines(&context, 80);

        assert!(
            lines[0].to_string().ends_with("workspace: loading..."),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn narrow_header_prioritizes_workspace_and_keeps_a_gap() {
        let context = ExecutionContext::known(
            "/a/very/long/terraform/target",
            "prod",
            "feature/ui",
            "working tree vs HEAD",
        );
        let line = execution_lines(&context, 48)[0].to_string();

        assert!(line.contains("workspace: prod"), "{line}");
        assert!(!line.contains("branch:"), "{line}");
        assert!(line.find("workspace:").unwrap() >= 2, "{line}");
        assert!(display_width(&line) <= 48, "{line}");
    }

    #[test]
    fn narrow_header_truncates_branch_value_without_workspace() {
        let context = ExecutionContext::known(
            "/repo/main",
            "default",
            "feature/with-a-very-long-branch-name",
            "working tree vs HEAD",
        );
        let line = execution_lines(&context, 48)[0].to_string();

        assert!(line.contains("branch: "), "{line}");
        assert!(!line.contains("workspace:"), "{line}");
        assert!(
            !line.contains("feature/with-a-very-long-branch-name"),
            "{line}"
        );
        assert!(display_width(&line) <= 48, "{line}");
    }

    #[test]
    fn comparison_line_uses_the_full_width_without_branch() {
        let lines = execution_lines(
            &ExecutionContext::known(
                "/repo/main",
                "default",
                "feature/ui",
                "比較条件がとても長い🙂内容",
            ),
            12,
        );

        assert_eq!(display_width(&lines[1].to_string()), 12);
        assert!(!lines[1].to_string().contains("branch:"));
    }

    #[test]
    fn header_handles_unicode_context_at_supported_and_tiny_widths() {
        let context = ExecutionContext::known(
            "/repo/対象́🙂",
            "開発workspace",
            "feature/long-branch-name",
            "比較条件🙂とても長い内容",
        );

        for width in [165, 120, 80, 48, 12, 4, 0] {
            let lines = execution_lines(&context, width);

            assert_eq!(lines.len(), 2, "width: {width}");
            assert!(
                display_width(&lines[0].to_string()) <= usize::from(width),
                "width: {width}, line: {}",
                lines[0]
            );
            assert!(
                display_width(&lines[1].to_string()) <= usize::from(width),
                "width: {width}, line: {}",
                lines[1]
            );
        }

        let unavailable = ExecutionContext::known(
            "/repo/main",
            "unavailable",
            "feature/ui",
            "working tree vs HEAD",
        );
        assert!(
            execution_lines(&unavailable, 80)[0]
                .to_string()
                .contains("workspace: unavailable")
        );
    }
}
