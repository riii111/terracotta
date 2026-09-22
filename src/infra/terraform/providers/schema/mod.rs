use std::{collections::BTreeMap, ffi::OsString, path::Path};

use serde_json::{Map, Value};

use crate::app::execution::Tool;
use crate::app::plan::{AttributeType, ProviderSchema, ProviderSchemas, ResourceSchema};
use crate::infra::CancellationToken;

use super::super::command::{
    ProcessRunner, ProcessStatus, TerraformCommand, TerraformExecutionError, interrupted_error,
    run_command,
};
use super::super::show::PlanParseError;

const SUPPORTED_FORMAT_MAJOR: u64 = 1;

pub(crate) fn read_provider_schema_with_arguments(
    tool: Tool,
    root: &Path,
    global_arguments: &[OsString],
    cancellation: &CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<Option<ProviderSchemas>, TerraformExecutionError> {
    let mut arguments = global_arguments.to_vec();
    arguments.extend([
        OsString::from("providers"),
        OsString::from("schema"),
        OsString::from("-json"),
    ]);
    let output = match run_command(
        tool,
        root,
        TerraformCommand::ProvidersSchema,
        &arguments,
        cancellation,
        runner,
    ) {
        Ok(output) => output,
        Err(_error) if !cancellation.is_cancelled() => return Ok(None),
        Err(error) => return Err(error),
    };
    if output.interrupted {
        return Err(interrupted_error(
            tool,
            TerraformCommand::ProvidersSchema,
            output,
        ));
    }
    if !output.status.is_some_and(ProcessStatus::is_success) {
        return Ok(None);
    }
    Ok(parse_provider_schemas(&output.output.stdout).ok())
}

fn parse_provider_schemas(input: &[u8]) -> Result<ProviderSchemas, PlanParseError> {
    let document =
        serde_json::from_slice::<Value>(input).map_err(|_| PlanParseError::InvalidJson)?;
    let root = document
        .as_object()
        .ok_or(PlanParseError::RootMustBeObject)?;
    parse_format_version(root)?;
    let providers = root
        .get("provider_schemas")
        .ok_or(PlanParseError::MissingField("provider_schemas"))?
        .as_object()
        .ok_or(PlanParseError::InvalidField("provider_schemas"))?;
    let providers = providers
        .iter()
        .map(|(name, value)| parse_provider_schema(value).map(|schema| (name.clone(), schema)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    Ok(ProviderSchemas { providers })
}

fn parse_provider_schema(value: &Value) -> Result<ProviderSchema, PlanParseError> {
    let provider = value
        .as_object()
        .ok_or(PlanParseError::InvalidField("provider schema"))?;
    let resources = provider
        .get("resource_schemas")
        .map(|value| {
            value
                .as_object()
                .ok_or(PlanParseError::InvalidField("resource_schemas"))?
                .iter()
                .map(|(name, value)| {
                    parse_resource_schema(value).map(|schema| (name.clone(), schema))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(ProviderSchema { resources })
}

fn parse_resource_schema(value: &Value) -> Result<ResourceSchema, PlanParseError> {
    let resource = value
        .as_object()
        .ok_or(PlanParseError::InvalidField("resource schema"))?;
    let block = resource
        .get("block")
        .ok_or(PlanParseError::MissingField("resource schema block"))?
        .as_object()
        .ok_or(PlanParseError::InvalidField("resource schema block"))?;
    let attributes = parse_attributes(block.get("attributes"))?;
    let block_types = parse_block_types(block.get("block_types"))?;
    Ok(ResourceSchema {
        attributes,
        block_types,
    })
}

fn parse_attributes(
    value: Option<&Value>,
) -> Result<BTreeMap<String, AttributeType>, PlanParseError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let attributes = value
        .as_object()
        .ok_or(PlanParseError::InvalidField("schema attributes"))?;
    attributes
        .iter()
        .map(|(name, value)| {
            let attribute = value
                .as_object()
                .ok_or(PlanParseError::InvalidField("schema attribute"))?;
            parse_attribute_type(attribute).map(|kind| (name.clone(), kind))
        })
        .collect()
}

fn parse_attribute_type(attribute: &Map<String, Value>) -> Result<AttributeType, PlanParseError> {
    if let Some(kind) = attribute.get("type") {
        return parse_type(kind);
    }
    if let Some(nested_type) = attribute.get("nested_type") {
        return parse_nested_type(nested_type);
    }
    Err(PlanParseError::MissingField("schema attribute type"))
}

fn parse_nested_type(value: &Value) -> Result<AttributeType, PlanParseError> {
    let nested_type = value
        .as_object()
        .ok_or(PlanParseError::InvalidField("schema nested type"))?;
    let attributes = parse_attributes(nested_type.get("attributes"))?;
    let object = AttributeType::Object(attributes);
    match nested_type
        .get("nesting_mode")
        .ok_or(PlanParseError::MissingField(
            "schema nested type nesting_mode",
        ))?
        .as_str()
        .ok_or(PlanParseError::InvalidField(
            "schema nested type nesting_mode",
        ))? {
        "single" | "group" => Ok(object),
        "list" => Ok(AttributeType::List(Box::new(object))),
        "set" => Ok(AttributeType::Set(Box::new(object))),
        "map" => Ok(AttributeType::Map(Box::new(object))),
        _ => Err(PlanParseError::InvalidField(
            "schema nested type nesting_mode",
        )),
    }
}

fn parse_block_types(
    value: Option<&Value>,
) -> Result<BTreeMap<String, AttributeType>, PlanParseError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let blocks = value
        .as_object()
        .ok_or(PlanParseError::InvalidField("schema block_types"))?;
    blocks
        .iter()
        .map(|(name, value)| {
            let block = value
                .as_object()
                .ok_or(PlanParseError::InvalidField("schema block type"))?;
            let nested = block
                .get("block")
                .ok_or(PlanParseError::MissingField("schema nested block"))?;
            let nested = nested
                .as_object()
                .ok_or(PlanParseError::InvalidField("schema nested block"))?;
            let object = parse_attributes(nested.get("attributes"))?
                .into_iter()
                .collect::<BTreeMap<_, _>>();
            Ok((name.clone(), AttributeType::Object(object)))
        })
        .collect()
}

fn parse_type(value: &Value) -> Result<AttributeType, PlanParseError> {
    match value {
        Value::String(kind) => match kind.as_str() {
            "bool" => Ok(AttributeType::Bool),
            "number" => Ok(AttributeType::Number),
            "string" => Ok(AttributeType::String),
            "dynamic" => Ok(AttributeType::Dynamic),
            _ => Err(PlanParseError::InvalidField("schema attribute type")),
        },
        Value::Array(parts) => {
            let Some(kind) = parts.first().and_then(Value::as_str) else {
                return Err(PlanParseError::InvalidField("schema attribute type"));
            };
            let nested = |index| {
                parts
                    .get(index)
                    .ok_or(PlanParseError::MissingField("schema nested type"))
                    .and_then(parse_type)
            };
            match kind {
                "list" => Ok(AttributeType::List(Box::new(nested(1)?))),
                "set" => Ok(AttributeType::Set(Box::new(nested(1)?))),
                "map" => Ok(AttributeType::Map(Box::new(nested(1)?))),
                "tuple" => parts
                    .get(1)
                    .and_then(Value::as_array)
                    .ok_or(PlanParseError::InvalidField("schema tuple type"))?
                    .iter()
                    .map(parse_type)
                    .collect::<Result<Vec<_>, _>>()
                    .map(AttributeType::Tuple),
                "object" => parts
                    .get(1)
                    .and_then(Value::as_object)
                    .ok_or(PlanParseError::InvalidField("schema object type"))?
                    .iter()
                    .map(|(name, value)| parse_type(value).map(|value| (name.clone(), value)))
                    .collect::<Result<BTreeMap<_, _>, _>>()
                    .map(AttributeType::Object),
                _ => Err(PlanParseError::InvalidField("schema attribute type")),
            }
        }
        _ => Err(PlanParseError::InvalidField("schema attribute type")),
    }
}

fn parse_format_version(root: &Map<String, Value>) -> Result<(), PlanParseError> {
    let version = root
        .get("format_version")
        .ok_or(PlanParseError::MissingField("format_version"))?
        .as_str()
        .ok_or(PlanParseError::InvalidField("format_version"))?;
    let major = version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u64>().ok())
        .ok_or(PlanParseError::InvalidField("format_version"))?;
    if major == SUPPORTED_FORMAT_MAJOR {
        Ok(())
    } else {
        Err(PlanParseError::UnsupportedFormatMajor(major))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_resource_types_without_treating_objects_as_maps() {
        let document = serde_json::json!({
            "format_version": "1.0",
            "provider_schemas": {
                "registry.terraform.io/hashicorp/example": {
                    "resource_schemas": {
                        "example_resource": {
                            "block": {
                                "attributes": {
                                    "labels": {"type": ["map", "string"]},
                                    "nested": {"type": ["object", {"name": "string"}]},
                                    "framework_nested": {
                                        "nested_type": {
                                            "nesting_mode": "list",
                                            "attributes": {"name": {"type": "string"}}
                                        }
                                    },
                                    "framework_group": {
                                        "nested_type": {
                                            "nesting_mode": "group",
                                            "attributes": {"name": {"type": "string"}}
                                        }
                                    }
                                },
                                "block_types": {
                                    "settings": {"nesting_mode": "list", "block": {"attributes": {}}}
                                }
                            }
                        }
                    }
                }
            }
        });

        let schemas = parse_provider_schemas(document.to_string().as_bytes())
            .expect("provider schema should parse");
        let provider = schemas
            .providers
            .get("registry.terraform.io/hashicorp/example")
            .expect("provider should be retained");
        let resource = provider
            .resources
            .get("example_resource")
            .expect("resource should be retained");
        assert!(resource.attributes["labels"].is_simple_map());
        assert!(!resource.attributes["nested"].is_simple_map());
        assert!(!resource.attributes["framework_nested"].is_simple_map());
        assert!(!resource.attributes["framework_group"].is_simple_map());
        assert!(matches!(
            resource.block_types["settings"],
            AttributeType::Object(_)
        ));
    }

    #[test]
    fn malformed_provider_schema_is_a_recoverable_read_failure() {
        assert!(parse_provider_schemas(br#"{"format_version":"1.0"}"#).is_err());
    }
}
