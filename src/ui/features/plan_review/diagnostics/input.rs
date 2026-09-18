use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::input::normalize_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticsInput {
    Back,
    Quit,
    Scroll(DiagnosticsScroll),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticsScroll {
    Up,
    Down,
    PageUp,
    PageDown,
}

pub(crate) fn key_to_input(key: KeyEvent) -> Option<DiagnosticsInput> {
    let key = normalize_key(key);

    if key.code == KeyCode::Esc {
        return Some(DiagnosticsInput::Back);
    }
    if matches!(key.code, KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return Some(DiagnosticsInput::Quit);
    }
    if key.modifiers == KeyModifiers::NONE {
        return match key.code {
            KeyCode::Char('w') => Some(DiagnosticsInput::Back),
            KeyCode::Up | KeyCode::Char('k') => {
                Some(DiagnosticsInput::Scroll(DiagnosticsScroll::Up))
            }
            KeyCode::Down | KeyCode::Char('j') => {
                Some(DiagnosticsInput::Scroll(DiagnosticsScroll::Down))
            }
            KeyCode::PageUp => Some(DiagnosticsInput::Scroll(DiagnosticsScroll::PageUp)),
            KeyCode::PageDown => Some(DiagnosticsInput::Scroll(DiagnosticsScroll::PageDown)),
            _ => None,
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEventKind, KeyEventState};
    use rstest::rstest;

    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[rstest]
    #[case::up_arrow(KeyCode::Up, DiagnosticsScroll::Up)]
    #[case::up_vim(KeyCode::Char('k'), DiagnosticsScroll::Up)]
    #[case::down_arrow(KeyCode::Down, DiagnosticsScroll::Down)]
    #[case::down_vim(KeyCode::Char('j'), DiagnosticsScroll::Down)]
    #[case::page_up(KeyCode::PageUp, DiagnosticsScroll::PageUp)]
    #[case::page_down(KeyCode::PageDown, DiagnosticsScroll::PageDown)]
    fn scroll_keys_map_to_panel_scroll(#[case] code: KeyCode, #[case] scroll: DiagnosticsScroll) {
        assert_eq!(
            key_to_input(key(code, KeyModifiers::NONE)),
            Some(DiagnosticsInput::Scroll(scroll))
        );
    }

    #[test]
    fn back_and_quit_keys_are_consumed_by_the_panel() {
        assert_eq!(
            key_to_input(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(DiagnosticsInput::Back)
        );
        assert_eq!(
            key_to_input(key(KeyCode::Char('w'), KeyModifiers::NONE)),
            Some(DiagnosticsInput::Back)
        );
        assert_eq!(
            key_to_input(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(DiagnosticsInput::Quit)
        );
        assert_eq!(
            key_to_input(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(DiagnosticsInput::Quit)
        );
    }

    #[test]
    fn modified_panel_shortcuts_do_not_trigger_actions() {
        assert_eq!(
            key_to_input(key(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            key_to_input(key(KeyCode::Char('j'), KeyModifiers::ALT)),
            None
        );
    }
}
