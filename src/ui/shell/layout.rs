use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders};

use crate::ui::theme;

const HEADER_HEIGHT: u16 = 2;

pub(crate) struct ShellLayout {
    header: Rect,
    content: Rect,
    footer: Rect,
    footer_lines: Vec<Line<'static>>,
}

impl ShellLayout {
    pub(crate) const fn header(&self) -> Rect {
        self.header
    }

    pub(crate) const fn content(&self) -> Rect {
        self.content
    }

    pub(crate) fn content_inner(&self) -> Rect {
        Block::new().borders(Borders::ALL).inner(self.content)
    }

    pub(crate) const fn footer(&self) -> Rect {
        self.footer
    }

    pub(crate) fn footer_lines(&self) -> &[Line<'static>] {
        &self.footer_lines
    }
}

pub(crate) fn layout(
    area: Rect,
    footer_lines: Vec<Line<'static>>,
    required_footer_lines: Vec<Line<'static>>,
    minimum_body_height: u16,
) -> ShellLayout {
    let header_height = HEADER_HEIGHT.min(area.height);
    let header = Rect::new(area.x, area.y, area.width, header_height);
    let after_header_y = area.y.saturating_add(header_height);
    let after_header_height = area.height.saturating_sub(header_height);
    let full_footer_height = u16::try_from(footer_lines.len()).unwrap_or(u16::MAX).max(1);
    let required_footer_height = u16::try_from(required_footer_lines.len())
        .unwrap_or(u16::MAX)
        .max(1);
    let border_height = 2;
    let minimum_content_height = minimum_body_height.saturating_add(border_height);
    let footer_fits = |footer_height: u16| {
        after_header_height >= footer_height.saturating_add(minimum_content_height)
    };
    let (footer_lines, footer_height) = if footer_fits(full_footer_height) {
        (footer_lines, full_footer_height)
    } else if footer_fits(required_footer_height) {
        (required_footer_lines, required_footer_height)
    } else {
        (
            required_footer_lines,
            required_footer_height.min(after_header_height),
        )
    };
    let content_height = after_header_height.saturating_sub(footer_height);
    let content = Rect::new(area.x, after_header_y, area.width, content_height);
    let footer = Rect::new(
        area.x,
        after_header_y.saturating_add(content_height),
        area.width,
        footer_height,
    );
    ShellLayout {
        header,
        content,
        footer,
        footer_lines,
    }
}

pub(crate) const fn centered_width(area: Rect) -> u16 {
    area.width.saturating_sub(2)
}

pub(crate) fn max_centered_height(area: Rect) -> u16 {
    let eighty_percent = u16::try_from(u32::from(area.height) * 4 / 5).unwrap_or(u16::MAX);
    area.height.saturating_sub(2).min(eighty_percent.max(22))
}

pub(crate) fn centered_area(area: Rect, requested_height: u16) -> Rect {
    let width = centered_width(area);
    let height = requested_height.min(max_centered_height(area));
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub(crate) fn max_centered_area(area: Rect) -> Rect {
    centered_area(area, max_centered_height(area))
}

pub(crate) fn required_height(
    content_height: u16,
    footer_lines: &[Line<'static>],
    required_footer_lines: &[Line<'static>],
) -> u16 {
    let footer_height = u16::try_from(footer_lines.len())
        .unwrap_or(u16::MAX)
        .max(1)
        .max(
            u16::try_from(required_footer_lines.len())
                .unwrap_or(u16::MAX)
                .max(1),
        );
    HEADER_HEIGHT
        .saturating_add(content_height)
        .saturating_add(footer_height)
}

pub(crate) fn required_body_height(
    line_count: usize,
    max_line_width: usize,
    body_width: u16,
) -> u16 {
    let line_height = u16::try_from(line_count).unwrap_or(u16::MAX).max(1);
    line_height.saturating_add(u16::from(max_line_width > usize::from(body_width)))
}

pub(crate) fn render_content_block(
    frame: &mut Frame<'_>,
    area: Rect,
    title: impl Into<String>,
) -> Rect {
    render_content_block_line(frame, area, Line::from(title.into()))
}

pub(crate) fn render_content_block_line(
    frame: &mut Frame<'_>,
    area: Rect,
    title: Line<'_>,
) -> Rect {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(theme::frame_style())
        .style(theme::body_style())
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_footer_after_the_content_frame() {
        let layout = layout(
            Rect::new(0, 0, 80, 20),
            vec![Line::from("full")],
            vec![Line::from("required")],
            1,
        );

        assert_eq!(layout.header(), Rect::new(0, 0, 80, 2));
        assert_eq!(
            layout.content().y + layout.content().height,
            layout.footer().y
        );
        assert!(layout.content_inner().height >= 1);
        assert_eq!(layout.footer_lines()[0].to_string(), "full");
    }

    #[test]
    fn falls_back_to_required_footer_lines_when_body_is_tight() {
        let layout = layout(
            Rect::new(0, 0, 80, 6),
            vec![Line::from("full 1"), Line::from("full 2")],
            vec![Line::from("required")],
            1,
        );

        assert_eq!(layout.footer_lines()[0].to_string(), "required");
    }

    #[test]
    fn caps_centered_height_at_four_fifths_with_two_rows_of_margin() {
        assert_eq!(max_centered_height(Rect::new(0, 0, 80, 24)), 22);
        assert_eq!(max_centered_height(Rect::new(0, 0, 120, 40)), 32);
        assert_eq!(max_centered_height(Rect::new(0, 0, 160, 60)), 48);
    }

    #[test]
    fn centers_requested_height_within_the_terminal_width_and_height_cap() {
        assert_eq!(
            centered_area(Rect::new(0, 0, 160, 60), 12),
            Rect::new(1, 24, 158, 12)
        );
        assert_eq!(
            max_centered_area(Rect::new(0, 0, 160, 60)),
            Rect::new(1, 6, 158, 48)
        );
    }
}
