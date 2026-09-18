use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::theme;

const SEPARATOR: &str = " | ";
const KEY_SEPARATOR: &str = "/";
const MAX_ROWS: usize = 2;

pub(crate) fn hint(alternative_keys: &[&'static str], description: &'static str) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, key) in alternative_keys.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                KEY_SEPARATOR,
                theme::footer_key_separator_style(),
            ));
        }
        spans.push(Span::styled(*key, theme::footer_key_style()));
    }
    spans.push(Span::styled(
        format!(" {description}"),
        theme::footer_text_style(),
    ));
    Line::from(spans)
}

pub(crate) fn layout(items: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    let mut rows = vec![Line::default()];
    let mut row_widths = vec![0usize];

    for item in items {
        let item_width = item.width();
        if item_width == 0 || item_width > width {
            continue;
        }

        let row_index = rows.len() - 1;
        let row = &mut rows[row_index];
        let separator_width = usize::from(!row.spans.is_empty()) * SEPARATOR.len();
        if row_widths[row_index] + separator_width + item_width <= width {
            if !row.spans.is_empty() {
                row.push_span(Span::styled(SEPARATOR, theme::footer_text_style()));
            }
            row.extend(item.spans);
            row_widths[row_index] += separator_width + item_width;
            continue;
        }

        if rows.len() == MAX_ROWS {
            continue;
        }

        rows.push(item);
        row_widths.push(item_width);
    }

    rows.into_iter()
        .filter(|row| !row.spans.is_empty())
        .collect()
}

pub(crate) fn render(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>) {
    frame.render_widget(
        Paragraph::new(lines).style(theme::footer_text_style()),
        area,
    );
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn rendered_keys_and_descriptions_use_rgb_colors_independent_of_ansi_palette() {
        let backend = ratatui::backend::TestBackend::new(16, 2);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    layout(
                        vec![hint(&["[", "]"], "prev/next"), hint(&["/"], "search")],
                        16,
                    ),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(0, 0)].fg,
            ratatui::style::Color::Rgb(0xd4, 0xa4, 0x85)
        );
        assert_eq!(
            buffer[(4, 0)].fg,
            ratatui::style::Color::Rgb(0xc0, 0xb8, 0xb8)
        );
        assert_eq!(buffer[(1, 0)].symbol(), "/");
        assert_eq!(
            buffer[(1, 0)].fg,
            ratatui::style::Color::Rgb(0x90, 0x90, 0x90)
        );
        assert_eq!(buffer[(0, 1)].symbol(), "/");
        assert_eq!(
            buffer[(0, 1)].fg,
            ratatui::style::Color::Rgb(0xd4, 0xa4, 0x85)
        );
    }

    #[rstest]
    #[case::single_row(80)]
    #[case::wrapped_rows(16)]
    fn key_and_description_colors_survive_layout(#[case] width: u16) {
        let rows = layout(
            vec![hint(&["[", "]"], "prev/next"), hint(&["/"], "search")],
            width,
        );
        let spans = rows.iter().flat_map(|line| &line.spans).collect::<Vec<_>>();

        for key in ["[", "]"] {
            let span = spans.iter().find(|span| span.content == key).unwrap();
            assert_eq!(span.style, theme::footer_key_style());
        }
        let slash_styles = spans
            .iter()
            .filter(|span| span.content == "/")
            .map(|span| span.style)
            .collect::<Vec<_>>();
        assert_eq!(
            slash_styles,
            vec![
                theme::footer_key_separator_style(),
                theme::footer_key_style()
            ]
        );
        for description in [" prev/next", " search"] {
            let span = spans
                .iter()
                .find(|span| span.content == description)
                .unwrap();
            assert_eq!(span.style, theme::footer_text_style());
        }
        assert_ne!(theme::footer_key_style().fg, theme::footer_text_style().fg);
    }

    #[test]
    fn alternative_keys_use_the_same_separator() {
        let cases: &[(&str, &[&str], &str)] = &[
            ("single", &["Enter"], "Enter confirm"),
            ("two", &["Enter", "Space"], "Enter/Space confirm"),
            (
                "three",
                &["Enter", "Space", "Ctrl-M"],
                "Enter/Space/Ctrl-M confirm",
            ),
        ];

        for (name, keys, expected) in cases {
            assert_eq!(hint(keys, "confirm").to_string(), *expected, "case: {name}");
        }
    }

    #[rstest]
    #[case::single_line(40, vec!["q quit", "Esc back"], vec!["q quit | Esc back"])]
    #[case::two_lines(16, vec!["q quit", "Esc back", "PgUp/PgDn scroll"], vec!["q quit", "Esc back"])]
    #[case::wide_item_is_skipped(8, vec!["q quit", "longer than width"], vec!["q quit"])]
    fn lays_out_complete_items_in_at_most_two_rows(
        #[case] width: u16,
        #[case] items: Vec<&str>,
        #[case] expected: Vec<&str>,
    ) {
        let actual = layout(
            items
                .into_iter()
                .map(|item| Line::from(item.to_owned()))
                .collect(),
            width,
        );

        assert_eq!(
            actual.iter().map(Line::to_string).collect::<Vec<_>>(),
            expected
        );
        assert!(actual.iter().all(|line| line.width() <= usize::from(width)));
        assert!(actual.len() <= MAX_ROWS);
    }
}
