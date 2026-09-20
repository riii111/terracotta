use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::input::normalize_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanReviewInput {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Top,
    Bottom,
    LeftEdge,
    RightEdge,
    Copy,
    Quit,
}

pub(crate) fn key_to_input(key: KeyEvent) -> Option<PlanReviewInput> {
    let key = normalize_key(key);
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(PlanReviewInput::Quit)
        }
        (KeyCode::Char('p'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(PlanReviewInput::Up)
        }
        (KeyCode::Char('n'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(PlanReviewInput::Down)
        }
        (KeyCode::Char('b'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(PlanReviewInput::Left)
        }
        (KeyCode::Char('f'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(PlanReviewInput::Right)
        }
        (KeyCode::Char('a'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(PlanReviewInput::LeftEdge)
        }
        (KeyCode::Char('e'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(PlanReviewInput::RightEdge)
        }
        (KeyCode::Char('<'), modifiers) if modifiers.contains(KeyModifiers::ALT) => {
            Some(PlanReviewInput::Top)
        }
        (KeyCode::Char('>'), modifiers) if modifiers.contains(KeyModifiers::ALT) => {
            Some(PlanReviewInput::Bottom)
        }
        (KeyCode::Char('v'), modifiers) if modifiers.contains(KeyModifiers::ALT) => {
            Some(PlanReviewInput::PageUp)
        }
        (KeyCode::Char('v'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(PlanReviewInput::PageDown)
        }
        (KeyCode::Up | KeyCode::Char('k'), _) => Some(PlanReviewInput::Up),
        (KeyCode::Down | KeyCode::Char('j'), _) => Some(PlanReviewInput::Down),
        (KeyCode::Left | KeyCode::Char('h'), _) => Some(PlanReviewInput::Left),
        (KeyCode::Right | KeyCode::Char('l'), _) => Some(PlanReviewInput::Right),
        (KeyCode::PageUp, _) => Some(PlanReviewInput::PageUp),
        (KeyCode::PageDown, _) => Some(PlanReviewInput::PageDown),
        (KeyCode::Home, _) => Some(PlanReviewInput::Top),
        (KeyCode::End, _) => Some(PlanReviewInput::Bottom),
        (KeyCode::Char('y'), KeyModifiers::NONE) => Some(PlanReviewInput::Copy),
        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(PlanReviewInput::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_list_and_detail_keys_have_no_special_actions() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_to_input(key), None);
    }
}
