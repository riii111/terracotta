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
            .map(|line| {
                super::super::wrap::wrapped_line_count_for_line(line, viewport_width.max(1))
            })
            .sum(),
    )
}

fn wrapped_line_count(content: &super::rows::DetailContent, viewport_width: u16) -> usize {
    content
        .lines
        .iter()
        .map(|line| super::super::wrap::wrapped_line_count_for_line(line, viewport_width.max(1)))
        .sum()
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
