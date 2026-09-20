use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders};

use crate::ui::theme;

const HEADER_HEIGHT: u16 = 2;
pub(crate) const MAX_WIDTH: u16 = 120;
pub(crate) const MAX_HEIGHT: u16 = 40;

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

pub(crate) fn centered_area(area: Rect) -> Rect {
    let width = area.width.saturating_sub(2).min(MAX_WIDTH);
    let height = area.height.saturating_sub(2).min(MAX_HEIGHT);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
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
}
