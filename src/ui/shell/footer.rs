use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
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

pub(crate) fn quit_confirmation_lines(width: u16, notice: Option<&str>) -> Vec<Line<'static>> {
    let available = available_width(width, notice);
    let full = quit_confirmation_line();
    let compact = compact_quit_confirmation_line();
    let line = if full.width() <= usize::from(available) {
        full
    } else if compact.width() <= usize::from(available) {
        compact
    } else {
        minimal_quit_confirmation_line()
    };
    layout_with_notice(vec![line], width, notice)
}

fn quit_confirmation_line() -> Line<'static> {
    Line::from(vec![
        Span::styled("Quit Terracotta? ", theme::footer_text_style()),
        Span::styled("Enter", theme::footer_key_style()),
        Span::styled(" quit | ", theme::footer_text_style()),
        Span::styled("Esc", theme::footer_key_style()),
        Span::styled(" cancel", theme::footer_text_style()),
    ])
}

fn compact_quit_confirmation_line() -> Line<'static> {
    Line::from(vec![
        Span::styled("Quit? ", theme::footer_text_style()),
        Span::styled("Enter", theme::footer_key_style()),
        Span::styled(" exit ", theme::footer_text_style()),
        Span::styled("/", theme::footer_key_separator_style()),
        Span::styled(" ", theme::footer_text_style()),
        Span::styled("Esc", theme::footer_key_style()),
        Span::styled(" cancel", theme::footer_text_style()),
    ])
}

fn minimal_quit_confirmation_line() -> Line<'static> {
    Line::from(vec![
        Span::styled("Quit? ", theme::footer_text_style()),
        Span::styled("Enter", theme::footer_key_style()),
        Span::styled("/", theme::footer_key_separator_style()),
        Span::styled("Esc", theme::footer_key_style()),
    ])
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

pub(crate) fn layout_with_notice(
    items: Vec<Line<'static>>,
    width: u16,
    notice: Option<&str>,
) -> Vec<Line<'static>> {
    layout(items, available_width(width, notice))
}

pub(crate) fn available_width(width: u16, notice: Option<&str>) -> u16 {
    let Some(notice) = notice else {
        return width;
    };
    let notice_width = u16::try_from(Line::from(notice).width()).unwrap_or(u16::MAX);
    width.saturating_sub(notice_width.saturating_add(1))
}

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    lines: &[Line<'static>],
    notice: Option<(&str, Style)>,
) {
    frame.render_widget(
        Paragraph::new(lines.to_owned()).style(theme::footer_text_style()),
        area,
    );
    let Some((message, style)) = notice else {
        return;
    };
    let notice_width = u16::try_from(Line::from(message).width())
        .unwrap_or(u16::MAX)
        .min(area.width);
    if notice_width == 0 || area.height == 0 {
        return;
    }
    let notice_area = Rect::new(
        area.right().saturating_sub(notice_width),
        area.y
            .saturating_add(u16::try_from(lines.len().saturating_sub(1)).unwrap_or(u16::MAX)),
        notice_width,
        1,
    );
    frame.render_widget(
        Paragraph::new(message)
            .style(style)
            .alignment(Alignment::Right),
        notice_area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};
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
                    &layout(
                        vec![hint(&["[", "]"], "prev/next"), hint(&["/"], "search")],
                        width,
                    ),
                    None,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_buffer_text_style(
            buffer,
            "[",
            0,
            Color::Rgb(0xe9, 0xdb, 0xdb),
            Color::Reset,
            Modifier::empty(),
        );
        assert_buffer_text_style(
            buffer,
            "]",
            0,
            Color::Rgb(0xe9, 0xdb, 0xdb),
            Color::Reset,
            Modifier::empty(),
        );
        assert_buffer_text_style(
            buffer,
            "/",
            0,
            Color::Rgb(0x90, 0x90, 0x90),
            Color::Reset,
            Modifier::empty(),
        );
        assert_buffer_text_style(
            buffer,
            "/",
            2,
            Color::Rgb(0xe9, 0xdb, 0xdb),
            Color::Reset,
            Modifier::empty(),
        );
        assert_buffer_text_style(
            buffer,
            " prev/next",
            0,
            Color::Rgb(0xc0, 0xb8, 0xb8),
            Color::Reset,
            Modifier::empty(),
        );
        assert_buffer_text_style(
            buffer,
            " search",
            0,
            Color::Rgb(0xc0, 0xb8, 0xb8),
            Color::Reset,
            Modifier::empty(),
        );
        if width == 80 {
            assert_buffer_text_style(
                buffer,
                " | ",
                0,
                Color::Rgb(0xc0, 0xb8, 0xb8),
                Color::Reset,
                Modifier::empty(),
            );
        }
    }

    #[test]
    fn notice_stays_at_the_right_edge_and_reserves_left_footer_space() {
        let width = 32;
        let notice = "Copied.";
        let lines = layout_with_notice(
            vec![
                hint(&["Ctrl-C"], "cancel"),
                hint(&["↑", "↓", "PgUp", "PgDn"], "scroll"),
                hint(&["End"], "follow latest"),
            ],
            width,
            Some(notice),
        );
        assert!(
            lines
                .iter()
                .all(|line| { line.width() <= usize::from(available_width(width, Some(notice))) })
        );

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 2)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &lines,
                    Some((notice, theme::accent_style())),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let notice_width = notice.chars().count();
        for y in 0..buffer.area().height {
            for x in 0..width {
                let found = notice.chars().enumerate().all(|(offset, character)| {
                    let Some(cell) = buffer.cell((x + u16::try_from(offset).unwrap(), y)) else {
                        return false;
                    };

                    cell.symbol() == character.to_string()
                });

                if found {
                    assert_eq!(usize::from(x) + notice_width, usize::from(width));
                    return;
                }
            }
        }
        panic!("footer notice should be rendered");
    }

    fn assert_buffer_text_style(
        buffer: &Buffer,
        text: &str,
        occurrence: usize,
        foreground: Color,
        background: Color,
        modifier: Modifier,
    ) {
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
                        assert_eq!(cell.fg, foreground);
                        assert_eq!(cell.bg, background);
                        assert_eq!(cell.modifier, modifier);
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

    #[test]
    fn quit_confirmation_uses_the_full_prompt_when_it_fits() {
        let lines = quit_confirmation_lines(80, None);

        assert_eq!(
            lines.iter().map(Line::to_string).collect::<Vec<_>>(),
            vec!["Quit Terracotta? Enter quit | Esc cancel".to_owned()]
        );
    }

    #[test]
    fn quit_confirmation_uses_a_short_prompt_when_the_footer_is_narrow() {
        let lines = quit_confirmation_lines(32, None);

        assert_eq!(
            lines.iter().map(Line::to_string).collect::<Vec<_>>(),
            vec!["Quit? Enter exit / Esc cancel".to_owned()]
        );
    }

    #[test]
    fn quit_confirmation_keeps_a_copy_notice_visible_when_the_footer_is_narrow() {
        let lines = quit_confirmation_lines(32, Some("Copied."));

        assert_eq!(
            lines.iter().map(Line::to_string).collect::<Vec<_>>(),
            vec!["Quit? Enter/Esc".to_owned()]
        );
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
