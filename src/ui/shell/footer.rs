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
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Style};
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::single_row(80)]
    #[case::wrapped_rows(16)]
    fn rendered_keys_and_descriptions_use_rgb_colors_independent_of_ansi_palette(
        #[case] width: u16,
    ) {
        let backend = ratatui::backend::TestBackend::new(width, 2);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    layout(
                        vec![hint(&["[", "]"], "prev/next"), hint(&["/"], "search")],
                        width,
                    ),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_buffer_text_style(buffer, "[", 0, theme::footer_key_style());
        assert_buffer_text_style(buffer, "]", 0, theme::footer_key_style());
        assert_buffer_text_style(buffer, "/", 0, theme::footer_key_separator_style());
        assert_buffer_text_style(buffer, "/", 2, theme::footer_key_style());
        assert_buffer_text_style(buffer, " prev/next", 0, theme::footer_text_style());
        assert_buffer_text_style(buffer, " search", 0, theme::footer_text_style());
        if width == 80 {
            assert_buffer_text_style(buffer, " | ", 0, theme::footer_text_style());
        }
    }

    fn assert_buffer_text_style(buffer: &Buffer, text: &str, occurrence: usize, style: Style) {
        let mut matches = 0;
        let area = buffer.area();
        for y in area.y..area.bottom() {
            let symbols = (area.x..area.right())
                .map(|x| buffer.cell((x, y)).expect("footer cell").symbol())
                .collect::<Vec<_>>();
            for start in 0..symbols.len() {
                if !symbols[start..]
                    .iter()
                    .copied()
                    .collect::<String>()
                    .starts_with(text)
                {
                    continue;
                }
                if matches == occurrence {
                    for offset in 0..text.chars().count() {
                        let cell = buffer
                            .cell((area.x + u16::try_from(start + offset).unwrap(), y))
                            .expect("footer cell");
                        assert_eq!(cell.fg, style.fg.unwrap_or(Color::Reset));
                        assert_eq!(cell.bg, style.bg.unwrap_or(Color::Reset));
                        assert_eq!(cell.modifier, style.add_modifier);
                    }
                    return;
                }
                matches += 1;
            }
        }
        panic!("text occurrence not found: {text} #{occurrence}");
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
