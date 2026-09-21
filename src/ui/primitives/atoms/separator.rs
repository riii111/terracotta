use ratatui::{
    style::{Color, Style},
    widgets::Paragraph,
};

pub(crate) fn render(width: u16) -> Paragraph<'static> {
    Paragraph::new("─".repeat(usize::from(width))).style(Style::default().fg(Color::DarkGray))
}
