use std::collections::{BTreeMap, BTreeSet};

use super::{
    AttributeType, PlanValue, ProviderSchemas, ResourceChange, ResourceChangeKind, ResourceSchema,
    attribute_diff::{CanonicalNumber, canonical_number},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AttributeComparison {
    pub(crate) attrs_differ: bool,
    pub(crate) unknown_differ: bool,
    pub(crate) values_differ: bool,
}

pub(crate) fn compare_resource_attributes(
    left: &ResourceChange,
    left_schemas: Option<&ProviderSchemas>,
    right: &ResourceChange,
    right_schemas: Option<&ProviderSchemas>,
) -> AttributeComparison {
    let left = changed_attributes(left, left_schemas);
    let right = changed_attributes(right, right_schemas);
    let mut comparison = AttributeComparison {
        attrs_differ: !left.keys().eq(right.keys()),
        ..AttributeComparison::default()
    };
    for (path, (before, after)) in &left {
        if let Some((other_before, other_after)) = right.get(path) {
            comparison.include(compare_values(before, other_before));
            comparison.include(compare_values(after, other_after));
        }
    }
    comparison
}

pub(crate) fn resource_has_unknown(change: &ResourceChange) -> bool {
    marker_contains_unknown(change.after_unknown.as_ref())
}

impl AttributeComparison {
    const fn include(&mut self, comparison: ValueComparison) {
        self.unknown_differ |= comparison.uncertain;
        self.values_differ |= comparison.different;
    }
}

// Comparison values never cross into display models or diagnostic output.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Value {
    Absent,
    Null,
    Unknown,
    Bool(bool),
    Number(CanonicalNumber),
    String(String),
    Object(BTreeMap<String, Self>),
    Sequence(Vec<Self>),
    Set(Vec<Self>),
    UntypedSequence(Vec<Self>),
}

type Attributes = BTreeMap<Vec<String>, (Value, Value)>;

fn changed_attributes(change: &ResourceChange, schemas: Option<&ProviderSchemas>) -> Attributes {
    let schema = resource_schema(change, schemas).map(|schema| {
        AttributeType::Object(
            schema
                .attributes
                .iter()
                .chain(&schema.block_types)
                .map(|(name, kind)| (name.clone(), kind.clone()))
                .collect(),
        )
    });
    let before = if change.kind == ResourceChangeKind::Create {
        Value::Object(BTreeMap::new())
    } else {
        comparison_value(change.before.as_ref(), None, schema.as_ref())
    };
    let after = if change.kind == ResourceChangeKind::Delete {
        Value::Object(BTreeMap::new())
    } else {
        comparison_value(
            change.after.as_ref(),
            change.after_unknown.as_ref(),
            schema.as_ref(),
        )
    };
    let mut attributes = BTreeMap::new();
    collect_changed_attributes(&mut attributes, Vec::new(), before, after);
    attributes
}

fn collect_changed_attributes(
    attributes: &mut Attributes,
    path: Vec<String>,
    before: Value,
    after: Value,
) {
    if let (Value::Object(left), Value::Object(right)) = (&before, &after) {
        let keys: BTreeSet<_> = left.keys().chain(right.keys()).collect();
        if !keys.is_empty() {
            for key in keys {
                let mut child = path.clone();
                child.push(key.clone());
                collect_changed_attributes(
                    attributes,
                    child,
                    left.get(key).cloned().unwrap_or(Value::Absent),
                    right.get(key).cloned().unwrap_or(Value::Absent),
                );
            }
            return;
        }
    }
    let comparison = compare_values(&before, &after);
    if comparison.different || comparison.uncertain || after.has_unknown() {
        attributes.insert(path, (before, after));
    }
}

fn resource_schema<'a>(
    change: &ResourceChange,
    schemas: Option<&'a ProviderSchemas>,
) -> Option<&'a ResourceSchema> {
    schemas?
        .providers
        .get(change.provider.as_ref()?)?
        .resources
        .get(change.resource_type.as_ref()?)
}

fn comparison_value(
    value: Option<&PlanValue>,
    unknown: Option<&PlanValue>,
    kind: Option<&AttributeType>,
) -> Value {
    if matches!(unknown, Some(PlanValue::Bool(true))) {
        return Value::Unknown;
    }
    match value {
        Some(PlanValue::Bool(value)) => Value::Bool(*value),
        Some(PlanValue::Number(value)) => {
            canonical_number(value).map_or(Value::Unknown, Value::Number)
        }
        Some(PlanValue::String(value)) => Value::String(value.clone()),
        Some(PlanValue::Object(values)) => object_value(Some(values), unknown, kind),
        Some(PlanValue::Array(values)) => array_value(values, unknown, kind),
        None | Some(PlanValue::Null) if marker_contains_unknown(unknown) => match unknown {
            Some(PlanValue::Object(_)) => object_value(None, unknown, kind),
            Some(PlanValue::Array(_)) => array_value(&[], unknown, kind),
            _ => Value::Unknown,
        },
        Some(PlanValue::Null) => Value::Null,
        None => Value::Absent,
    }
}

