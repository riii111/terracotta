use std::path::Path;

use ratatui::buffer::CellWidth;
use ratatui::style::Style;
use ratatui::text::Line;

use crate::app::execution::{ExecutionContext, ExecutionContextValue};
use crate::app::review::PlanListContext;

pub(crate) fn review_target(context: Option<&PlanListContext>) -> String {
    let Some(context) = context else {
        return "Target unavailable".to_owned();
    };

    context.repository_root().map_or_else(
        || format!("{} (Git unavailable)", path_name(context.root())),
        |repository_root| repository_target(repository_root, context.root()),
    )
}

pub(crate) fn execution_target(context: &ExecutionContext) -> String {
    match context.repository_root() {
        ExecutionContextValue::Known(repository_root) => {
            context.repository_root_path().map_or_else(
                || repository_root.to_owned(),
                |path| repository_target(path, context.cwd_path()),
            )
        }
        ExecutionContextValue::Loading => {
            format!("{} (Git loading)", path_name(context.cwd_path()))
        }
        ExecutionContextValue::Unavailable => {
            format!("{} (Git unavailable)", path_name(context.cwd_path()))
        }
    }
}

pub(crate) fn display_path(
    path: &Path,
    repository_root: Option<&Path>,
    execution_root: Option<&Path>,
) -> String {
    repository_root
        .or(execution_root)
        .and_then(|root| path.strip_prefix(root).ok())
        .map_or_else(
            || path.display().to_string(),
            |relative| relative.display().to_string(),
        )
}

pub(crate) fn truncate_middle(value: &str, max_width: usize) -> String {
    let width = display_width(value);
    if width <= max_width {
        return value.to_owned();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let remaining = max_width - 3;
    let prefix_width = remaining.div_ceil(2);
    let suffix_width = remaining - prefix_width;
    let prefix = take_from_start(value, prefix_width);
    let mut suffix = take_from_end(value, suffix_width);
    let prefix = if suffix.is_empty() && suffix_width < remaining {
        suffix = take_from_end(value, remaining);
        String::new()
    } else {
        prefix
    };
    format!("{prefix}...{suffix}")
}

fn repository_target(repository_root: &Path, execution_root: &Path) -> String {
    let repository_name = path_name(repository_root);
    match execution_root.strip_prefix(repository_root).ok() {
        Some(relative) if relative.as_os_str().is_empty() => repository_name,
        Some(relative) => format!("{repository_name} / {}", relative.display()),
        None => format!("{repository_name} / {}", execution_root.display()),
    }
}

fn path_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn display_width(value: &str) -> usize {
    Line::from(value)
        .styled_graphemes(Style::default())
        .map(|grapheme| usize::from(grapheme.symbol.cell_width()))
        .sum()
}

fn take_from_start(value: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut width = 0;
    for grapheme in Line::from(value).styled_graphemes(Style::default()) {
        let grapheme_width = usize::from(grapheme.symbol.cell_width());
        if width + grapheme_width > max_width {
            break;
        }
        result.push_str(grapheme.symbol);
        width += grapheme_width;
    }
    result
}

fn take_from_end(value: &str, max_width: usize) -> String {
    let graphemes = Line::from(value)
        .styled_graphemes(Style::default())
        .map(|grapheme| grapheme.symbol.to_owned())
        .collect::<Vec<_>>();
    let mut result = String::new();
    let mut width = 0;
    for grapheme in graphemes.iter().rev() {
        let grapheme_width = Line::from(grapheme.as_str()).width();
        if width + grapheme_width > max_width {
            break;
        }
        result.insert_str(0, grapheme);
        width += grapheme_width;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_context_does_not_invent_a_target() {
        assert_eq!(review_target(None), "Target unavailable");
    }

    #[test]
    fn display_path_relativizes_only_paths_inside_the_repository() {
        assert_eq!(
            display_path(Path::new("/repo/main.tf"), Some(Path::new("/repo")), None),
            "main.tf"
        );
        assert_eq!(
            display_path(
                Path::new("/outside/main.tf"),
                Some(Path::new("/repo")),
                None
            ),
            "/outside/main.tf"
        );
        assert_eq!(
            display_path(
                Path::new("/repo/root/main.tf"),
                None,
                Some(Path::new("/repo/root"))
            ),
            "main.tf"
        );
    }

    #[test]
    fn execution_target_distinguishes_git_loading_and_unavailable() {
        let loading = ExecutionContext::loading("/repo/main", "working tree vs HEAD");
        assert_eq!(execution_target(&loading), "main (Git loading)");

        let unavailable = loading.with_repository_root(None);
        assert_eq!(execution_target(&unavailable), "main (Git unavailable)");
    }

    #[test]
    fn repository_root_target_uses_the_repository_name() {
        let context = ExecutionContext::known("/repo", "default", "main", "working tree vs HEAD")
            .with_repository_root(Some(Path::new("/repo").to_path_buf()));

        assert_eq!(execution_target(&context), "repo");
        assert_eq!(
            repository_target(Path::new("/repo"), Path::new("/repo")),
            "repo"
        );
    }

    #[test]
    fn middle_truncation_keeps_the_tail_without_splitting_graphemes() {
        assert_eq!(truncate_middle("abcdef", 5), "a...f");
        assert_eq!(truncate_middle("日本語", 5), "...語");
        assert_eq!(truncate_middle("🙂🙂abc", 4), "...c");
        assert_eq!(truncate_middle("abcdef", 0), "");
        assert_eq!(truncate_middle("abcdef", 1), ".");
        assert_eq!(truncate_middle("abcdef", 2), "..");
        assert_eq!(truncate_middle("abcdef", 3), "...");
    }
}
