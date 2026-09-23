use ratatui::{style::Style, text::Line};

pub(crate) fn last_grapheme_boundary(text: &str) -> usize {
    grapheme_boundaries(text)
        .into_iter()
        .next_back()
        .unwrap_or(0)
}

pub(crate) fn previous_grapheme_boundary(text: &str, cursor: usize) -> usize {
    grapheme_boundaries(text)
        .into_iter()
        .rev()
        .find(|&boundary| boundary < cursor)
        .unwrap_or(0)
}

pub(crate) fn next_grapheme_boundary(text: &str, cursor: usize) -> usize {
    grapheme_boundaries(text)
        .into_iter()
        .find(|&boundary| boundary > cursor)
        .unwrap_or(text.len())
}

pub(crate) fn next_grapheme_boundary_at_or_after(text: &str, cursor: usize) -> usize {
    grapheme_boundaries(text)
        .into_iter()
        .find(|&boundary| boundary >= cursor)
        .unwrap_or(text.len())
}

fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries = vec![0];
    let mut offset = 0;
    for grapheme in Line::from(text).styled_graphemes(Style::default()) {
        offset += grapheme.symbol.len();
        boundaries.push(offset);
    }
    if boundaries.last().copied() != Some(text.len()) {
        boundaries.push(text.len());
    }
    boundaries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_follow_ratatui_graphemes_for_ascii_and_unicode() {
        let cases = [
            ("ascii", "ab", vec![0, 1, 2]),
            ("fullwidth", "あ界", vec![0, 3, 6]),
            ("combining", "e\u{301}x", vec![0, 3, 4]),
            ("zwj", "👩\u{200d}💻x", vec![0, 11, 12]),
        ];

        for (name, text, expected) in cases {
            let actual = grapheme_boundaries(text);

            assert_eq!(actual, expected, "{name}");
        }
    }
}