fn object_value(
    values: Option<&BTreeMap<String, PlanValue>>,
    unknown: Option<&PlanValue>,
    kind: Option<&AttributeType>,
) -> Value {
    let markers = match unknown {
        Some(PlanValue::Object(markers)) => Some(markers),
        _ => None,
    };
    let keys: BTreeSet<_> = values
        .into_iter()
        .flat_map(BTreeMap::keys)
        .chain(
            markers
                .into_iter()
                .flat_map(BTreeMap::iter)
                .filter(|(_, marker)| marker_contains_unknown(Some(marker)))
                .map(|(key, _)| key),
        )
        .collect();
    Value::Object(
        keys.into_iter()
            .map(|key| {
                let child_kind = match kind {
                    Some(AttributeType::Object(types)) => types.get(key),
                    Some(AttributeType::Map(element)) => Some(element.as_ref()),
                    _ => None,
                };
                (
                    key.clone(),
                    comparison_value(
                        values.and_then(|values| values.get(key)),
                        markers.and_then(|markers| markers.get(key)),
                        child_kind,
                    ),
                )
            })
            .collect(),
    )
}

fn array_value(
    values: &[PlanValue],
    unknown: Option<&PlanValue>,
    kind: Option<&AttributeType>,
) -> Value {
    let markers = match unknown {
        Some(PlanValue::Array(markers)) => markers.as_slice(),
        _ => &[],
    };
    let marker_length = markers
        .iter()
        .rposition(|marker| marker_contains_unknown(Some(marker)))
        .map_or(0, |index| index + 1);
    let mut elements: Vec<_> = (0..values.len().max(marker_length))
        .map(|index| {
            let child_kind = match kind {
                Some(AttributeType::List(element) | AttributeType::Set(element)) => {
                    Some(element.as_ref())
                }
                Some(AttributeType::Tuple(types)) => types.get(index),
                _ => None,
            };
            comparison_value(values.get(index), markers.get(index), child_kind)
        })
        .collect();
    match kind {
        Some(AttributeType::Set(_)) => {
            elements.sort();
            Value::Set(elements)
        }
        Some(AttributeType::List(_) | AttributeType::Tuple(_)) => Value::Sequence(elements),
        _ => Value::UntypedSequence(elements),
    }
}

#[derive(Default, Clone, Copy)]
struct ValueComparison {
    different: bool,
    uncertain: bool,
}

impl ValueComparison {
    const fn include(&mut self, other: Self) {
        self.different |= other.different;
        self.uncertain |= other.uncertain;
    }
}

fn compare_values(left: &Value, right: &Value) -> ValueComparison {
    if left == right {
        return ValueComparison::default();
    }
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            let keys: BTreeSet<_> = left.keys().chain(right.keys()).collect();
            let mut result = ValueComparison::default();
            for key in keys {
                result.include(compare_values(
                    left.get(key).unwrap_or(&Value::Absent),
                    right.get(key).unwrap_or(&Value::Absent),
                ));
            }
            result
        }
        (Value::Set(left), Value::Set(right)) => compare_sets(left, right),
        (Value::Sequence(left), Value::Sequence(right)) => {
            let mut result = ValueComparison::default();
            for index in 0..left.len().max(right.len()) {
                result.include(compare_values(
                    left.get(index).unwrap_or(&Value::Absent),
                    right.get(index).unwrap_or(&Value::Absent),
                ));
            }
            result
        }
        (Value::Unknown, _)
        | (_, Value::Unknown)
        | (
            Value::UntypedSequence(_),
            Value::UntypedSequence(_) | Value::Sequence(_) | Value::Set(_),
        )
        | (Value::Sequence(_) | Value::Set(_), Value::UntypedSequence(_))
        | (Value::Set(_), Value::Sequence(_))
        | (Value::Sequence(_), Value::Set(_)) => ValueComparison {
            different: false,
            uncertain: true,
        },
        _ => ValueComparison {
            different: true,
            uncertain: left.has_unknown() || right.has_unknown(),
        },
    }
}

fn compare_sets(left: &[Value], right: &[Value]) -> ValueComparison {
    let mut compatible_right = vec![false; right.len()];
    let mut same_unknown_right = vec![false; right.len()];
    let mut different = false;
    let mut uncertain = false;
    for left_value in left {
        let mut compatible_left = false;
        let mut same_unknown_left = false;
        for (index, right_value) in right.iter().enumerate() {
            let comparison = compare_values(left_value, right_value);
            compatible_left |= !comparison.different;
            compatible_right[index] |= !comparison.different;
            same_unknown_left |= !comparison.uncertain;
            same_unknown_right[index] |= !comparison.uncertain;
        }
        different |= !compatible_left;
        uncertain |= !same_unknown_left && left_value.has_unknown();
    }
    different |= compatible_right.contains(&false);
    uncertain |= right
        .iter()
        .zip(same_unknown_right)
        .any(|(value, matched)| !matched && value.has_unknown());
    // Unknown elements may converge to one member, so cardinality and sorted
    // positions cannot prove a difference while every element has a candidate.
    ValueComparison {
        different,
        uncertain: uncertain || !different,
    }
}

impl Value {
    fn has_unknown(&self) -> bool {
        match self {
            Self::Unknown => true,
            Self::Object(values) => values.values().any(Self::has_unknown),
            Self::Sequence(values) | Self::Set(values) | Self::UntypedSequence(values) => {
                values.iter().any(Self::has_unknown)
            }
            _ => false,
        }
    }
}

fn marker_contains_unknown(marker: Option<&PlanValue>) -> bool {
    match marker {
        Some(PlanValue::Bool(true)) => true,
        Some(PlanValue::Array(values)) => values
            .iter()
            .any(|value| marker_contains_unknown(Some(value))),
        Some(PlanValue::Object(values)) => values
            .values()
            .any(|value| marker_contains_unknown(Some(value))),
        _ => false,
    }
}
