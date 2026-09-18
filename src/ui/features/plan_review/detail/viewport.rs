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
    use crate::app::attribution::SourceSide;
    use crate::app::copy::CopyTarget;
    use crate::app::plan::AttributeChangeKind;
    use crate::app::plan::AttributePathSegment;
    use crate::app::review::{AttributeGroup, DetailAction, DetailRow};
    use crate::ui::test_support::buffer_text;
    use ratatui::text::Line;

    use super::wrapped_selected_line;

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
    fn selected_line_starts_on_the_next_rendered_row_after_fitting_trailing_space() {
        let content = super::super::rows::DetailContent {
            lines: vec![Line::from("12345 "), Line::from("next")],
            selected_line: Some(1),
        };

        assert_eq!(wrapped_selected_line(&content, 5), Some(1));
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

    #[test]
    fn reaches_the_last_of_one_hundred_analyzed_files_with_page_scroll() {
        let mut state = state_with_sources(
            (0..100)
                .map(|index| {
                    source_file(
                        format!("sources/source-{index:03}.tf"),
                        SourceSide::After,
                        Vec::new(),
                    )
                })
                .collect(),
        );
        let collapsed =
            super::super::rows::detail_content(&state.list, &state.detail, false, Instant::now());
        let collapsed_diff = collapsed
            .lines
            .iter()
            .position(|line| line.to_string() == "Diff:")
            .expect("diff heading should be present");

        state.toggle_analysis_info(80, 20, Instant::now());
        let expanded =
            super::super::rows::detail_content(&state.list, &state.detail, true, Instant::now());
        let expanded_diff = expanded
            .lines
            .iter()
            .position(|line| line.to_string() == "Diff:")
            .expect("diff heading should be present");
        assert_eq!(collapsed_diff, expanded_diff);

        for _ in 0..100 {
            state.apply_scroll(DetailScroll::PageDown, 78, 4, Instant::now());
        }
        let text = buffer_text(&render(&state, 80, 20));
        assert!(text.contains("source-099.tf (working tree)"), "{text}");
    }

    #[test]
    fn toggling_analysis_info_preserves_selection_reveal_and_copy_state() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();
        state.detail.apply(DetailAction::Reveal, now);
        state.set_copy_notice(CopyNotice::Copied {
            target: CopyTarget::Resource,
            resource_count: 1,
        });
        let detail_before = state.detail.clone();
        let list_before = state.list.clone();
        let notice_before = state.copy_notice;

        state.toggle_analysis_info(100, 40, now);
        let expanded = buffer_text(&render_at(&state, 100, 40, now));
        assert!(expanded.contains("old-secret"), "{expanded}");
        assert_eq!(state.detail, detail_before);
        assert_eq!(state.list, list_before);
        assert_eq!(state.copy_notice, notice_before);

        state.toggle_analysis_info(100, 40, now);
        assert_eq!(state.detail, detail_before);
        assert_eq!(state.list, list_before);
        assert_eq!(state.copy_notice, notice_before);
    }
}
