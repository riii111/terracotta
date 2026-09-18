use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::copy::CopyTarget;
use crate::app::review::{DetailAction, ResourceNavigation};
use crate::ui::input::normalize_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailInput {
    Action(DetailAction),
    Scroll(DetailScroll),
    ToggleSources,
    Navigate(ResourceNavigation),
    Copy(CopyTarget),
    Back,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailScroll {
    PageUp,
    PageDown,
}

pub(crate) fn key_to_input(key: KeyEvent) -> Option<DetailInput> {
    let key = normalize_key(key);

    if key.code == KeyCode::Esc {
        return Some(DetailInput::Back);
    }
    if matches!(key.code, KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return Some(DetailInput::Quit);
    }
    if key.modifiers == KeyModifiers::NONE {
        match key.code {
            KeyCode::Char('y') => return Some(DetailInput::Copy(CopyTarget::Resource)),
            KeyCode::Char('Y') => return Some(DetailInput::Copy(CopyTarget::Plan)),
            KeyCode::Char('s') => return Some(DetailInput::ToggleSources),
            _ => {}
        }
    }

    let action = match key.code {
        KeyCode::Up | KeyCode::Char('k') => DetailAction::SelectPrevious,
        KeyCode::Down | KeyCode::Char('j') => DetailAction::SelectNext,
        KeyCode::Char('[') => return Some(DetailInput::Navigate(ResourceNavigation::Previous)),
        KeyCode::Char(']') => return Some(DetailInput::Navigate(ResourceNavigation::Next)),
        KeyCode::Char('r') if key.modifiers == KeyModifiers::NONE => DetailAction::Reveal,
        KeyCode::Enter => DetailAction::ToggleExpansion,
        KeyCode::PageUp => return Some(DetailInput::Scroll(DetailScroll::PageUp)),
        KeyCode::PageDown => return Some(DetailInput::Scroll(DetailScroll::PageDown)),
        _ => return None,
    };
    Some(DetailInput::Action(action))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEventKind, KeyEventState};
    use rstest::rstest;

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[rstest]
    #[case::up_arrow(KeyCode::Up)]
    #[case::up_vim(KeyCode::Char('k'))]
    fn previous_selection_keys_map_to_previous_action(#[case] code: KeyCode) {
        assert_eq!(
            key_to_input(key(code)),
            Some(DetailInput::Action(DetailAction::SelectPrevious))
        );
    }

    #[rstest]
    #[case::down_arrow(KeyCode::Down)]
    #[case::down_vim(KeyCode::Char('j'))]
    fn next_selection_keys_map_to_next_action(#[case] code: KeyCode) {
        assert_eq!(
            key_to_input(key(code)),
            Some(DetailInput::Action(DetailAction::SelectNext))
        );
    }

    #[test]
    fn key_to_input_maps_non_selection_keys() {
        let cases = [
            (
                "page_down",
                key(KeyCode::PageDown),
                Some(DetailInput::Scroll(DetailScroll::PageDown)),
            ),
            (
                "page_up",
                key(KeyCode::PageUp),
                Some(DetailInput::Scroll(DetailScroll::PageUp)),
            ),
            ("back", key(KeyCode::Esc), Some(DetailInput::Back)),
            ("quit", key(KeyCode::Char('q')), Some(DetailInput::Quit)),
            (
                "copy_resource",
                key(KeyCode::Char('y')),
                Some(DetailInput::Copy(CopyTarget::Resource)),
            ),
            (
                "copy_plan",
                key(KeyCode::Char('Y')),
                Some(DetailInput::Copy(CopyTarget::Plan)),
            ),
            (
                "copy_plan_with_redundant_shift",
                key_with_modifiers(KeyCode::Char('Y'), KeyModifiers::SHIFT),
                Some(DetailInput::Copy(CopyTarget::Plan)),
            ),
            (
                "toggle_sources",
                key(KeyCode::Char('s')),
                Some(DetailInput::ToggleSources),
            ),
            (
                "control_s_does_not_toggle_sources",
                key_with_modifiers(KeyCode::Char('s'), KeyModifiers::CONTROL),
                None,
            ),
            (
                "expand",
                key(KeyCode::Enter),
                Some(DetailInput::Action(DetailAction::ToggleExpansion)),
            ),
            (
                "reveal",
                key(KeyCode::Char('r')),
                Some(DetailInput::Action(DetailAction::Reveal)),
            ),
            (
                "previous_resource",
                key(KeyCode::Char('[')),
                Some(DetailInput::Navigate(ResourceNavigation::Previous)),
            ),
            (
                "next_resource",
                key(KeyCode::Char(']')),
                Some(DetailInput::Navigate(ResourceNavigation::Next)),
            ),
            (
                "control_r_does_not_reveal",
                key_with_modifiers(KeyCode::Char('r'), KeyModifiers::CONTROL),
                None,
            ),
            (
                "alt_r_does_not_reveal",
                key_with_modifiers(KeyCode::Char('r'), KeyModifiers::ALT),
                None,
            ),
            (
                "uppercase_y_with_control_does_not_copy",
                key_with_modifiers(KeyCode::Char('Y'), KeyModifiers::CONTROL),
                None,
            ),
            (
                "uppercase_y_with_control_and_shift_does_not_copy",
                key_with_modifiers(
                    KeyCode::Char('Y'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                ),
                None,
            ),
            (
                "uppercase_y_with_alt_does_not_copy",
                key_with_modifiers(KeyCode::Char('Y'), KeyModifiers::ALT),
                None,
            ),
            (
                "uppercase_y_with_alt_and_shift_does_not_copy",
                key_with_modifiers(KeyCode::Char('Y'), KeyModifiers::ALT | KeyModifiers::SHIFT),
                None,
            ),
        ];

        for (name, input, expected) in cases {
            assert_eq!(key_to_input(input), expected, "case: {name}");
        }
    }
}
