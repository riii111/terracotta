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

pub(crate) fn module_breadcrumbs(address: &str) -> Option<Vec<String>> {
    let components = parse_address_components_with_wildcards(address, true)?;
    let mut components = components.into_iter().peekable();
    let mut breadcrumbs = Vec::new();

    while matches!(
        components.peek(),
        Some(AddressComponent::Name(name)) if name == "module"
    ) {
        components.next();
        let Some(AddressComponent::Name(mut module)) = components.next() else {
            return None;
        };
        while matches!(components.peek(), Some(AddressComponent::InstanceKey(_))) {
            let Some(AddressComponent::InstanceKey(key)) = components.next() else {
                unreachable!("the next address component was checked as an instance key");
            };
            module.push_str(&key);
        }
        breadcrumbs.push(module);
    }

    Some(breadcrumbs)
}

pub(crate) fn resource_display_address(address: &str) -> Option<String> {
    let mut segments = Vec::new();
    for component in parse_address_components_with_wildcards(address, true)? {
        match component {
            AddressComponent::Name(name) => segments.push(name),
            AddressComponent::InstanceKey(key) => segments.last_mut()?.push_str(&key),
        }
    }
    let mut resource_start = 0;
    while segments
        .get(resource_start)
        .is_some_and(|segment| segment == "module")
    {
        resource_start += 2;
    }
    if segments.len() < resource_start + 2 {
        return None;
    }
    if segments
        .get(resource_start)
        .is_some_and(|segment| segment == "data")
    {
        return Some(segments[resource_start..].join("."));
    }
    Some(segments.split_off(segments.len() - 2).join("."))
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
    parse_address_components_with_wildcards(address, false)
}

fn parse_address_components_with_wildcards(
    address: &str,
    allow_wildcards: bool,
) -> Option<Vec<AddressComponent>> {
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
            position = parse_instance_key(bytes, position, allow_wildcards)?;
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

fn parse_instance_key(bytes: &[u8], mut position: usize, allow_wildcards: bool) -> Option<usize> {
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
        Some(b'*') if allow_wildcards => {
            position += 1;
            (bytes.get(position) == Some(&b']')).then_some(position + 1)
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

    #[test]
    fn module_breadcrumbs_keep_module_instance_keys_and_omit_resource_keys() {
        let cases = [
            (
                r#"module.app["prod"].module.dns[0].aws_route53_record.db[3]"#,
                vec![r#"app["prod"]"#.to_owned(), "dns[0]".to_owned()],
            ),
            (
                "module.app[*].module.dns[*].aws_route53_record.db[*]",
                vec!["app[*]".to_owned(), "dns[*]".to_owned()],
            ),
            ("aws_route53_record.db[3]", Vec::new()),
        ];

        for (address, expected) in cases {
            assert_eq!(module_breadcrumbs(address), Some(expected));
        }
    }

    #[test]
    fn resource_display_address_keeps_only_resource_and_its_instance_keys() {
        let cases = [
            (
                r#"module.app["prod"].aws_route53_record.db[3]"#,
                "aws_route53_record.db[3]",
            ),
            (
                "module.app[*].aws_route53_record.db[*]",
                "aws_route53_record.db[*]",
            ),
            ("module.app[0].data.aws_ami.base[*]", "data.aws_ami.base[*]"),
            ("aws_route53_record.db[3]", "aws_route53_record.db[3]"),
        ];

        for (address, expected) in cases {
            assert_eq!(resource_display_address(address).as_deref(), Some(expected));
        }
    }
}
