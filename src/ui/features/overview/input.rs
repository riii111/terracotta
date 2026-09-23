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
    OpenEnvironmentFilter,
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
        (KeyCode::Esc, KeyModifiers::NONE) if filter_confirmed => Some(OverviewInput::SearchCancel),
        (KeyCode::Esc, KeyModifiers::NONE) => Some(OverviewInput::Back),
        (KeyCode::Char(' '), KeyModifiers::NONE) => Some(OverviewInput::ToggleExpand),
        (KeyCode::Enter, _) => Some(OverviewInput::Open),
        (KeyCode::Char('v'), KeyModifiers::NONE) => Some(OverviewInput::ViewPlan),
        (KeyCode::Char('e'), KeyModifiers::NONE) => Some(OverviewInput::OpenEnvironmentFilter),
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
        (KeyCode::Home, _) => Some(OverviewInput::Top),
        (KeyCode::End, _) => Some(OverviewInput::Bottom),
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
    fn search_keeps_vim_navigation_keys_as_query_text() {
        for character in ['j', 'k'] {
            assert_eq!(
                key_to_input(
                    KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
                    true,
                    false,
                ),
                Some(OverviewInput::SearchChar(character))
            );
        }
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
    fn e_opens_the_temporary_environment_filter() {
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
                false,
                false,
            ),
            Some(OverviewInput::OpenEnvironmentFilter)
        );
        assert_eq!(
            key_to_input(
                KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
                true,
                false,
            ),
            Some(OverviewInput::SearchChar('e'))
        );
    }
}
