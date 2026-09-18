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

    count + 1
}
