use ratatui::style::{Color, Modifier, Style};

pub(crate) fn footer_key_style() -> Style {
    body_style()
}

pub(crate) fn footer_key_separator_style() -> Style {
    Style::default().fg(Color::Rgb(0x90, 0x90, 0x90))
}

pub(crate) fn footer_text_style() -> Style {
    Style::default().fg(Color::Rgb(0xc0, 0xb8, 0xb8))
}

pub(crate) fn body_style() -> Style {
    Style::default().fg(Color::Rgb(0xe9, 0xdb, 0xdb))
}

pub(crate) fn secondary_style() -> Style {
    Style::default().fg(Color::Rgb(0xc0, 0xb8, 0xb8))
}

pub(crate) fn accent_style() -> Style {
    Style::default().fg(Color::Rgb(0xf4, 0x9e, 0x4c))
}

pub(crate) fn search_match_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0x11, 0x14, 0x19))
        .bg(Color::Rgb(0xf4, 0x9e, 0x4c))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn selected_row_style() -> Style {
    body_style().bg(Color::Rgb(0x35, 0x35, 0x3d))
}

pub(crate) fn search_cursor_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0x11, 0x14, 0x19))
        .bg(Color::Rgb(0xf4, 0x9e, 0x4c))
}

pub(crate) fn selected_search_match_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0x11, 0x14, 0x19))
        .bg(Color::Rgb(0xff, 0xd0, 0x8a))
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

pub(crate) fn copy_flash_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0x11, 0x14, 0x19))
        .bg(Color::Rgb(0xf4, 0x9e, 0x4c))
}

pub(crate) fn frame_style() -> Style {
    Style::default().fg(Color::Rgb(0x76, 0x7a, 0x84))
}

pub(crate) fn separator_style() -> Style {
    Style::default().fg(Color::Rgb(0x85, 0x8b, 0x94))
}

pub(crate) fn scrollbar_thumb_style() -> Style {
    Style::default().fg(Color::Rgb(0xc0, 0xb8, 0xb0))
}

pub(crate) fn scrollbar_track_style() -> Style {
    Style::default().fg(Color::Rgb(0x50, 0x52, 0x5e))
}

pub(crate) fn warning_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xeb, 0xcb, 0x8b))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn error_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xbf, 0x61, 0x6a))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn success_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xa3, 0xbe, 0x8c))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn plan_line_style(line: &str) -> Style {
    match line.trim_start().chars().next() {
        Some('+') => Style::default().fg(Color::Rgb(0xa3, 0xbe, 0x8c)),
        Some('-') => Style::default().fg(Color::Rgb(0xbf, 0x61, 0x6a)),
        Some('~') => Style::default().fg(Color::Rgb(0xeb, 0xcb, 0x8b)),
        _ => body_style(),
    }
}

pub(crate) fn plan_note_style() -> Style {
    secondary_style()
}
