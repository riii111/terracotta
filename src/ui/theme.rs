use ratatui::style::{Color, Modifier, Style};

pub(crate) fn footer_key_style() -> Style {
    Style::default().fg(Color::Rgb(0xd4, 0xa4, 0x85))
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

pub(crate) fn frame_style() -> Style {
    Style::default().fg(Color::Rgb(0x76, 0x7a, 0x84))
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

pub(crate) fn plan_line_style(line: &str) -> Style {
    match line.trim_start().chars().next() {
        Some('+') => Style::default().fg(Color::Rgb(0xa3, 0xbe, 0x8c)),
        Some('-') => Style::default().fg(Color::Rgb(0xbf, 0x61, 0x6a)),
        Some('~') => Style::default().fg(Color::Rgb(0xeb, 0xcb, 0x8b)),
        _ => body_style(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_change_markers_have_meaningful_colors() {
        assert_ne!(plan_line_style("+ create"), plan_line_style("- delete"));
        assert_ne!(plan_line_style("~ update"), body_style());
    }
}
