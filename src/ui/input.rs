use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitConfirmationInput {
    Confirm,
    Cancel,
    Consume,
    Forward(KeyEvent),
}

pub(crate) const fn quit_confirmation_key_to_input(key: KeyEvent) -> QuitConfirmationInput {
    match (key.code, key.modifiers) {
        (KeyCode::Enter, _) => QuitConfirmationInput::Confirm,
        (KeyCode::Esc, _) => QuitConfirmationInput::Cancel,
        (KeyCode::Char('q'), KeyModifiers::NONE) => QuitConfirmationInput::Consume,
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            QuitConfirmationInput::Consume
        }
        _ => QuitConfirmationInput::Forward(key),
    }
}

pub(super) fn normalize_key(mut key: KeyEvent) -> KeyEvent {
    if key.modifiers == KeyModifiers::CONTROL {
        let code = match key.code {
            KeyCode::Char('n') => Some(KeyCode::Down),
            KeyCode::Char('p') => Some(KeyCode::Up),
            KeyCode::Char('f') => Some(KeyCode::Right),
            KeyCode::Char('b') => Some(KeyCode::Left),
            _ => None,
        };
        if let Some(code) = code {
            key.code = code;
            key.modifiers = KeyModifiers::NONE;
        }
    }
    if let KeyCode::Char(character) = key.code
        && character.is_ascii_uppercase()
        && key.modifiers.contains(KeyModifiers::SHIFT)
    {
        key.modifiers.remove(KeyModifiers::SHIFT);
    }
    key
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn emacs_navigation_matches_arrow_keys() {
        for (character, arrow) in [
            ('n', KeyCode::Down),
            ('p', KeyCode::Up),
            ('f', KeyCode::Right),
            ('b', KeyCode::Left),
        ] {
            assert_eq!(
                normalize_key(KeyEvent::new(
                    KeyCode::Char(character),
                    KeyModifiers::CONTROL
                )),
                KeyEvent::new(arrow, KeyModifiers::NONE),
                "Ctrl+{character}"
            );
        }
    }

    #[rstest]
    #[case::uppercase_without_shift(KeyModifiers::NONE)]
    #[case::uppercase_with_redundant_shift(KeyModifiers::SHIFT)]
    fn uppercase_key_has_no_redundant_shift(#[case] modifiers: KeyModifiers) {
        let key = normalize_key(KeyEvent::new(KeyCode::Char('Y'), modifiers));

        assert_eq!(key.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn uppercase_normalization_preserves_non_shift_modifiers() {
        struct Case {
            name: &'static str,
            character: char,
            modifiers: KeyModifiers,
            expected: KeyModifiers,
        }

        let cases = [
            Case {
                name: "control",
                character: 'Y',
                modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                expected: KeyModifiers::CONTROL,
            },
            Case {
                name: "alt",
                character: 'Y',
                modifiers: KeyModifiers::ALT | KeyModifiers::SHIFT,
                expected: KeyModifiers::ALT,
            },
            Case {
                name: "control_and_alt",
                character: 'Y',
                modifiers: KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT,
                expected: KeyModifiers::CONTROL | KeyModifiers::ALT,
            },
            Case {
                name: "lowercase",
                character: 'y',
                modifiers: KeyModifiers::SHIFT,
                expected: KeyModifiers::SHIFT,
            },
        ];

        for case in cases {
            assert_eq!(
                normalize_key(KeyEvent::new(KeyCode::Char(case.character), case.modifiers,))
                    .modifiers,
                case.expected,
                "case: {}",
                case.name
            );
        }
    }

    #[test]
    fn quit_confirmation_key_mapping_preserves_the_raw_forwarded_key() {
        struct Case {
            name: &'static str,
            key: KeyEvent,
            expected: QuitConfirmationInput,
        }

        let cases = [
            Case {
                name: "enter_with_alt",
                key: KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
                expected: QuitConfirmationInput::Confirm,
            },
            Case {
                name: "esc_with_control",
                key: KeyEvent::new(KeyCode::Esc, KeyModifiers::CONTROL),
                expected: QuitConfirmationInput::Cancel,
            },
            Case {
                name: "q_without_modifiers",
                key: KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
                expected: QuitConfirmationInput::Consume,
            },
            Case {
                name: "control_c",
                key: KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                expected: QuitConfirmationInput::Consume,
            },
            Case {
                name: "q_with_control",
                key: KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
                expected: QuitConfirmationInput::Forward(KeyEvent::new(
                    KeyCode::Char('q'),
                    KeyModifiers::CONTROL,
                )),
            },
        ];

        for case in cases {
            assert_eq!(
                quit_confirmation_key_to_input(case.key),
                case.expected,
                "case: {}",
                case.name
            );
        }
    }
}
