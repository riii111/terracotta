use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::copy::CopyTarget;
use crate::app::execution::{ExecutionAction, ExecutionStage};
use crate::ui::input::normalize_key;

use super::ExecutionScroll;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionInput {
    Action(ExecutionAction),
    Scroll(ExecutionScroll),
    Copy(CopyTarget),
    End,
    Quit,
}

pub(crate) fn execution_key_to_input(
    key: KeyEvent,
    stage: ExecutionStage,
) -> Option<ExecutionInput> {
    let key = normalize_key(key);

    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(if stage == ExecutionStage::Failed {
            ExecutionInput::Quit
        } else {
            ExecutionInput::Action(ExecutionAction::RequestCancellation)
        });
    }

    if stage == ExecutionStage::Failed && key.code == KeyCode::Char('q') {
        return Some(ExecutionInput::Quit);
    }
    if stage == ExecutionStage::Failed
        && key.modifiers == KeyModifiers::NONE
        && key.code == KeyCode::Char('y')
    {
        return Some(ExecutionInput::Copy(CopyTarget::Diagnostic));
    }

    if key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(ExecutionInput::Scroll(ExecutionScroll::LeftEdge));
    }
    if key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(ExecutionInput::Scroll(ExecutionScroll::RightEdge));
    }
    if key.code == KeyCode::Char('<') && key.modifiers.contains(KeyModifiers::ALT) {
        return Some(ExecutionInput::Scroll(ExecutionScroll::Top));
    }
    if key.code == KeyCode::Char('>') && key.modifiers.contains(KeyModifiers::ALT) {
        return Some(ExecutionInput::End);
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(ExecutionInput::Scroll(ExecutionScroll::Up)),
        KeyCode::Down | KeyCode::Char('j') => Some(ExecutionInput::Scroll(ExecutionScroll::Down)),
        KeyCode::Left | KeyCode::Char('h') => Some(ExecutionInput::Scroll(ExecutionScroll::Left)),
        KeyCode::Right | KeyCode::Char('l') => Some(ExecutionInput::Scroll(ExecutionScroll::Right)),
        KeyCode::PageUp => Some(ExecutionInput::Scroll(ExecutionScroll::PageUp)),
        KeyCode::PageDown => Some(ExecutionInput::Scroll(ExecutionScroll::PageDown)),
        KeyCode::Home => Some(ExecutionInput::Scroll(ExecutionScroll::Top)),
        KeyCode::End => Some(ExecutionInput::End),
        _ => None,
    }
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

    #[test]
    fn key_mapping_respects_execution_stage() {
        let cases = [
            (
                "control_c_cancels_running",
                key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                ExecutionStage::Planning,
                Some(ExecutionInput::Action(ExecutionAction::RequestCancellation)),
            ),
            (
                "q_is_ignored_while_running",
                key(KeyCode::Char('q'), KeyModifiers::NONE),
                ExecutionStage::Planning,
                None,
            ),
            (
                "q_quits_failed",
                key(KeyCode::Char('q'), KeyModifiers::NONE),
                ExecutionStage::Failed,
                Some(ExecutionInput::Quit),
            ),
            (
                "control_c_quits_failed",
                key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                ExecutionStage::Failed,
                Some(ExecutionInput::Quit),
            ),
            (
                "y_is_ignored_while_running",
                key(KeyCode::Char('y'), KeyModifiers::NONE),
                ExecutionStage::Planning,
                None,
            ),
            (
                "y_copies_diagnostic_after_failure",
                key(KeyCode::Char('y'), KeyModifiers::NONE),
                ExecutionStage::Failed,
                Some(ExecutionInput::Copy(CopyTarget::Diagnostic)),
            ),
        ];

        for (name, input, stage, expected) in cases {
            assert_eq!(
                execution_key_to_input(input, stage),
                expected,
                "case: {name}"
            );
        }
    }

    #[rstest]
    #[case::plain(KeyModifiers::NONE)]
    #[case::shift(KeyModifiers::SHIFT)]
    #[case::control(KeyModifiers::CONTROL)]
    #[case::control_with_redundant_shift(KeyModifiers::CONTROL | KeyModifiers::SHIFT)]
    #[case::alt(KeyModifiers::ALT)]
    #[case::alt_with_redundant_shift(KeyModifiers::ALT | KeyModifiers::SHIFT)]
    fn uppercase_y_does_not_copy_after_failure(#[case] modifiers: KeyModifiers) {
        assert_eq!(
            execution_key_to_input(key(KeyCode::Char('Y'), modifiers), ExecutionStage::Failed),
            None
        );
    }

    #[rstest]
    #[case::lowercase(KeyCode::Char('y'), KeyModifiers::NONE)]
    #[case::uppercase(KeyCode::Char('Y'), KeyModifiers::NONE)]
    #[case::uppercase_with_redundant_shift(KeyCode::Char('Y'), KeyModifiers::SHIFT)]
    fn copy_keys_are_ignored_while_running(#[case] code: KeyCode, #[case] modifiers: KeyModifiers) {
        assert_eq!(
            execution_key_to_input(key(code, modifiers), ExecutionStage::Planning),
            None
        );
    }
}
