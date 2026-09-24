use std::fmt::Write;

use super::{ReplacePathSegment, attribute_diff::AttributePathSegment};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedResourceAddress {
    normalized: String,
    display: String,
}

impl NormalizedResourceAddress {
    #[must_use]
    pub(crate) fn normalized(&self) -> &str {
        &self.normalized
    }

    #[must_use]
    pub(crate) fn display(&self) -> &str {
        &self.display
    }
}

#[must_use]
pub(crate) fn normalize_resource_address(address: &str) -> Option<NormalizedResourceAddress> {
    normalize_resource_addresses([address])
}

pub(crate) fn normalize_resource_addresses<'a>(
    addresses: impl IntoIterator<Item = &'a str>,
) -> Option<NormalizedResourceAddress> {
    let mut merged: Vec<(String, usize)> = Vec::new();
    for address in addresses {
        let mut parts: Vec<(String, usize)> = Vec::new();
        for component in parse_address_components(address)? {
            match component {
                AddressComponent::Name(name) => parts.push((name, 0)),
                AddressComponent::InstanceKey(_) => parts.last_mut()?.1 += 1,
            }
        }
        if merged.is_empty() {
            merged = parts;
            continue;
        }
        if merged.len() != parts.len() {
            return None;
        }
        for ((name, keys), (other_name, other_keys)) in merged.iter_mut().zip(parts) {
            if *name != other_name {
                return None;
            }
            *keys = (*keys).max(other_keys);
        }
    }
    if merged.is_empty() {
        return None;
    }
    let mut normalized = String::new();
    let mut display = String::new();
    for (name, keys) in merged {
        if !normalized.is_empty() {
            normalized.push('.');
            display.push('.');
        }
        normalized.push_str(&name);
        display.push_str(&name);
        display.push_str(&"[*]".repeat(keys));
    }
    Some(NormalizedResourceAddress {
        normalized,
        display,
    })
}

pub(crate) fn resource_address_matches_block(block: &str, candidate: &str) -> bool {
    let (Some(block), Some(candidate)) = (address_segments(block), address_segments(candidate))
    else {
        return false;
    };
    block.len() == candidate.len()
        && block.iter().zip(candidate).all(|(block, candidate)| {
            block.name == candidate.name
                && (block.instance_keys.is_empty()
                    || block.instance_keys == candidate.instance_keys)
        })
}

struct AddressSegment {
    name: String,
    instance_keys: Vec<String>,
}

fn address_segments(address: &str) -> Option<Vec<AddressSegment>> {
    let mut segments: Vec<AddressSegment> = Vec::new();
    for component in parse_address_components(address)? {
        match component {
            AddressComponent::Name(name) => segments.push(AddressSegment {
                name,
                instance_keys: Vec::new(),
            }),
            AddressComponent::InstanceKey(key) => segments.last_mut()?.instance_keys.push(key),
        }
    }
    (!segments.is_empty()).then_some(segments)
}

enum AddressComponent {
    Name(String),
    InstanceKey(String),
}

fn parse_address_components(address: &str) -> Option<Vec<AddressComponent>> {
    let bytes = address.as_bytes();
    let mut position = 0;
    let mut components = Vec::new();

    while position < bytes.len() {
        let name_start = position;
        while position < bytes.len() && !matches!(bytes[position], b'.' | b'[') {
            position += 1;
        }
        if position == name_start {
            return None;
        }
        components.push(AddressComponent::Name(
            address.get(name_start..position)?.to_owned(),
        ));

        while bytes.get(position) == Some(&b'[') {
            let key_start = position;
            position = parse_instance_key(bytes, position)?;
            components.push(AddressComponent::InstanceKey(
                address.get(key_start..position)?.to_owned(),
            ));
        }

        match bytes.get(position) {
            None => return Some(components),
            Some(b'.') => {
                position += 1;
                if position == bytes.len() {
                    return None;
                }
            }
            Some(_) => return None,
        }
    }

    None
}

fn parse_instance_key(bytes: &[u8], mut position: usize) -> Option<usize> {
    position += 1;
    match bytes.get(position) {
        Some(b'"') => {
            position += 1;
            let mut escaped = false;
            while let Some(byte) = bytes.get(position) {
                if escaped {
                    escaped = false;
                    position += 1;
                    continue;
                }
                match byte {
                    b'\\' => escaped = true,
                    b'"' => {
                        position += 1;
                        return (bytes.get(position) == Some(&b']')).then_some(position + 1);
                    }
                    _ => {}
                }
                position += 1;
            }
            None
        }
        Some(byte) if byte.is_ascii_digit() => {
            while bytes.get(position).is_some_and(u8::is_ascii_digit) {
                position += 1;
            }
            (bytes.get(position) == Some(&b']')).then_some(position + 1)
        }
        _ => None,
    }
}

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

    struct AddressCase {
        name: &'static str,
        input: &'static str,
        normalized: &'static str,
        display: &'static str,
    }

    #[test]
    fn removes_all_module_and_resource_instance_keys_without_regex_replacement() {
        let cases = [
            AddressCase {
                name: "resource_count",
                input: "aws_instance.web[0]",
                normalized: "aws_instance.web",
                display: "aws_instance.web[*]",
            },
            AddressCase {
                name: "resource_for_each",
                input: r#"aws_instance.web["blue.green"]"#,
                normalized: "aws_instance.web",
                display: "aws_instance.web[*]",
            },
            AddressCase {
                name: "nested_modules_with_mixed_keys",
                input: r#"module.network["prod[0]"].module.zone[3].aws_instance.web["a.b"]"#,
                normalized: "module.network.module.zone.aws_instance.web",
                display: "module.network[*].module.zone[*].aws_instance.web[*]",
            },
        ];

        for case in cases {
            let address = normalize_resource_address(case.input).expect("address should parse");
            assert_eq!(address.normalized(), case.normalized, "case: {}", case.name);
            assert_eq!(address.display(), case.display, "case: {}", case.name);
        }
    }

    #[test]
    fn rejects_malformed_instance_keys_instead_of_grouping_them() {
        for address in [
            "aws_instance.web[]",
            "aws_instance.web[true]",
            "aws_instance.web[\"unterminated]",
            "aws_instance.web[0].",
        ] {
            assert!(
                normalize_resource_address(address).is_none(),
                "address: {address}"
            );
        }
    }

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
