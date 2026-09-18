use ratatui::style::{Color, Modifier, Style};

use crate::app::plan::{AttributeChangeKind, ResourceChangeKind};

pub(crate) fn footer_key_style() -> Style {
    Style::default().fg(Color::Rgb(0xd4, 0xa4, 0x85))
}

pub(crate) fn footer_text_style() -> Style {
    Style::default().fg(Color::Rgb(0xc0, 0xb8, 0xb8))
}

pub(crate) fn diff_style(kind: AttributeChangeKind, after: bool) -> Style {
    if kind == AttributeChangeKind::Changed {
        Style::default().fg(if after { Color::Green } else { Color::Red })
    } else {
        Style::default().add_modifier(Modifier::DIM)
    }
}

pub(crate) const fn action_symbol(kind: ResourceChangeKind) -> &'static str {
    match kind {
        ResourceChangeKind::Create => "+",
        ResourceChangeKind::Update => "~",
        ResourceChangeKind::Replace => "R",
        ResourceChangeKind::Delete => "-",
    }
}

pub(crate) fn action_style(kind: ResourceChangeKind) -> Style {
    let color = match kind {
        ResourceChangeKind::Create => Color::Green,
        ResourceChangeKind::Update => Color::Yellow,
        ResourceChangeKind::Replace => Color::Magenta,
        ResourceChangeKind::Delete => Color::Red,
    };
    Style::default().fg(color)
}

pub(crate) fn review_style(needs_review: bool) -> Style {
    if needs_review {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

pub(crate) fn git_style(needs_review: bool) -> Style {
    if needs_review {
        Style::default()
    } else {
        Style::default().fg(Color::Cyan)
    }
}

pub(crate) fn warning_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}
