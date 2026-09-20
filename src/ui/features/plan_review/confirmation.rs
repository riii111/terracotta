use crate::app::session::Action;

use super::ApplyConfirmationInput;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ApplyConfirmationViewState {
    input: String,
    cursor: usize,
}

impl ApplyConfirmationViewState {
    pub(crate) fn apply(&mut self, input: ApplyConfirmationInput) -> Option<Action> {
        match input {
            ApplyConfirmationInput::Character(character) => {
                self.input.insert(self.cursor, character);
                self.cursor += character.len_utf8();
                None
            }
            ApplyConfirmationInput::Backspace => {
                if self.cursor > 0 {
                    let previous = self.input[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(index, _)| index);
                    self.input.drain(previous..self.cursor);
                    self.cursor = previous;
                }
                None
            }
            ApplyConfirmationInput::Left => {
                self.cursor = self.input[..self.cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(index, _)| index);
                None
            }
            ApplyConfirmationInput::Right => {
                self.cursor = self.input[self.cursor..]
                    .char_indices()
                    .nth(1)
                    .map_or(self.input.len(), |(index, _)| self.cursor + index);
                None
            }
            ApplyConfirmationInput::Home => {
                self.cursor = 0;
                None
            }
            ApplyConfirmationInput::End => {
                self.cursor = self.input.len();
                None
            }
            ApplyConfirmationInput::Confirm if self.input == "yes" => {
                self.reset();
                Some(Action::ConfirmApply)
            }
            ApplyConfirmationInput::Confirm if self.input == "no" => {
                self.reset();
                Some(Action::CancelApply)
            }
            ApplyConfirmationInput::Cancel => {
                self.reset();
                Some(Action::CancelApply)
            }
            ApplyConfirmationInput::Confirm => None,
        }
    }

    pub(crate) const fn input(&self) -> &str {
        self.input.as_str()
    }

    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    fn reset(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn enter(view: &mut ApplyConfirmationViewState, value: &str) {
        for character in value.chars() {
            assert_eq!(
                view.apply(ApplyConfirmationInput::Character(character)),
                None
            );
        }
    }

    #[test]
    fn yes_and_no_confirmations_reset_input() {
        let mut view = ApplyConfirmationViewState::default();
        enter(&mut view, "yes");
        assert_eq!(
            view.apply(ApplyConfirmationInput::Confirm),
            Some(Action::ConfirmApply)
        );
        assert_eq!(view.input(), "");
        assert_eq!(view.cursor(), 0);

        enter(&mut view, "no");
        assert_eq!(
            view.apply(ApplyConfirmationInput::Confirm),
            Some(Action::CancelApply)
        );
        assert_eq!(view.input(), "");
        assert_eq!(view.cursor(), 0);
    }

    #[test]
    fn escape_cancels_and_resets_input() {
        let mut view = ApplyConfirmationViewState::default();
        enter(&mut view, "maybe");

        assert_eq!(
            view.apply(ApplyConfirmationInput::Cancel),
            Some(Action::CancelApply)
        );
        assert_eq!(view.input(), "");
        assert_eq!(view.cursor(), 0);
    }

    #[rstest]
    #[case::empty("")]
    #[case::invalid("maybe")]
    fn confirmation_requires_exact_yes_or_no(#[case] value: &str) {
        let mut view = ApplyConfirmationViewState::default();
        enter(&mut view, value);
        let cursor = view.cursor();

        assert_eq!(view.apply(ApplyConfirmationInput::Confirm), None);
        assert_eq!(view.input(), value);
        assert_eq!(view.cursor(), cursor);
    }

    #[test]
    fn editing_preserves_utf8_character_boundaries() {
        let mut view = ApplyConfirmationViewState::default();
        enter(&mut view, "aあb");

        view.apply(ApplyConfirmationInput::Left);
        view.apply(ApplyConfirmationInput::Backspace);

        assert_eq!(view.input(), "ab");
        assert_eq!(view.cursor(), 1);
    }
}
