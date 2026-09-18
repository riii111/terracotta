use ratatui::buffer::CellWidth;
use ratatui::style::Style;
use ratatui::text::Line;

use super::ResourceDetailState;

pub(super) fn max_scroll(
    state: &ResourceDetailState,
    viewport_width: u16,
    viewport_height: u16,
    now: std::time::Instant,
) -> u16 {
    let content = super::detail_content(state, now);
    let max_scroll =
        wrapped_line_count(&content, viewport_width).saturating_sub(usize::from(viewport_height));
    u16::try_from(max_scroll).unwrap_or(u16::MAX)
}

pub(super) fn wrapped_selected_line(
    content: &super::rows::DetailContent,
    viewport_width: u16,
) -> Option<usize> {
    let selected_line = content.selected_line?;
    Some(
        content.lines[..selected_line]
            .iter()
            .map(|line| wrapped_line_count_for_line(line, viewport_width.max(1)))
            .sum(),
    )
}

fn wrapped_line_count(content: &super::rows::DetailContent, viewport_width: u16) -> usize {
    content
        .lines
        .iter()
        .map(|line| wrapped_line_count_for_line(line, viewport_width.max(1)))
        .sum()
}

fn wrapped_line_count_for_line(line: &Line<'_>, max_width: u16) -> usize {
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

#[cfg(test)]
mod tests {
    use crate::app::plan::AttributeChangeKind;
    use crate::ui::test_support::buffer_text;

    use super::super::test_support::*;
    use super::super::*;

    #[test]
    fn selects_changed_attributes_and_scrolls_without_exposing_values() {
        let mut state = state();
        let initial_scroll = state.scroll();

        state.apply_at(DetailAction::SelectNext, 46, 4, Instant::now());
        assert!(state.scroll() > initial_scroll);
        state.apply_at(DetailAction::SelectNext, 46, 4, Instant::now());
        let text = buffer_text(&render(&state, 48, 12));
        assert!(text.contains("> password"), "{text}");

        state.apply_at(DetailAction::PageDown, 46, 4, Instant::now());
        let text = buffer_text(&render(&state, 48, 12));
        assert!(!text.contains("synthetic-secret"), "{text}");
    }

    #[test]
    fn expanding_deep_group_keeps_child_selection_and_scrolls_to_it() {
        let mut state = state_for_change(expansion_change(), &[]);
        let group = AttributeGroup::Nested {
            kind: AttributeChangeKind::Changed,
            path: vec![AttributePathSegment::Key("group_a".to_owned())],
        };
        select_group(&mut state, &group);

        state.apply_at(DetailAction::ToggleExpansion, 36, 4, Instant::now());
        state.apply_at(DetailAction::SelectNext, 36, 4, Instant::now());
        state.apply_at(DetailAction::SelectNext, 36, 4, Instant::now());
        assert!(state.scroll() > 0);

        let nested_group = AttributeGroup::Nested {
            kind: AttributeChangeKind::Changed,
            path: vec![
                AttributePathSegment::Key("group_a".to_owned()),
                AttributePathSegment::Key("nested".to_owned()),
            ],
        };
        assert_eq!(
            detail_rows(&state)
                .get(state.selected)
                .and_then(DetailRow::group),
            Some(&nested_group)
        );
        state.apply_at(DetailAction::ToggleExpansion, 36, 4, Instant::now());
        state.apply_at(DetailAction::SelectNext, 36, 4, Instant::now());
        let text = buffer_text(&render(&state, 48, 12));
        assert!(text.contains("> group_a.nested.value"), "{text}");
    }

    #[test]
    fn page_down_stops_at_the_last_wrapped_line() {
        let mut state = state();

        for _ in 0..100 {
            state.apply_at(DetailAction::PageDown, 46, 4, Instant::now());
        }
        let last_scroll = state.scroll();
        state.apply_at(DetailAction::PageDown, 46, 4, Instant::now());

        assert_eq!(state.scroll(), last_scroll);
    }
}
