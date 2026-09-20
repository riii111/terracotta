use ratatui::layout::Rect;

use super::PlanReviewInput;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchInputState {
    query: String,
    cursor: usize,
    previous_query: String,
    previous_vertical: u16,
    previous_horizontal: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PlanReviewViewState {
    vertical: u16,
    horizontal: u16,
    search: Option<SearchInputState>,
}

impl PlanReviewViewState {
    pub(crate) fn apply(
        &mut self,
        input: PlanReviewInput,
        body: Rect,
        max_vertical: u16,
        max_horizontal: u16,
        current_query: &str,
    ) -> Option<String> {
        self.vertical = self.vertical.min(max_vertical);
        self.horizontal = self.horizontal.min(max_horizontal);
        if self.search.is_some() {
            return self.apply_search_input(input);
        }

        match input {
            PlanReviewInput::SearchStart => {
                let query = current_query.to_owned();
                self.search = Some(SearchInputState {
                    cursor: query.len(),
                    previous_query: query.clone(),
                    query,
                    previous_vertical: self.vertical,
                    previous_horizontal: self.horizontal,
                });
                None
            }
            PlanReviewInput::Up => self.scroll_vertical(-1, max_vertical),
            PlanReviewInput::Down => self.scroll_vertical(1, max_vertical),
            PlanReviewInput::Left => self.scroll_horizontal(-1, max_horizontal),
            PlanReviewInput::Right => self.scroll_horizontal(1, max_horizontal),
            PlanReviewInput::PageUp => {
                self.vertical = self.vertical.saturating_sub(body.height.max(1));
                None
            }
            PlanReviewInput::PageDown => {
                self.vertical = self
                    .vertical
                    .saturating_add(body.height.max(1))
                    .min(max_vertical);
                None
            }
            PlanReviewInput::Top => {
                self.vertical = 0;
                None
            }
            PlanReviewInput::Bottom => {
                self.vertical = max_vertical;
                None
            }
            PlanReviewInput::LeftEdge => {
                self.horizontal = 0;
                None
            }
            PlanReviewInput::RightEdge => {
                self.horizontal = max_horizontal;
                None
            }
            PlanReviewInput::SearchChar(_)
            | PlanReviewInput::SearchBackspace
            | PlanReviewInput::SearchLeft
            | PlanReviewInput::SearchRight
            | PlanReviewInput::SearchHome
            | PlanReviewInput::SearchEnd
            | PlanReviewInput::SearchConfirm
            | PlanReviewInput::SearchCancel
            | PlanReviewInput::Apply
            | PlanReviewInput::Copy
            | PlanReviewInput::Quit => None,
        }
    }

    pub(crate) const fn searching(&self) -> bool {
        self.search.is_some()
    }

    pub(crate) fn search_query(&self) -> Option<&str> {
        self.search.as_ref().map(|search| search.query.as_str())
    }

    pub(crate) const fn search_cursor(&self) -> Option<usize> {
        match &self.search {
            Some(search) => Some(search.cursor),
            None => None,
        }
    }

    pub(crate) const fn scroll(&self) -> (u16, u16) {
        (self.vertical, self.horizontal)
    }

    fn apply_search_input(&mut self, input: PlanReviewInput) -> Option<String> {
        let search = self.search.as_mut()?;
        match input {
            PlanReviewInput::SearchChar(character) => {
                search.query.insert(search.cursor, character);
                search.cursor += character.len_utf8();
                self.vertical = 0;
                self.horizontal = 0;
                Some(search.query.clone())
            }
            PlanReviewInput::SearchBackspace => {
                if search.cursor > 0 {
                    let previous = search.query[..search.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(index, _)| index);
                    search.query.drain(previous..search.cursor);
                    search.cursor = previous;
                    self.vertical = 0;
                    self.horizontal = 0;
                }
                Some(search.query.clone())
            }
            PlanReviewInput::SearchLeft => {
                search.cursor = search.query[..search.cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(index, _)| index);
                None
            }
            PlanReviewInput::SearchRight => {
                search.cursor = search.query[search.cursor..]
                    .char_indices()
                    .nth(1)
                    .map_or(search.query.len(), |(index, _)| search.cursor + index);
                None
            }
            PlanReviewInput::SearchHome => {
                search.cursor = 0;
                None
            }
            PlanReviewInput::SearchEnd => {
                search.cursor = search.query.len();
                None
            }
            PlanReviewInput::SearchConfirm => {
                self.search = None;
                None
            }
            PlanReviewInput::SearchCancel => {
                let (previous_query, previous_vertical, previous_horizontal) = (
                    search.previous_query.clone(),
                    search.previous_vertical,
                    search.previous_horizontal,
                );
                self.vertical = previous_vertical;
                self.horizontal = previous_horizontal;
                self.search = None;
                Some(previous_query)
            }
            _ => None,
        }
    }

    fn scroll_vertical(&mut self, delta: i16, max: u16) -> Option<String> {
        self.vertical = if delta.is_negative() {
            self.vertical.saturating_sub(delta.unsigned_abs())
        } else {
            self.vertical.saturating_add(delta.unsigned_abs()).min(max)
        };
        None
    }

    fn scroll_horizontal(&mut self, delta: i16, max: u16) -> Option<String> {
        self.horizontal = if delta.is_negative() {
            self.horizontal.saturating_sub(delta.unsigned_abs())
        } else {
            self.horizontal
                .saturating_add(delta.unsigned_abs())
                .min(max)
        };
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: Rect = Rect::new(0, 0, 10, 3);
    const MAX_VERTICAL: u16 = 20;
    const MAX_HORIZONTAL: u16 = 30;

    fn apply(view: &mut PlanReviewViewState, input: PlanReviewInput) -> Option<String> {
        view.apply(input, BODY, MAX_VERTICAL, MAX_HORIZONTAL, "existing")
    }

    #[test]
    fn search_input_changes_query_and_resets_scroll() {
        let mut view = PlanReviewViewState::default();
        apply(&mut view, PlanReviewInput::Bottom);
        apply(&mut view, PlanReviewInput::RightEdge);
        apply(&mut view, PlanReviewInput::SearchStart);

        assert_eq!(
            apply(&mut view, PlanReviewInput::SearchChar('a')),
            Some("existinga".to_owned())
        );
        assert_eq!(view.search_query(), Some("existinga"));
        assert_eq!(view.scroll(), (0, 0));
    }

    #[test]
    fn escape_restores_query_and_position() {
        let mut view = PlanReviewViewState::default();
        apply(&mut view, PlanReviewInput::Bottom);
        apply(&mut view, PlanReviewInput::RightEdge);
        apply(&mut view, PlanReviewInput::SearchStart);
        apply(&mut view, PlanReviewInput::SearchChar('a'));

        assert_eq!(
            apply(&mut view, PlanReviewInput::SearchCancel),
            Some("existing".to_owned())
        );
        assert!(!view.searching());
        assert_eq!(view.scroll(), (MAX_VERTICAL, MAX_HORIZONTAL));
    }

    #[test]
    fn search_right_moves_to_the_next_character_boundary() {
        let mut view = PlanReviewViewState::default();
        apply(&mut view, PlanReviewInput::SearchStart);
        apply(&mut view, PlanReviewInput::SearchHome);
        apply(&mut view, PlanReviewInput::SearchChar('あ'));
        apply(&mut view, PlanReviewInput::SearchChar('b'));
        apply(&mut view, PlanReviewInput::SearchHome);
        apply(&mut view, PlanReviewInput::SearchRight);
        apply(&mut view, PlanReviewInput::SearchChar('X'));

        assert_eq!(view.search_query(), Some("あXbexisting"));
        assert_eq!(view.search_cursor(), Some("あX".len()));
    }

    #[test]
    fn scroll_stays_within_the_calculated_offsets() {
        let mut view = PlanReviewViewState::default();

        for _ in 0..30 {
            apply(&mut view, PlanReviewInput::Down);
            apply(&mut view, PlanReviewInput::Right);
        }

        assert_eq!(view.scroll(), (MAX_VERTICAL, MAX_HORIZONTAL));
    }
}
