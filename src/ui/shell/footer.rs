use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

const SEPARATOR: &str = " | ";
const MAX_ROWS: usize = 2;

pub(crate) fn layout(items: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    let mut rows = vec![String::new()];
    let mut row_widths = vec![0usize];

    for item in items {
        let item_width = item.width();
        if item_width == 0 || item_width > width {
            continue;
        }

        let row_index = rows.len() - 1;
        let row = &mut rows[row_index];
        let separator_width = usize::from(!row.is_empty()) * SEPARATOR.len();
        if row_widths[row_index] + separator_width + item_width <= width {
            if !row.is_empty() {
                row.push_str(SEPARATOR);
            }
            row.push_str(item.to_string().as_str());
            row_widths[row_index] += separator_width + item_width;
            continue;
        }

        if rows.len() == MAX_ROWS {
            continue;
        }

        rows.push(item.to_string());
        row_widths.push(item_width);
    }

    rows.into_iter()
        .filter(|row| !row.is_empty())
        .map(Line::from)
        .collect()
}

pub(crate) fn render(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>) {
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

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
