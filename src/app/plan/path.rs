use std::fmt::Write;

use super::{AttributePathSegment, ReplacePathSegment};

pub(crate) fn format_attribute_path(path: &[AttributePathSegment]) -> String {
    if path.is_empty() {
        return "<resource>".to_owned();
    }

    let mut formatted = String::new();
    for segment in path {
        match segment {
            AttributePathSegment::Key(key) => append_key_path(&mut formatted, key),
            AttributePathSegment::Index(index) => append_index_path(&mut formatted, *index),
        }
    }
    formatted
}

pub(crate) fn format_replace_path(path: &[ReplacePathSegment]) -> String {
    if path.is_empty() {
        return "<resource>".to_owned();
    }

    let mut formatted = String::new();
    for segment in path {
        match segment {
            ReplacePathSegment::Attribute(attribute) => {
                append_key_path(&mut formatted, attribute);
            }
            ReplacePathSegment::Index(index) => append_index_path(&mut formatted, *index),
        }
    }
    formatted
}

fn append_key_path(formatted: &mut String, key: &str) {
    if is_simple_path_key(key) {
        if !formatted.is_empty() {
            formatted.push('.');
        }
        formatted.push_str(key);
        return;
    }

    formatted.push('[');
    write!(formatted, "{key:?}").expect("writing attribute path should not fail");
    formatted.push(']');
}

fn append_index_path(formatted: &mut String, index: impl std::fmt::Display) {
    formatted.push('[');
    write!(formatted, "{index}").expect("writing attribute path should not fail");
    formatted.push(']');
}

fn is_simple_path_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AttributePathCase {
        name: &'static str,
        path: Vec<AttributePathSegment>,
        expected: &'static str,
    }

    #[test]
    fn format_attribute_path_uses_unambiguous_key_and_index_syntax() {
        let cases = [
            AttributePathCase {
                name: "empty",
                path: Vec::new(),
                expected: "<resource>",
            },
            AttributePathCase {
                name: "nested_simple_keys",
                path: vec![
                    AttributePathSegment::Key("a".to_owned()),
                    AttributePathSegment::Key("b".to_owned()),
                ],
                expected: "a.b",
            },
            AttributePathCase {
                name: "special_key_with_dot_and_array_index",
                path: vec![
                    AttributePathSegment::Key("a.b".to_owned()),
                    AttributePathSegment::Index(0),
                ],
                expected: r#"["a.b"][0]"#,
            },
            AttributePathCase {
                name: "empty_key",
                path: vec![AttributePathSegment::Key(String::new())],
                expected: r#"[""]"#,
            },
            AttributePathCase {
                name: "quoted_key",
                path: vec![AttributePathSegment::Key(r#"a"b"#.to_owned())],
                expected: r#"["a\"b"]"#,
            },
            AttributePathCase {
                name: "backslash_key",
                path: vec![AttributePathSegment::Key(r"a\b".to_owned())],
                expected: r#"["a\\b"]"#,
            },
            AttributePathCase {
                name: "unicode_key",
                path: vec![AttributePathSegment::Key("日本語".to_owned())],
                expected: "[\"日本語\"]",
            },
            AttributePathCase {
                name: "array_index",
                path: vec![
                    AttributePathSegment::Key("items".to_owned()),
                    AttributePathSegment::Index(0),
                    AttributePathSegment::Key("name".to_owned()),
                ],
                expected: "items[0].name",
            },
        ];

        for case in cases {
            assert_eq!(
                format_attribute_path(&case.path),
                case.expected,
                "case: {}",
                case.name
            );
        }
    }

    struct ReplacePathCase {
        name: &'static str,
        path: Vec<ReplacePathSegment>,
        expected: &'static str,
    }

    #[test]
    fn format_replace_path_uses_unambiguous_key_and_index_syntax() {
        let cases = [
            ReplacePathCase {
                name: "empty",
                path: Vec::new(),
                expected: "<resource>",
            },
            ReplacePathCase {
                name: "nested_simple_keys",
                path: vec![
                    ReplacePathSegment::Attribute("a".to_owned()),
                    ReplacePathSegment::Attribute("b".to_owned()),
                ],
                expected: "a.b",
            },
            ReplacePathCase {
                name: "special_key_with_dot_and_array_index",
                path: vec![
                    ReplacePathSegment::Attribute("a.b".to_owned()),
                    ReplacePathSegment::Index(0),
                ],
                expected: r#"["a.b"][0]"#,
            },
            ReplacePathCase {
                name: "empty_key",
                path: vec![ReplacePathSegment::Attribute(String::new())],
                expected: r#"[""]"#,
            },
            ReplacePathCase {
                name: "quoted_key",
                path: vec![ReplacePathSegment::Attribute(r#"a"b"#.to_owned())],
                expected: r#"["a\"b"]"#,
            },
            ReplacePathCase {
                name: "backslash_key",
                path: vec![ReplacePathSegment::Attribute(r"a\b".to_owned())],
                expected: r#"["a\\b"]"#,
            },
            ReplacePathCase {
                name: "unicode_key",
                path: vec![ReplacePathSegment::Attribute("日本語".to_owned())],
                expected: "[\"日本語\"]",
            },
            ReplacePathCase {
                name: "maximum_array_index",
                path: vec![
                    ReplacePathSegment::Attribute("items".to_owned()),
                    ReplacePathSegment::Index(u64::MAX),
                ],
                expected: "items[18446744073709551615]",
            },
        ];

        for case in cases {
            assert_eq!(
                format_replace_path(&case.path),
                case.expected,
                "case: {}",
                case.name
            );
        }
    }
}
