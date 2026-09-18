use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::copy::CopyTarget;
use crate::app::review::{PlanListAction, PlanListState};
use crate::ui::input::normalize_key;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListInput {
    Selection(PlanListAction),
    Copy(CopyTarget),
    OpenDetail,
    OpenDiagnostics,
    StartSearch,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchInput {
    Insert(char),
    Delete,
    Confirm,
    Cancel,
    Quit,
}

pub(crate) fn key_to_list_input(key: KeyEvent) -> Option<ListInput> {
    let key = normalize_key(key);

    if matches!(key.code, KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return Some(ListInput::Quit);
    }
    if key.modifiers == KeyModifiers::NONE {
        match key.code {
            KeyCode::Char('y') => return Some(ListInput::Copy(CopyTarget::Resource)),
            KeyCode::Char('Y') => return Some(ListInput::Copy(CopyTarget::Plan)),
            KeyCode::Char('w') => return Some(ListInput::OpenDiagnostics),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Char('f') => Some(ListInput::Selection(PlanListAction::ToggleFilter)),
        KeyCode::Char('/') => Some(ListInput::StartSearch),
        KeyCode::Up | KeyCode::Char('k') => {
            Some(ListInput::Selection(PlanListAction::SelectPrevious))
        }
        KeyCode::Down | KeyCode::Char('j') => {
            Some(ListInput::Selection(PlanListAction::SelectNext))
        }
        KeyCode::Enter => Some(ListInput::OpenDetail),
        _ => None,
    }
}

pub(crate) fn search_key_to_input(key: KeyEvent) -> Option<SearchInput> {
    let key = normalize_key(key);

    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(SearchInput::Quit);
    }

    match key.code {
        KeyCode::Enter => Some(SearchInput::Confirm),
        KeyCode::Esc => Some(SearchInput::Cancel),
        KeyCode::Backspace => Some(SearchInput::Delete),
        KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(SearchInput::Insert(character))
        }
        _ => None,
    }
}

pub(crate) fn search_key_to_action(state: &PlanListState, key: KeyEvent) -> Option<PlanListAction> {
    match search_key_to_input(key) {
        Some(SearchInput::Confirm) => Some(PlanListAction::ConfirmSearch),
        Some(SearchInput::Cancel) => Some(PlanListAction::CancelSearch),
        Some(SearchInput::Delete) => {
            let mut search = state.search().to_owned();
            search.pop();
            Some(PlanListAction::SetSearch(search))
        }
        Some(SearchInput::Insert(character)) => {
            let mut search = state.search().to_owned();
            search.push(character);
            Some(PlanListAction::SetSearch(search))
        }
        Some(SearchInput::Quit) | None => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEventKind, KeyEventState};
    use rstest::rstest;

    use super::*;

    #[test]
    fn emacs_keys_navigate_without_toggling_filter_or_typing_into_search() {
        for (character, expected) in [
            ('n', Some(ListInput::Selection(PlanListAction::SelectNext))),
            (
                'p',
                Some(ListInput::Selection(PlanListAction::SelectPrevious)),
            ),
            ('f', None),
            ('b', None),
        ] {
            let input = key(KeyCode::Char(character), KeyModifiers::CONTROL);
            assert_eq!(key_to_list_input(input), expected, "Ctrl+{character}");
            assert_eq!(search_key_to_input(input), None, "Ctrl+{character}");
        }
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
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
            key_to_list_input(key(code, KeyModifiers::NONE)),
            Some(ListInput::Selection(PlanListAction::SelectPrevious))
        );
    }

    #[rstest]
    #[case::down_arrow(KeyCode::Down)]
    #[case::down_vim(KeyCode::Char('j'))]
    fn next_selection_keys_map_to_next_action(#[case] code: KeyCode) {
        assert_eq!(
            key_to_list_input(key(code, KeyModifiers::NONE)),
            Some(ListInput::Selection(PlanListAction::SelectNext))
        );
    }

    #[test]
    fn key_to_list_input_maps_non_selection_keys() {
        let cases = [
            (
                "quit",
                key(KeyCode::Char('q'), KeyModifiers::NONE),
                Some(ListInput::Quit),
            ),
            (
                "control_c_quit",
                key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                Some(ListInput::Quit),
            ),
            (
                "open_detail",
                key(KeyCode::Enter, KeyModifiers::NONE),
                Some(ListInput::OpenDetail),
            ),
            (
                "toggle_filter",
                key(KeyCode::Char('f'), KeyModifiers::NONE),
                Some(ListInput::Selection(PlanListAction::ToggleFilter)),
            ),
            (
                "start_search",
                key(KeyCode::Char('/'), KeyModifiers::NONE),
                Some(ListInput::StartSearch),
            ),
            (
                "copy_resource",
                key(KeyCode::Char('y'), KeyModifiers::NONE),
                Some(ListInput::Copy(CopyTarget::Resource)),
            ),
            (
                "copy_plan",
                key(KeyCode::Char('Y'), KeyModifiers::NONE),
                Some(ListInput::Copy(CopyTarget::Plan)),
            ),
            (
                "open_diagnostics",
                key(KeyCode::Char('w'), KeyModifiers::NONE),
                Some(ListInput::OpenDiagnostics),
            ),
            (
                "copy_plan_with_redundant_shift",
                key(KeyCode::Char('Y'), KeyModifiers::SHIFT),
                Some(ListInput::Copy(CopyTarget::Plan)),
            ),
            (
                "uppercase_y_with_control_does_not_copy",
                key(KeyCode::Char('Y'), KeyModifiers::CONTROL),
                None,
            ),
            (
                "uppercase_y_with_control_and_shift_does_not_copy",
                key(
                    KeyCode::Char('Y'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                ),
                None,
            ),
            (
                "uppercase_y_with_alt_does_not_copy",
                key(KeyCode::Char('Y'), KeyModifiers::ALT),
                None,
            ),
            (
                "uppercase_y_with_alt_and_shift_does_not_copy",
                key(KeyCode::Char('Y'), KeyModifiers::ALT | KeyModifiers::SHIFT),
                None,
            ),
        ];

        for (name, input, expected) in cases {
            assert_eq!(key_to_list_input(input), expected, "case: {name}");
        }
    }

    #[test]
    fn search_input_treats_regular_shortcut_keys_as_search_text() {
        assert_eq!(
            search_key_to_input(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(SearchInput::Insert('q'))
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Char('f'), KeyModifiers::NONE)),
            Some(SearchInput::Insert('f'))
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Char('y'), KeyModifiers::NONE)),
            Some(SearchInput::Insert('y'))
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(SearchInput::Quit)
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(SearchInput::Delete)
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(SearchInput::Confirm)
        );
        assert_eq!(
            search_key_to_input(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(SearchInput::Cancel)
        );
    }

    #[rstest]
    #[case::uppercase_without_shift(KeyModifiers::NONE)]
    #[case::uppercase_with_redundant_shift(KeyModifiers::SHIFT)]
    fn uppercase_y_is_search_text_during_search(#[case] modifiers: KeyModifiers) {
        assert_eq!(
            search_key_to_input(key(KeyCode::Char('Y'), modifiers)),
            Some(SearchInput::Insert('Y'))
        );
    }
}
