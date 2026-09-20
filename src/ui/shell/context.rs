use std::path::Path;

use ratatui::buffer::CellWidth;
use ratatui::style::Style;
use ratatui::text::Line;

pub(crate) fn target(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
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
    format!(
        "{}...{}",
        take_from_start(value, prefix_width),
        take_from_end(value, suffix_width)
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
    fn middle_truncation_preserves_both_ends() {
        assert_eq!(truncate_middle("abcdef", 5), "a...f");
        assert_eq!(truncate_middle("abcdef", 3), "...");
    }
}
