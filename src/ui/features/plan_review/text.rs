use ratatui::buffer::CellWidth;
use ratatui::style::Style;
use ratatui::text::Line;

pub(super) fn display_width(value: &str) -> usize {
    Line::from(value)
        .styled_graphemes(Style::default())
        .map(|grapheme| usize::from(grapheme.symbol.cell_width()))
        .sum()
}

pub(super) fn truncate_end(value: &str, max_width: usize) -> String {
    let line = Line::from(value);
    if display_width(value) <= max_width {
        return value.to_owned();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let prefix_width = max_width - 3;
    let mut result = String::new();
    let mut width = 0;
    for grapheme in line.styled_graphemes(Style::default()) {
        let grapheme_width = usize::from(grapheme.symbol.cell_width());
        if width + grapheme_width > prefix_width {
            break;
        }
        result.push_str(grapheme.symbol);
        width += grapheme_width;
    }
    result.push_str("...");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TruncationCase {
        name: &'static str,
        input: &'static str,
        width: usize,
        expected: &'static str,
    }

    #[test]
    fn truncates_by_cell_width_without_splitting_graphemes() {
        let cases = [
            TruncationCase {
                name: "ascii_exact",
                input: "abcdef",
                width: 6,
                expected: "abcdef",
            },
            TruncationCase {
                name: "ascii_boundary_before_ellipsis",
                input: "abcdef",
                width: 5,
                expected: "ab...",
            },
            TruncationCase {
                name: "japanese_exact",
                input: "日本語",
                width: 6,
                expected: "日本語",
            },
            TruncationCase {
                name: "japanese_boundary_before_ellipsis",
                input: "日本語abc",
                width: 7,
                expected: "日本...",
            },
            TruncationCase {
                name: "emoji_boundary_before_ellipsis",
                input: "🙂🙂abc",
                width: 5,
                expected: "🙂...",
            },
            TruncationCase {
                name: "emoji_does_not_split_wide_grapheme",
                input: "🙂🙂abc",
                width: 4,
                expected: "...",
            },
            TruncationCase {
                name: "combining_character_stays_with_base",
                input: "e\u{301}clair",
                width: 4,
                expected: "e\u{301}...",
            },
            TruncationCase {
                name: "halfwidth_sound_mark_uses_terminal_width",
                input: "ｶﾞabcdef",
                width: 7,
                expected: "ｶﾞab...",
            },
            TruncationCase {
                name: "zwj_sequence_stays_together",
                input: "👩‍💻abc",
                width: 4,
                expected: "...",
            },
            TruncationCase {
                name: "width_zero",
                input: "abcdef",
                width: 0,
                expected: "",
            },
            TruncationCase {
                name: "width_one",
                input: "abcdef",
                width: 1,
                expected: ".",
            },
            TruncationCase {
                name: "width_two",
                input: "abcdef",
                width: 2,
                expected: "..",
            },
            TruncationCase {
                name: "width_three",
                input: "abcdef",
                width: 3,
                expected: "...",
            },
        ];

        for case in cases {
            assert_eq!(
                truncate_end(case.input, case.width),
                case.expected,
                "case: {}",
                case.name
            );
        }
    }
}
