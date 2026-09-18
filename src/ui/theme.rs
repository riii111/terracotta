use ratatui::style::{Color, Modifier, Style};

use crate::app::plan::{AttributeChangeKind, ResourceChangeKind};

pub(crate) fn footer_key_style() -> Style {
    Style::default().fg(Color::Rgb(0xd4, 0xa4, 0x85))
}

pub(crate) fn footer_key_separator_style() -> Style {
    Style::default().fg(Color::Rgb(0x90, 0x90, 0x90))
}

pub(crate) fn footer_text_style() -> Style {
    Style::default().fg(Color::Rgb(0xc0, 0xb8, 0xb8))
}

pub(crate) fn diff_style(kind: AttributeChangeKind, after: bool) -> Style {
    if kind == AttributeChangeKind::Changed {
        Style::default().fg(if after {
            Color::Rgb(0xa3, 0xbe, 0x8c)
        } else {
            Color::Rgb(0xbf, 0x61, 0x6a)
        })
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
        ResourceChangeKind::Create => Color::Rgb(0xa3, 0xbe, 0x8c),
        ResourceChangeKind::Update => Color::Rgb(0xeb, 0xcb, 0x8b),
        ResourceChangeKind::Replace => Color::Rgb(0xb4, 0x8e, 0xad),
        ResourceChangeKind::Delete => Color::Rgb(0xbf, 0x61, 0x6a),
    };
    Style::default().fg(color)
}

pub(crate) fn review_style(needs_review: bool) -> Style {
    if needs_review {
        Style::default()
            .fg(Color::Rgb(0xeb, 0xcb, 0x8b))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

pub(crate) fn git_style(needs_review: bool) -> Style {
    if needs_review {
        Style::default()
    } else {
        Style::default().fg(Color::Rgb(0x97, 0xc9, 0xc3))
    }
}

pub(crate) fn warning_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xeb, 0xcb, 0x8b))
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_styles_keep_the_explicit_rgb_palette() {
        assert_eq!(footer_key_style().fg, Some(Color::Rgb(0xd4, 0xa4, 0x85)));
        assert_eq!(
            footer_key_separator_style().fg,
            Some(Color::Rgb(0x90, 0x90, 0x90))
        );
        assert_eq!(footer_text_style().fg, Some(Color::Rgb(0xc0, 0xb8, 0xb8)));
    }

    #[test]
    fn diff_styles_distinguish_changed_sides_and_dim_unchanged_values() {
        let before = diff_style(AttributeChangeKind::Changed, false);
        let after = diff_style(AttributeChangeKind::Changed, true);
        let unchanged = diff_style(AttributeChangeKind::Unchanged, false);

        assert_eq!(before.fg, Some(Color::Rgb(0xbf, 0x61, 0x6a)));
        assert_eq!(after.fg, Some(Color::Rgb(0xa3, 0xbe, 0x8c)));
        assert_eq!(unchanged, Style::default().add_modifier(Modifier::DIM));
    }

    #[test]
    fn action_styles_match_the_change_semantics() {
        let cases = [
            (
                ResourceChangeKind::Create,
                Color::Rgb(0xa3, 0xbe, 0x8c),
                "create",
            ),
            (
                ResourceChangeKind::Update,
                Color::Rgb(0xeb, 0xcb, 0x8b),
                "update",
            ),
            (
                ResourceChangeKind::Replace,
                Color::Rgb(0xb4, 0x8e, 0xad),
                "replace",
            ),
            (
                ResourceChangeKind::Delete,
                Color::Rgb(0xbf, 0x61, 0x6a),
                "delete",
            ),
        ];

        for (kind, expected, name) in cases {
            assert_eq!(action_style(kind).fg, Some(expected), "case: {name}");
        }
    }

    #[test]
    fn review_and_git_styles_keep_status_and_evidence_distinct() {
        assert_eq!(review_style(false), Style::default());
        assert_eq!(
            review_style(true),
            Style::default()
                .fg(Color::Rgb(0xeb, 0xcb, 0x8b))
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(git_style(true), Style::default());
        assert_eq!(git_style(false).fg, Some(Color::Rgb(0x97, 0xc9, 0xc3)));
    }

    #[test]
    fn warning_style_uses_the_attention_rgb_color() {
        assert_eq!(
            warning_style(),
            Style::default()
                .fg(Color::Rgb(0xeb, 0xcb, 0x8b))
                .add_modifier(Modifier::BOLD)
        );
    }
}
