use ratatui::{
    Frame,
    layout::Rect,
    symbols::scrollbar::Set,
    widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::ui::theme;

pub(crate) fn render_vertical(
    frame: &mut Frame<'_>,
    area: Rect,
    content_length: usize,
    viewport_length: usize,
    position: usize,
) {
    let mut state = ScrollbarState::new(content_length)
        .viewport_content_length(viewport_length)
        .position(position);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .symbols(Set {
            track: "│",
            thumb: "█",
            begin: "↑",
            end: "↓",
        })
        .thumb_style(theme::scrollbar_thumb_style())
        .track_style(theme::scrollbar_track_style())
        .begin_style(begin_style(position))
        .end_style(end_style(position, content_length, viewport_length));
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

pub(crate) fn render_horizontal(
    frame: &mut Frame<'_>,
    area: Rect,
    content_length: usize,
    viewport_length: usize,
    position: usize,
) {
    let mut state = ScrollbarState::new(content_length)
        .viewport_content_length(viewport_length)
        .position(position);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
        .symbols(Set {
            track: "─",
            thumb: "█",
            begin: "←",
            end: "→",
        })
        .thumb_style(theme::scrollbar_thumb_style())
        .track_style(theme::scrollbar_track_style())
        .begin_style(begin_style(position))
        .end_style(end_style(position, content_length, viewport_length));
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

fn begin_style(position: usize) -> ratatui::style::Style {
    if position == 0 {
        theme::scrollbar_track_style()
    } else {
        theme::scrollbar_thumb_style()
    }
}

fn end_style(
    position: usize,
    content_length: usize,
    viewport_length: usize,
) -> ratatui::style::Style {
    if position.saturating_add(viewport_length) >= content_length {
        theme::scrollbar_track_style()
    } else {
        theme::scrollbar_thumb_style()
    }
}
