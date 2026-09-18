use ratatui::buffer::CellWidth;
use ratatui::style::Style;
use ratatui::text::Line;

pub(super) fn wrapped_line_count_for_line(line: &Line<'_>, max_width: u16) -> usize {
    let mut line_width: u16 = 0;
    let mut word_width: u16 = 0;
    let mut whitespace_width: u16 = 0;
    let mut whitespace = std::collections::VecDeque::new();
    let mut line_has_content = false;
    let mut word_has_content = false;
    let mut non_whitespace_previous = false;
    let mut count = 0;

    for grapheme in line.styled_graphemes(Style::default()) {
        let is_whitespace = grapheme.is_whitespace();
        let symbol_width = grapheme.symbol.cell_width();
        if symbol_width > max_width {
            continue;
        }

        let word_found = non_whitespace_previous && is_whitespace;
        let untrimmed_overflow = !line_has_content
            && word_width
                .saturating_add(whitespace_width)
                .saturating_add(symbol_width)
                > max_width;
        if word_found || untrimmed_overflow {
            if !whitespace.is_empty() {
                line_has_content = true;
            }
            if word_has_content {
                line_has_content = true;
            }
            line_width = line_width
                .saturating_add(whitespace_width)
                .saturating_add(word_width);
            whitespace.clear();
            whitespace_width = 0;
            word_width = 0;
            word_has_content = false;
        }

        let line_full = line_width >= max_width;
        let pending_word_overflow = symbol_width > 0
            && line_width
                .saturating_add(whitespace_width)
                .saturating_add(word_width)
                >= max_width;
        if line_full || pending_word_overflow {
            count += 1;
            let mut remaining_width = max_width.saturating_sub(line_width);
            while let Some(width) = whitespace.front().copied() {
                if width > remaining_width {
                    break;
                }
                whitespace.pop_front();
                whitespace_width = whitespace_width.saturating_sub(width);
                remaining_width = remaining_width.saturating_sub(width);
            }
            line_width = 0;
            line_has_content = false;
            if is_whitespace && whitespace.is_empty() {
                continue;
            }
        }

        if is_whitespace {
            whitespace_width = whitespace_width.saturating_add(symbol_width);
            whitespace.push_back(symbol_width);
        } else {
            word_width = word_width.saturating_add(symbol_width);
            word_has_content = true;
        }
        non_whitespace_previous = !is_whitespace;
    }

    let has_unrendered_content = line_has_content || word_has_content || !whitespace.is_empty();
    count + usize::from(count == 0 || has_unrendered_content)
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::Text;
    use ratatui::widgets::{Paragraph, Widget, Wrap};
    use rstest::rstest;

    use super::*;

    const WIDTH: u16 = 5;
    const MARKER: &str = "¤";

    fn rendered_line_count(line: Line<'_>, width: u16) -> usize {
        let area = Rect::new(0, 0, width, 64);
        let text = Text::from(vec![line, Line::from(MARKER)]);
        let mut buffer = Buffer::empty(area);
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .render(area, &mut buffer);

        (0..area.height)
            .find(|&y| {
                buffer
                    .cell((0, y))
                    .is_some_and(|cell| cell.symbol() == MARKER)
            })
            .map(usize::from)
            .expect("marker should be rendered after the target line")
    }

    #[rstest]
    #[case::empty("")]
    #[case::exact_width("12345")]
    #[case::one_trailing_space("12345 ")]
    #[case::two_trailing_spaces("12345  ")]
    #[case::whitespace_only("     ")]
    #[case::long_word("123456")]
    #[case::wide_character("界")]
    #[case::combining_character("e\u{301}")]
    fn wrapped_count_matches_paragraph_rendering(#[case] input: &str) {
        let line = Line::from(input);

        assert_eq!(
            wrapped_line_count_for_line(&line, WIDTH),
            rendered_line_count(line, WIDTH),
            "input: {input:?}"
        );
    }

    struct ExpectedCase {
        name: &'static str,
        input: &'static str,
        expected: usize,
    }

    #[test]
    fn known_trailing_space_counts_match_rendering() {
        for case in [
            ExpectedCase {
                name: "exact_width",
                input: "12345",
                expected: 1,
            },
            ExpectedCase {
                name: "one_trailing_space",
                input: "12345 ",
                expected: 1,
            },
            ExpectedCase {
                name: "two_trailing_spaces",
                input: "12345  ",
                expected: 2,
            },
        ] {
            let line = Line::from(case.input);
            assert_eq!(
                wrapped_line_count_for_line(&line, WIDTH),
                case.expected,
                "case: {}",
                case.name
            );
            assert_eq!(
                rendered_line_count(line, WIDTH),
                case.expected,
                "case: {}",
                case.name
            );
        }
    }
}
