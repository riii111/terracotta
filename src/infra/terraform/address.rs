use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum AddressIndex {
    Number(String),
    String(String),
}

impl Display for AddressIndex {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(value) => formatter.write_str(value),
            Self::String(value) => {
                let quoted = serde_json::to_string(value).map_err(|_| fmt::Error)?;
                formatter.write_str(&quoted)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ModuleAddressSegment {
    pub(super) name: String,
    pub(super) index: Option<AddressIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ResourceAddress {
    pub(super) modules: Vec<ModuleAddressSegment>,
    pub(super) mode: String,
    pub(super) resource_type: String,
    pub(super) name: String,
    pub(super) index: Option<AddressIndex>,
}

impl ResourceAddress {
    pub(super) fn block(&self) -> String {
        format_resource_address(
            &self.modules,
            &self.mode,
            &self.resource_type,
            &self.name,
            None,
        )
    }

    pub(super) fn full(&self) -> String {
        format_resource_address(
            &self.modules,
            &self.mode,
            &self.resource_type,
            &self.name,
            self.index.as_ref(),
        )
    }

    pub(super) fn module_names(&self) -> impl Iterator<Item = &str> {
        self.modules.iter().map(|module| module.name.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResourceReference {
    Resource(ResourceAddress),
    ModuleOutput {
        modules: Vec<ModuleAddressSegment>,
        output: String,
    },
    Module {
        modules: Vec<ModuleAddressSegment>,
    },
    Variable(String),
    Local(String),
    Meta,
}

pub(super) fn parse_resource_address(address: &str) -> Option<ResourceAddress> {
    let parts = split_traversal(address)?;
    let parsed = parse_resource_parts(&parts)?;
    let expected_parts = parsed.modules.len() * 2 + usize::from(parsed.mode == "data") + 2;
    (parts.len() == expected_parts).then_some(parsed)
}

pub(super) fn parse_reference(reference: &str) -> Option<ResourceReference> {
    let parts = split_traversal(reference)?;
    if parts.is_empty() {
        return None;
    }

    match parts[0].as_str() {
        "var" => {
            return parts
                .get(1)
                .map(|name| ResourceReference::Variable(name.clone()));
        }
        "local" => {
            return parts
                .get(1)
                .map(|name| ResourceReference::Local(name.clone()));
        }
        "count" | "each" | "self" | "path" | "terraform" => {
            return Some(ResourceReference::Meta);
        }
        _ => {}
    }

    let (modules, rest) = module_prefix(&parts)?;
    if rest.is_empty() {
        return (!modules.is_empty()).then_some(ResourceReference::Module { modules });
    }
    if !modules.is_empty() {
        let output = parse_indexed_segment(&rest[0])?.name;
        return Some(ResourceReference::ModuleOutput { modules, output });
    }

    parse_resource_parts(rest).map(ResourceReference::Resource)
}

pub(super) fn parse_module_address(module: &str) -> Option<Vec<ModuleAddressSegment>> {
    if module.is_empty() {
        return Some(Vec::new());
    }
    let parts = split_traversal(module)?;
    let (modules, rest) = module_prefix(&parts)?;
    rest.is_empty().then_some(modules)
}

pub(super) fn format_resource_address(
    modules: &[ModuleAddressSegment],
    mode: &str,
    resource_type: &str,
    name: &str,
    index: Option<&AddressIndex>,
) -> String {
    let mut address = String::new();
    for module in modules {
        if !address.is_empty() {
            address.push('.');
        }
        address.push_str("module.");
        address.push_str(&module.name);
        push_index(&mut address, module.index.as_ref());
    }
    if !address.is_empty() {
        address.push('.');
    }
    if mode == "data" {
        address.push_str("data.");
    }
    address.push_str(resource_type);
    address.push('.');
    address.push_str(name);
    push_index(&mut address, index);
    address
}

fn parse_resource_parts(parts: &[String]) -> Option<ResourceAddress> {
    let (modules, rest) = module_prefix(parts)?;
    let (mode, resource_offset) = if rest.first().is_some_and(|part| part == "data") {
        ("data", 1)
    } else {
        ("managed", 0)
    };
    let resource_type = parse_segment(rest.get(resource_offset)?)?;
    let resource_name = parse_indexed_segment(rest.get(resource_offset + 1)?)?;
    Some(ResourceAddress {
        modules,
        mode: mode.to_owned(),
        resource_type,
        name: resource_name.name,
        index: resource_name.index,
    })
}

fn module_prefix(parts: &[String]) -> Option<(Vec<ModuleAddressSegment>, &[String])> {
    let mut modules = Vec::new();
    let mut offset = 0;
    while parts.get(offset).is_some_and(|part| part == "module") {
        let module = parse_indexed_segment(parts.get(offset + 1)?)?;
        modules.push(module);
        offset += 2;
    }
    Some((modules, &parts[offset..]))
}

fn split_traversal(value: &str) -> Option<Vec<String>> {
    if value.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut bracket_depth = 0_u8;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        match character {
            '"' if bracket_depth > 0 && !escaped => quoted = !quoted,
            '\\' if quoted => escaped = !escaped,
            '[' if !quoted => {
                bracket_depth = bracket_depth.checked_add(1)?;
                escaped = false;
            }
            ']' if !quoted => {
                bracket_depth = bracket_depth.checked_sub(1)?;
                escaped = false;
            }
            '.' if !quoted && bracket_depth == 0 => {
                if index == start {
                    return None;
                }
                parts.push(value[start..index].to_owned());
                start = index + character.len_utf8();
                escaped = false;
            }
            _ => escaped = false,
        }
    }
    if bracket_depth != 0 || quoted || start == value.len() {
        return None;
    }
    parts.push(value[start..].to_owned());
    Some(parts)
}

fn parse_segment(segment: &str) -> Option<String> {
    let name = parse_indexed_segment(segment)?;
    name.index.is_none().then_some(name.name)
}

fn parse_indexed_segment(segment: &str) -> Option<ModuleAddressSegment> {
    let Some(open) = segment.find('[') else {
        return valid_name(segment).then(|| ModuleAddressSegment {
            name: segment.to_owned(),
            index: None,
        });
    };
    if !segment.ends_with(']') || !valid_name(&segment[..open]) {
        return None;
    }
    let raw_index = &segment[open + 1..segment.len() - 1];
    let index = if raw_index.starts_with('"') {
        serde_json::from_str::<String>(raw_index)
            .ok()
            .map(AddressIndex::String)?
    } else {
        if raw_index.is_empty() || !raw_index.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        AddressIndex::Number(raw_index.to_owned())
    };
    Some(ModuleAddressSegment {
        name: segment[..open].to_owned(),
        index: Some(index),
    })
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn push_index(address: &mut String, index: Option<&AddressIndex>) {
    if let Some(index) = index {
        address.push('[');
        address.push_str(&index.to_string());
        address.push(']');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_instance_addresses_without_losing_module_keys() {
        let address =
            parse_resource_address("module.app[\"blue.zone\"].module.dns.aws_record.db[1]")
                .expect("resource address should parse");

        assert_eq!(address.module_names().collect::<Vec<_>>(), ["app", "dns"]);
        assert_eq!(
            address.block(),
            "module.app[\"blue.zone\"].module.dns.aws_record.db"
        );
        assert_eq!(
            address.full(),
            "module.app[\"blue.zone\"].module.dns.aws_record.db[1]"
        );
    }

    #[test]
    fn distinguishes_resource_blocks_from_instance_references() {
        let block = parse_reference("aws_instance.api.id").expect("reference should parse");
        let instance = parse_reference("aws_instance.api[1].id").expect("reference should parse");

        assert!(
            matches!(block, ResourceReference::Resource(ref address) if address.index.is_none())
        );
        assert!(
            matches!(instance, ResourceReference::Resource(ref address) if address.full() == "aws_instance.api[1]")
        );
        assert!(parse_resource_address("aws_instance.api.id").is_none());
        assert!(matches!(
            parse_reference("module.child.output.secret"),
            Some(ResourceReference::ModuleOutput { ref output, .. }) if output == "output"
        ));
        assert!(matches!(
            parse_reference("module.child.output[0]"),
            Some(ResourceReference::ModuleOutput { ref output, .. }) if output == "output"
        ));
    }
}
