use crate::app::session::Action;

use super::ApplyConfirmationInput;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ApplyConfirmationViewState {
    input: String,
    cursor: usize,
    scroll: u16,
    overlay: Option<ConfirmationOverlay>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmationOverlay {
    Help,
    Context,
}

impl ApplyConfirmationViewState {
    pub(crate) fn apply(
        &mut self,
        input: ApplyConfirmationInput,
        expected: &str,
    ) -> Option<Action> {
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
            ApplyConfirmationInput::Confirm if self.input == expected => {
                self.reset();
                Some(Action::ConfirmApply(expected.to_owned()))
            }
            ApplyConfirmationInput::Cancel => {
                self.reset();
                Some(Action::CancelApply)
            }
            ApplyConfirmationInput::ScrollUp => {
                self.scroll = self.scroll.saturating_sub(1);
                None
            }
            ApplyConfirmationInput::ScrollDown => {
                self.scroll = self.scroll.saturating_add(1);
                None
            }
            ApplyConfirmationInput::PageUp => {
                self.scroll = self.scroll.saturating_sub(5);
                None
            }
            ApplyConfirmationInput::PageDown => {
                self.scroll = self.scroll.saturating_add(5);
                None
            }
            ApplyConfirmationInput::OpenHelp => {
                self.overlay = Some(ConfirmationOverlay::Help);
                None
            }
            ApplyConfirmationInput::OpenContext => {
                self.overlay = Some(ConfirmationOverlay::Context);
                None
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

    pub(crate) const fn scroll(&self) -> u16 {
        self.scroll
    }

    pub(crate) const fn overlay(&self) -> Option<ConfirmationOverlay> {
        self.overlay
    }

    pub(crate) const fn close_overlay(&mut self) {
        self.overlay = None;
    }

    fn reset(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.scroll = 0;
        self.overlay = None;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn enter(view: &mut ApplyConfirmationViewState, value: &str) {
        for character in value.chars() {
            assert_eq!(
                view.apply(ApplyConfirmationInput::Character(character), "yes"),
                None
            );
        }
    }

    #[test]
    fn yes_and_no_confirmations_reset_input() {
        let mut view = ApplyConfirmationViewState::default();
        enter(&mut view, "yes");
        assert_eq!(
            view.apply(ApplyConfirmationInput::Confirm, "yes"),
            Some(Action::ConfirmApply("yes".to_owned()))
        );
        assert_eq!(view.input(), "");
        assert_eq!(view.cursor(), 0);

        enter(&mut view, "no");
        assert_eq!(
            view.apply(ApplyConfirmationInput::Confirm, "no"),
            Some(Action::ConfirmApply("no".to_owned()))
        );
        assert_eq!(view.input(), "");
        assert_eq!(view.cursor(), 0);
    }

    #[test]
    fn no_does_not_cancel_when_yes_is_required() {
        let mut view = ApplyConfirmationViewState::default();
        enter(&mut view, "no");

        assert_eq!(view.apply(ApplyConfirmationInput::Confirm, "yes"), None);
        assert_eq!(view.input(), "no");
    }

    #[test]
    fn escape_cancels_and_resets_input() {
        let mut view = ApplyConfirmationViewState::default();
        enter(&mut view, "maybe");

        assert_eq!(
            view.apply(ApplyConfirmationInput::Cancel, "yes"),
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

        assert_eq!(view.apply(ApplyConfirmationInput::Confirm, "yes"), None);
        assert_eq!(view.input(), value);
        assert_eq!(view.cursor(), cursor);
    }

    #[test]
    fn editing_preserves_utf8_character_boundaries() {
        let mut view = ApplyConfirmationViewState::default();
        enter(&mut view, "aあb");

        view.apply(ApplyConfirmationInput::Left, "yes");
        view.apply(ApplyConfirmationInput::Backspace, "yes");

        assert_eq!(view.input(), "ab");
        assert_eq!(view.cursor(), 1);
    }
}
