use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::input::normalize_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverviewInput {
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Left,
    Right,
    ToggleExpand,
    Open,
    ViewPlan,
    Back,
    SearchStart,
    SearchChar(char),
    SearchBackspace,
    SearchLeft,
    SearchRight,
    SearchHome,
    SearchEnd,
    SearchConfirm,
    SearchCancel,
    FocusChanges,
    FocusRelations,
    ToggleMaximize,
    OpenHelp,
    OpenContext,
    Copy,
    Quit,
}

pub(crate) fn key_to_input(
    key: KeyEvent,
    searching: bool,
    filter_confirmed: bool,
) -> Option<OverviewInput> {
    let key = normalize_key(key);
    if searching {
        return search_key_to_input(key);
    }
    if let Some(input) = navigation_key_to_input(key) {
        return Some(input);
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(OverviewInput::Quit)
        }
        (KeyCode::Char('/'), KeyModifiers::NONE) => Some(OverviewInput::SearchStart),
        (KeyCode::Char('2'), KeyModifiers::NONE) => Some(OverviewInput::FocusChanges),
        (KeyCode::Char('3'), KeyModifiers::NONE) => Some(OverviewInput::FocusRelations),
        (KeyCode::Char('f'), KeyModifiers::NONE) => Some(OverviewInput::ToggleMaximize),
        (KeyCode::Esc, KeyModifiers::NONE) if filter_confirmed => Some(OverviewInput::SearchCancel),
        (KeyCode::Esc, KeyModifiers::NONE) => Some(OverviewInput::Back),
        (KeyCode::Char(' '), KeyModifiers::NONE) => Some(OverviewInput::ToggleExpand),
        (KeyCode::Enter, _) => Some(OverviewInput::Open),
        (KeyCode::Char('v'), KeyModifiers::NONE) => Some(OverviewInput::ViewPlan),
        (KeyCode::Char('y'), KeyModifiers::NONE) => Some(OverviewInput::Copy),
        (KeyCode::Char('?'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(OverviewInput::OpenHelp)
        }
        (KeyCode::Char('c'), KeyModifiers::NONE) => Some(OverviewInput::OpenContext),
        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(OverviewInput::Quit),
        _ => None,
    }
}

const fn navigation_key_to_input(key: KeyEvent) -> Option<OverviewInput> {
    match (key.code, key.modifiers) {
        (KeyCode::Up | KeyCode::Char('k'), _) => Some(OverviewInput::Up),
        (KeyCode::Down | KeyCode::Char('j'), _) => Some(OverviewInput::Down),
        (KeyCode::PageUp, _) => Some(OverviewInput::PageUp),
        (KeyCode::PageDown, _) => Some(OverviewInput::PageDown),
        (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => Some(OverviewInput::Top),
        (KeyCode::End, _) | (KeyCode::Char('G'), KeyModifiers::NONE) => Some(OverviewInput::Bottom),
        (KeyCode::Left, _) | (KeyCode::Char('h'), KeyModifiers::NONE) => Some(OverviewInput::Left),
        (KeyCode::Right, _) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
            Some(OverviewInput::Right)
        }
        _ => None,
    }
}

const fn search_key_to_input(key: KeyEvent) -> Option<OverviewInput> {
    match (key.code, key.modifiers) {
        (KeyCode::Enter, _) => Some(OverviewInput::SearchConfirm),
        (KeyCode::Esc, _) => Some(OverviewInput::SearchCancel),
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(OverviewInput::SearchCancel)
        }
        (KeyCode::Backspace, _) => Some(OverviewInput::SearchBackspace),
        (KeyCode::Home, _) => Some(OverviewInput::SearchHome),
        (KeyCode::End, _) => Some(OverviewInput::SearchEnd),
        (KeyCode::Left, _) => Some(OverviewInput::SearchLeft),
        (KeyCode::Right, _) => Some(OverviewInput::SearchRight),
        (KeyCode::Char('a'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(OverviewInput::SearchHome)
        }
        (KeyCode::Char('e'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(OverviewInput::SearchEnd)
        }
        (KeyCode::Char(character), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(OverviewInput::SearchChar(character))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_keys_cover_navigation_grouping_and_raw_plan() {
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
                false,
                false
            ),
            Some(OverviewInput::Down)
        );
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
                false,
                false
            ),
            Some(OverviewInput::ToggleExpand)
        );
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
                false,
                false
            ),
            Some(OverviewInput::ViewPlan)
        );
    }

    #[test]
    fn vim_navigation_keys_match_arrows_and_home_end() {
        for (character, expected) in [
            ('h', OverviewInput::Left),
            ('l', OverviewInput::Right),
            ('g', OverviewInput::Top),
            ('G', OverviewInput::Bottom),
        ] {
            assert_eq!(
                key_to_input(
                    KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
                    false,
                    false,
                ),
                Some(expected),
                "key: {character}"
            );
        }
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
                false,
                false,
            ),
            Some(OverviewInput::Bottom)
        );
    }

    #[test]
    fn search_keeps_vim_navigation_keys_as_query_text() {
        for character in ['h', 'j', 'k', 'l', 'g', 'G'] {
            assert_eq!(
                key_to_input(
                    KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
                    true,
                    false,
                ),
                Some(OverviewInput::SearchChar(character))
            );
        }
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
                true,
                false,
            ),
            Some(OverviewInput::SearchChar('G'))
        );
    }

    #[test]
    fn escape_clears_filter_before_leaving_overview() {
        assert_eq!(
            key_to_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), false, true),
            Some(OverviewInput::SearchCancel)
        );
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                false,
                false
            ),
            Some(OverviewInput::Back)
        );
    }

    #[test]
    fn single_environment_pane_keys_are_available_only_outside_search() {
        for (key, expected) in [
            ('2', OverviewInput::FocusChanges),
            ('3', OverviewInput::FocusRelations),
            ('f', OverviewInput::ToggleMaximize),
        ] {
            assert_eq!(
                key_to_input(
                    KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE),
                    false,
                    false,
                ),
                Some(expected)
            );
            assert_eq!(
                key_to_input(
                    KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE),
                    true,
                    false,
                ),
                Some(OverviewInput::SearchChar(key))
            );
        }
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
                false,
                false,
            ),
            None
        );
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
                false,
                false,
            ),
            None
        );
    }
}
