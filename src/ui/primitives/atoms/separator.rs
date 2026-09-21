use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub(crate) fn render(width: u16) -> Paragraph<'static> {
    Paragraph::new("─".repeat(usize::from(width))).style(Style::default().fg(Color::DarkGray))
}

pub(crate) fn render_labeled(width: u16, label: &str) -> Paragraph<'static> {
    let width = usize::from(width);
    if width == 0 {
        return Paragraph::new(Line::default());
    }
    let label = format!(" {label} ");
    let label_width = label.chars().count().min(width);
    let line = format!(
        "{}{}",
        label.chars().take(label_width).collect::<String>(),
        "─".repeat(width.saturating_sub(label_width)),
    );
    Paragraph::new(Line::from(Span::styled(
        line,
        Style::default().fg(Color::DarkGray),
    )))
}
