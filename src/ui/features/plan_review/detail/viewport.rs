use ratatui::buffer::CellWidth;
use ratatui::style::Style;
use ratatui::text::Line;

use super::{DetailScroll, DetailViewState};

pub(super) fn apply_scroll(
    view: &mut DetailViewState,
    scroll: DetailScroll,
    content: &super::rows::DetailContent,
    viewport_width: u16,
    viewport_height: u16,
) {
    let page = viewport_height.max(1);
    let max = max_scroll(content, viewport_width, page);
    view.scroll = match scroll {
        DetailScroll::PageUp => view.scroll.saturating_sub(page),
        DetailScroll::PageDown => view.scroll.saturating_add(page).min(max),
    };
}

pub(super) fn clamp_scroll(
    view: &mut DetailViewState,
    content: &super::rows::DetailContent,
    viewport_width: u16,
    viewport_height: u16,
) {
    view.scroll = view
        .scroll
        .min(max_scroll(content, viewport_width, viewport_height.max(1)));
}

pub(super) fn ensure_selected_visible(
    view: &mut DetailViewState,
    content: &super::rows::DetailContent,
    viewport_width: u16,
    viewport_height: u16,
) {
    clamp_scroll(view, content, viewport_width, viewport_height);
    let Some(selected_line) = wrapped_selected_line(content, viewport_width) else {
        return;
    };
    let selected_line = u16::try_from(selected_line).unwrap_or(u16::MAX);
    let viewport_height = viewport_height.max(1);
    if selected_line < view.scroll {
        view.scroll = selected_line;
    } else if selected_line >= view.scroll.saturating_add(viewport_height) {
        view.scroll = selected_line.saturating_sub(viewport_height.saturating_sub(1));
    }
    clamp_scroll(view, content, viewport_width, viewport_height);
}

pub(super) fn max_scroll(
    content: &super::rows::DetailContent,
    viewport_width: u16,
    viewport_height: u16,
) -> u16 {
    let max_scroll =
        wrapped_line_count(content, viewport_width).saturating_sub(usize::from(viewport_height));
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
    use super::super::test_support::*;
    use super::super::*;
    use crate::app::plan::AttributeChangeKind;
    use crate::app::plan::AttributePathSegment;
    use crate::app::review::{AttributeGroup, DetailAction, DetailRow};
    use crate::ui::test_support::buffer_text;

    #[test]
    fn selects_changed_attributes_and_scrolls_without_exposing_values() {
        let mut state = state();
        let initial_scroll = state.scroll();

        state.apply_action(DetailAction::SelectNext, 46, 4, Instant::now());
        assert!(state.scroll() > initial_scroll);
        state.apply_action(DetailAction::SelectNext, 46, 4, Instant::now());
        let text = buffer_text(&render(&state, 48, 12));
        assert!(text.contains("> password"), "{text}");

        state.apply_scroll(DetailScroll::PageDown, 46, 4, Instant::now());
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

        state.apply_action(DetailAction::ToggleExpansion, 36, 4, Instant::now());
        state.apply_action(DetailAction::SelectNext, 36, 4, Instant::now());
        state.apply_action(DetailAction::SelectNext, 36, 4, Instant::now());
        assert!(state.scroll() > 0);

        let nested_group = AttributeGroup::Nested {
            kind: AttributeChangeKind::Changed,
            path: vec![
                AttributePathSegment::Key("group_a".to_owned()),
                AttributePathSegment::Key("nested".to_owned()),
            ],
        };
        assert_eq!(
            state
                .detail
                .rows()
                .get(state.detail.selected())
                .and_then(DetailRow::group),
            Some(&nested_group)
        );
        state.apply_action(DetailAction::ToggleExpansion, 36, 4, Instant::now());
        state.apply_action(DetailAction::SelectNext, 36, 4, Instant::now());
        let text = buffer_text(&render(&state, 48, 12));
        assert!(text.contains("> group_a.nested.value"), "{text}");
    }

    #[test]
    fn page_down_stops_at_the_last_wrapped_line() {
        let mut state = state();

        for _ in 0..100 {
            state.apply_scroll(DetailScroll::PageDown, 46, 4, Instant::now());
        }
        let last_scroll = state.scroll();
        state.apply_scroll(DetailScroll::PageDown, 46, 4, Instant::now());

        assert_eq!(state.scroll(), last_scroll);
    }

    #[test]
    fn resize_clamps_manual_offset_without_reselecting_the_row() {
        let mut state = state();
        for _ in 0..100 {
            state.apply_scroll(DetailScroll::PageDown, 46, 4, Instant::now());
        }
        let selected = state.detail.selected();
        assert!(state.scroll() > 0);

        super::super::clamp_detail_scroll(
            &mut state.view,
            &state.list,
            &state.detail,
            state.copy_notice,
            Instant::now(),
            Rect::new(0, 0, 48, 30),
        );

        assert_eq!(state.detail.selected(), selected);
        assert!(state.scroll() < 100);
    }
}
