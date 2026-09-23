use ratatui::widgets::Paragraph;

use crate::ui::theme;

pub(crate) fn render(width: u16) -> Paragraph<'static> {
    Paragraph::new("─".repeat(usize::from(width))).style(theme::separator_style())
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    use super::*;

    #[test]
    fn renders_with_a_visible_rgb_color() {
        let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(render(frame.area().width), frame.area()))
            .unwrap();

        assert_eq!(
            terminal.backend().buffer().cell((0, 0)).unwrap().fg,
            Color::Rgb(0x85, 0x8b, 0x94)
        );
    }
}
