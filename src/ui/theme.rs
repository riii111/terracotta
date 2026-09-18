use ratatui::style::{Color, Modifier, Style};

use crate::app::plan::ResourceChangeKind;

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
