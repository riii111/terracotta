use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn normalize_key(mut key: KeyEvent) -> KeyEvent {
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
}
