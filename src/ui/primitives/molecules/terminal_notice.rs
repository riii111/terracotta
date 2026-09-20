use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Paragraph, Wrap};

pub(crate) fn render_wrapped(frame: &mut Frame<'_>, area: Rect, message: &'static str) {
    frame.render_widget(
        Paragraph::new(message)
            .wrap(Wrap { trim: false })
            .style(Style::default().add_modifier(Modifier::BOLD)),
        area,
    );
}
