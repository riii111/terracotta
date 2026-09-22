use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter, Write};

use super::{PlanValue, ReplacePathSegment, ResourceChange, ResourceChangeKind};

const ABSENT_DISPLAY: &str = "<absent>";
const SENSITIVE_DISPLAY: &str = "<sensitive>";
const UNKNOWN_DISPLAY: &str = "<unknown>";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AttributePathSegment {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttributeValueKind {
    Absent,
    Null,
    Unknown,
    Known,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AttributeValue {
    kind: AttributeValueKind,
    original: Option<PlanValue>,
    unknown_marker: Option<PlanValue>,
    sensitive: bool,
    display: String,
}

impl Debug for AttributeValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AttributeValue")
            .field("kind", &self.kind)
            .field("sensitive", &self.sensitive)
            .field("original", &self.original.as_ref().map(|_| "<redacted>"))
            .field(
                "unknown_marker",
                &self.unknown_marker.as_ref().map(|_| "<redacted>"),
            )
            .field("display", &"<redacted>")
            .finish()
    }
}

impl AttributeValue {
    #[must_use]
    pub(crate) const fn kind(&self) -> AttributeValueKind {
        self.kind
    }

    #[must_use]
    pub(crate) const fn is_sensitive(&self) -> bool {
        self.sensitive
    }

    #[must_use]
    pub(crate) const fn is_unknown(&self) -> bool {
        matches!(self.kind, AttributeValueKind::Unknown)
    }

    #[must_use]
    pub(crate) fn grouping_value(&self) -> Option<GroupingValue> {
        match (&self.kind, self.original.as_ref()) {
            (AttributeValueKind::Absent, None) => Some(GroupingValue::Absent),
            (AttributeValueKind::Known, Some(PlanValue::Bool(value))) => {
                Some(GroupingValue::Bool(*value))
            }
            (AttributeValueKind::Known, Some(PlanValue::Number(value))) => {
                canonical_number(value).map(GroupingValue::Number)
            }
            (AttributeValueKind::Known, Some(PlanValue::String(value))) => {
                Some(GroupingValue::String(value.clone()))
            }
            _ => None,
        }
    }

    #[must_use]
    pub(crate) fn display(&self) -> String {
        self.display.clone()
    }

    #[must_use]
    #[expect(
        dead_code,
        reason = "dormant attribute reveal behavior remains available for future plan review"
    )]
    pub(crate) const fn is_revealable(&self) -> bool {
        self.sensitive
            && self.unknown_marker.is_none()
            && matches!(
                self.kind,
                AttributeValueKind::Known | AttributeValueKind::Null
            )
    }

    #[must_use]
    pub(crate) const fn is_unmasked_unknown(&self) -> bool {
        matches!(self.kind, AttributeValueKind::Unknown) && !self.sensitive
    }

    #[must_use]
    #[expect(
        dead_code,
        reason = "dormant attribute reveal behavior remains available for future plan review"
    )]
    pub(crate) fn revealed_display(&self) -> Option<String> {
        if !self.is_revealable() {
            return None;
        }
        self.original.as_ref().map(display_plan_value)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GroupingValue {
    Absent,
    Bool(bool),
    Number(CanonicalNumber),
    String(String),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CanonicalNumber(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttributeChangeKind {
    Changed,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributeDiff {
    pub(crate) path: Vec<AttributePathSegment>,
    pub(crate) before: AttributeValue,
    pub(crate) after: AttributeValue,
    pub(crate) kind: AttributeChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributeDiffs {
    pub(crate) attributes: Vec<AttributeDiff>,
    pub(crate) changed_count: usize,
    pub(crate) unchanged_count: usize,
    pub(crate) replace_paths: Option<Vec<Vec<ReplacePathSegment>>>,
    pub(crate) action_reason: Option<String>,
}

pub(crate) fn diff_resource_attributes(change: &ResourceChange) -> AttributeDiffs {
    let mut attributes = Vec::new();
    collect_diffs(
        &mut attributes,
        Vec::new(),
        DiffInput {
            before: root_value(change, AttributeSide::Before),
            after: root_value(change, AttributeSide::After),
            before_sensitive: change.before_sensitive.as_ref(),
            after_sensitive: change.after_sensitive.as_ref(),
            after_unknown: change.after_unknown.as_ref(),
        },
        false,
        false,
    );

    let changed_count = attributes
        .iter()
        .filter(|attribute| attribute.kind == AttributeChangeKind::Changed)
        .count();
    let unchanged_count = attributes.len() - changed_count;

    AttributeDiffs {
        attributes,
        changed_count,
        unchanged_count,
        replace_paths: change.replace_paths.clone(),
        action_reason: change.action_reason.clone(),
    }
}

#[derive(Clone, Copy)]
enum AttributeSide {
    Before,
    After,
}

const fn root_value(change: &ResourceChange, side: AttributeSide) -> Option<&PlanValue> {
    let value = match side {
        AttributeSide::Before => change.before.as_ref(),
        AttributeSide::After => change.after.as_ref(),
    };

    match (change.kind, side, value) {
        (ResourceChangeKind::Create, AttributeSide::Before, Some(PlanValue::Null))
        | (ResourceChangeKind::Delete, AttributeSide::After, Some(PlanValue::Null)) => None,
        _ => value,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
    Object,
    Array,
}

#[derive(Clone, Copy)]
struct DiffInput<'a> {
    before: Option<&'a PlanValue>,
    after: Option<&'a PlanValue>,
    before_sensitive: Option<&'a PlanValue>,
    after_sensitive: Option<&'a PlanValue>,
    after_unknown: Option<&'a PlanValue>,
}

impl DiffInput<'_> {
    fn child(self, segment: &AttributePathSegment) -> Self {
        Self {
            before: child_value(self.before, segment),
            after: child_value(self.after, segment),
            before_sensitive: child_value(self.before_sensitive, segment),
            after_sensitive: child_value(self.after_sensitive, segment),
            after_unknown: child_value(self.after_unknown, segment),
        }
    }
}

fn collect_diffs(
    attributes: &mut Vec<AttributeDiff>,
    path: Vec<AttributePathSegment>,
    input: DiffInput<'_>,
    inherited_before_sensitive: bool,
    inherited_after_sensitive: bool,
) {
    let before_is_sensitive = inherited_before_sensitive || marker_is_true(input.before_sensitive);
    let after_is_sensitive = inherited_after_sensitive || marker_is_true(input.after_sensitive);

    if marker_is_true(input.before_sensitive) || marker_is_true(input.after_sensitive) {
        push_diff(
            attributes,
            path,
            input,
            before_is_sensitive,
            after_is_sensitive,
        );
        return;
    }

    if marker_is_true(input.after_unknown) {
        push_diff(
            attributes,
            path,
            input,
            before_is_sensitive,
            after_is_sensitive,
        );
        return;
    }

    if omitted_complex_unknown(input) {
        push_diff(
            attributes,
            path,
            input,
            before_is_sensitive,
            after_is_sensitive,
        );
        return;
    }

    if preserves_atomic_transition(input) {
        push_diff(
            attributes,
            path,
            input,
            before_is_sensitive,
            after_is_sensitive,
        );
        return;
    }

    let Some(container_kind) = container_kind(input) else {
        push_diff(
            attributes,
            path,
            input,
            before_is_sensitive,
            after_is_sensitive,
        );
        return;
    };

    if child_segments(container_kind, input).is_empty() {
        push_diff(
            attributes,
            path,
            input,
            before_is_sensitive,
            after_is_sensitive,
        );
        return;
    }

    collect_children(
        attributes,
        &path,
        input,
        container_kind,
        before_is_sensitive,
        after_is_sensitive,
    );
}

fn collect_children(
    attributes: &mut Vec<AttributeDiff>,
    path: &[AttributePathSegment],
    input: DiffInput<'_>,
    container_kind: ContainerKind,
    before_is_sensitive: bool,
    after_is_sensitive: bool,
) {
    for segment in child_segments(container_kind, input) {
        let mut child_path = path.to_vec();
        child_path.push(segment.clone());
        collect_diffs(
            attributes,
            child_path,
            input.child(&segment),
            before_is_sensitive,
            after_is_sensitive,
        );
    }
}

fn preserves_atomic_transition(input: DiffInput<'_>) -> bool {
    match (input.before, input.after) {
        (Some(before), Some(after)) => {
            let before_kind = value_container_kind(before);
            let after_kind = value_container_kind(after);
            before_kind != after_kind && (before_kind.is_some() || after_kind.is_some())
        }
        (Some(before), None) => {
            match (
                value_container_kind(before),
                metadata_container_kind(input.after_unknown),
            ) {
                (Some(before_kind), Some(after_kind)) => before_kind != after_kind,
                (None, Some(_)) => true,
                _ => false,
            }
        }
        (None, Some(after)) => {
            value_container_kind(after).is_none()
                && metadata_container_kind(input.after_unknown).is_some()
        }
        (None, None) => false,
    }
}

fn omitted_complex_unknown(input: DiffInput<'_>) -> bool {
    input.after.is_none()
        && input
            .before
            .is_none_or(|before| value_container_kind(before).is_none())
        && metadata_container_kind(input.after_unknown).is_some()
        && marker_contains_true(input.after_unknown)
}

fn push_diff(
    attributes: &mut Vec<AttributeDiff>,
    path: Vec<AttributePathSegment>,
    input: DiffInput<'_>,
    inherited_before_sensitive: bool,
    inherited_after_sensitive: bool,
) {
    let before_value = attribute_value(
        input.before,
        input.before_sensitive,
        None,
        inherited_before_sensitive,
    );
    let after_value = attribute_value(
        input.after,
        input.after_sensitive,
        input.after_unknown,
        inherited_after_sensitive,
    );
    let kind = if same_attribute_value(&before_value, &after_value) {
        AttributeChangeKind::Unchanged
    } else {
        AttributeChangeKind::Changed
    };

    attributes.push(AttributeDiff {
        path,
        before: before_value,
        after: after_value,
        kind,
    });
}

fn attribute_value(
    value: Option<&PlanValue>,
    sensitive_marker: Option<&PlanValue>,
    unknown_marker: Option<&PlanValue>,
    inherited_sensitive: bool,
) -> AttributeValue {
    let is_unknown = marker_is_true(unknown_marker)
        || (value.is_none()
            && metadata_container_kind(unknown_marker).is_some()
            && marker_contains_true(unknown_marker));
    let is_sensitive = inherited_sensitive || marker_contains_true(sensitive_marker);
    let kind = if is_unknown {
        AttributeValueKind::Unknown
    } else {
        match value {
            None => AttributeValueKind::Absent,
            Some(PlanValue::Null) => AttributeValueKind::Null,
            Some(_) => AttributeValueKind::Known,
        }
    };
    let display = display_attribute_value(
        kind,
        value,
        sensitive_marker,
        unknown_marker,
        inherited_sensitive,
    );

    AttributeValue {
        kind,
        original: value.cloned(),
        unknown_marker: unknown_marker
            .and_then(|marker| marker_contains_true(Some(marker)).then(|| marker.clone())),
        sensitive: is_sensitive,
        display,
    }
}

fn display_attribute_value(
    kind: AttributeValueKind,
    value: Option<&PlanValue>,
    sensitive_marker: Option<&PlanValue>,
    unknown_marker: Option<&PlanValue>,
    inherited_sensitive: bool,
) -> String {
    let sensitive_here = inherited_sensitive || marker_is_true(sensitive_marker);
    let sensitive = sensitive_here || marker_contains_true(sensitive_marker);

    if sensitive_here && !matches!(kind, AttributeValueKind::Absent) {
        return SENSITIVE_DISPLAY.to_owned();
    }

    match kind {
        AttributeValueKind::Absent => ABSENT_DISPLAY.to_owned(),
        AttributeValueKind::Null => {
            if sensitive {
                SENSITIVE_DISPLAY.to_owned()
            } else {
                "null".to_owned()
            }
        }
        AttributeValueKind::Unknown => {
            if sensitive {
                SENSITIVE_DISPLAY.to_owned()
            } else {
                UNKNOWN_DISPLAY.to_owned()
            }
        }
        AttributeValueKind::Known => value.map_or_else(
            || ABSENT_DISPLAY.to_owned(),
            |value| display_plan_value_with_markers(value, sensitive_marker, unknown_marker),
        ),
    }
}

fn same_attribute_value(before: &AttributeValue, after: &AttributeValue) -> bool {
    before.kind == after.kind
        && same_plan_value(before.original.as_ref(), after.original.as_ref())
        && before.unknown_marker == after.unknown_marker
}

fn same_plan_value(before: Option<&PlanValue>, after: Option<&PlanValue>) -> bool {
    match (before, after) {
        (Some(PlanValue::Number(before)), Some(PlanValue::Number(after))) => {
            match (canonical_number(before), canonical_number(after)) {
                (Some(before), Some(after)) => before == after,
                _ => before == after,
            }
        }
        _ => before == after,
    }
}

pub(super) fn canonical_number(value: &str) -> Option<CanonicalNumber> {
    let mut parser = NumberParser::new(value);
    let negative = parser.take_sign();
    let integer = parser.take_digits()?;
    let fraction = parser.take_fraction()?;
    let exponent = parser.take_exponent()?;
    if !parser.is_complete() {
        return None;
    }

    let mut digits = integer;
    digits.push_str(&fraction);
    let first_non_zero = digits
        .bytes()
        .position(|digit| digit != b'0')
        .unwrap_or(digits.len());
    if first_non_zero == digits.len() {
        return Some(CanonicalNumber("0".to_owned()));
    }
    digits.drain(..first_non_zero);

    let trailing_zero_count = digits
        .bytes()
        .rev()
        .take_while(|digit| *digit == b'0')
        .count();
    digits.truncate(digits.len() - trailing_zero_count);

    let mut scale = exponent;
    scale.adjust(false, fraction.len());
    scale.adjust(true, trailing_zero_count);

    let sign = if negative { "-" } else { "" };
    Some(CanonicalNumber(format!(
        "{sign}{digits}e{}",
        scale.as_string()
    )))
}

struct NumberParser<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> NumberParser<'a> {
    const fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
        }
    }

    fn take_sign(&mut self) -> bool {
        if self.input.get(self.position) == Some(&b'-') {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn take_digits(&mut self) -> Option<String> {
        let start = self.position;
        while self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
        (self.position > start)
            .then(|| String::from_utf8_lossy(&self.input[start..self.position]).into())
    }

    fn take_fraction(&mut self) -> Option<String> {
        if self.input.get(self.position) != Some(&b'.') {
            return Some(String::new());
        }
        self.position += 1;
        self.take_digits()
    }

    fn take_exponent(&mut self) -> Option<DecimalExponent> {
        if !matches!(self.input.get(self.position), Some(b'e' | b'E')) {
            return Some(DecimalExponent::zero());
        }
        self.position += 1;
        let negative = match self.input.get(self.position) {
            Some(b'-') => {
                self.position += 1;
                true
            }
            Some(b'+') => {
                self.position += 1;
                false
            }
            _ => false,
        };
        let digits = self.take_digits()?;
        Some(DecimalExponent::new(negative, &digits))
    }

    const fn is_complete(&self) -> bool {
        self.position == self.input.len()
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DecimalExponent {
    negative: bool,
    digits: String,
}

impl DecimalExponent {
    fn new(negative: bool, digits: &str) -> Self {
        let first_non_zero = digits
            .bytes()
            .position(|digit| digit != b'0')
            .unwrap_or(digits.len());
        if first_non_zero == digits.len() {
            return Self::zero();
        }
        Self {
            negative,
            digits: digits[first_non_zero..].to_owned(),
        }
    }

    const fn zero() -> Self {
        Self {
            negative: false,
            digits: String::new(),
        }
    }

    fn adjust(&mut self, positive: bool, amount: usize) {
        if amount == 0 {
            return;
        }
        let amount = amount.to_string();
        if self.digits.is_empty() {
            self.negative = !positive;
            self.digits = amount;
            return;
        }
        if self.negative != positive {
            self.digits = add_decimal_digits(&self.digits, &amount);
            return;
        }
        match self
            .digits
            .len()
            .cmp(&amount.len())
            .then_with(|| self.digits.cmp(&amount))
        {
            std::cmp::Ordering::Greater => {
                self.digits = subtract_decimal_digits(&self.digits, &amount);
            }
            std::cmp::Ordering::Equal => {
                self.digits.clear();
                self.negative = false;
            }
            std::cmp::Ordering::Less => {
                self.digits = subtract_decimal_digits(&amount, &self.digits);
                self.negative = !self.negative;
            }
        }
    }

    fn as_string(&self) -> String {
        if self.digits.is_empty() {
            "0".to_owned()
        } else if self.negative {
            format!("-{}", self.digits)
        } else {
            self.digits.clone()
        }
    }
}

fn add_decimal_digits(left: &str, right: &str) -> String {
    let mut result = Vec::with_capacity(left.len().max(right.len()) + 1);
    let mut carry = 0u8;
    let mut left = left.bytes().rev();
    let mut right = right.bytes().rev();
    loop {
        let left_digit = left.next();
        let right_digit = right.next();
        if left_digit.is_none() && right_digit.is_none() {
            if carry != 0 {
                result.push(b'1');
            }
            result.reverse();
            return String::from_utf8(result).expect("decimal digits are valid UTF-8");
        }
        let sum = left_digit.map_or(0, |digit| digit - b'0')
            + right_digit.map_or(0, |digit| digit - b'0')
            + carry;
        result.push(b'0' + sum % 10);
        carry = sum / 10;
    }
}

fn subtract_decimal_digits(left: &str, right: &str) -> String {
    let mut result = Vec::with_capacity(left.len());
    let mut borrow = 0i16;
    let left = left.bytes().rev();
    let mut right = right.bytes().rev();
    for left_digit in left {
        let mut difference = i16::from(left_digit - b'0') - borrow;
        let right_digit = right.next().map_or(0, |digit| digit - b'0');
        difference -= i16::from(right_digit);
        if difference < 0 {
            difference += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        result.push(
            b'0' + u8::try_from(difference).expect("decimal subtraction stays within one digit"),
        );
    }
    while result.last() == Some(&b'0') {
        result.pop();
    }
    result.reverse();
    String::from_utf8(result).expect("decimal digits are valid UTF-8")
}

fn container_kind(input: DiffInput<'_>) -> Option<ContainerKind> {
    match (input.before, input.after) {
        (Some(before), Some(after)) => {
            matching_container_kinds(value_container_kind(before), value_container_kind(after))
        }
        (Some(before), None) => value_container_kind(before),
        (None, Some(after)) => value_container_kind(after),
        (None, None) => matching_container_kinds(
            metadata_container_kind(input.before_sensitive),
            matching_container_kinds(
                metadata_container_kind(input.after_sensitive),
                metadata_container_kind(input.after_unknown),
            ),
        ),
    }
}

fn matching_container_kinds(
    first: Option<ContainerKind>,
    second: Option<ContainerKind>,
) -> Option<ContainerKind> {
    match (first, second) {
        (Some(first), Some(second)) if first != second => None,
        (Some(kind), _) | (_, Some(kind)) => Some(kind),
        (None, None) => None,
    }
}

fn child_segments(kind: ContainerKind, input: DiffInput<'_>) -> Vec<AttributePathSegment> {
    match kind {
        ContainerKind::Object => {
            let mut keys = BTreeSet::new();
            add_object_keys(&mut keys, input.before);
            add_object_keys(&mut keys, input.after);
            add_object_keys(&mut keys, input.before_sensitive);
            add_object_keys(&mut keys, input.after_sensitive);
            add_object_keys(&mut keys, input.after_unknown);
            keys.into_iter().map(AttributePathSegment::Key).collect()
        }
        ContainerKind::Array => {
            let length = [
                input.before,
                input.after,
                input.before_sensitive,
                input.after_sensitive,
                input.after_unknown,
            ]
            .into_iter()
            .filter_map(value_array_length)
            .max()
            .unwrap_or(0);
            (0..length).map(AttributePathSegment::Index).collect()
        }
    }
}

fn add_object_keys(keys: &mut BTreeSet<String>, value: Option<&PlanValue>) {
    if let Some(PlanValue::Object(values)) = value {
        keys.extend(values.keys().cloned());
    }
}

fn child_value<'a>(
    value: Option<&'a PlanValue>,
    segment: &AttributePathSegment,
) -> Option<&'a PlanValue> {
    match (value, segment) {
        (Some(PlanValue::Object(values)), AttributePathSegment::Key(key)) => values.get(key),
        (Some(PlanValue::Array(values)), AttributePathSegment::Index(index)) => values.get(*index),
        _ => None,
    }
}

const fn marker_is_true(value: Option<&PlanValue>) -> bool {
    matches!(value, Some(PlanValue::Bool(true)))
}

fn marker_contains_true(value: Option<&PlanValue>) -> bool {
    match value {
        Some(PlanValue::Bool(value)) => *value,
        Some(PlanValue::Array(values)) => values.iter().any(marker_contains_true_value),
        Some(PlanValue::Object(values)) => values.values().any(marker_contains_true_value),
        _ => false,
    }
}

fn marker_contains_true_value(value: &PlanValue) -> bool {
    marker_contains_true(Some(value))
}

const fn value_container_kind(value: &PlanValue) -> Option<ContainerKind> {
    match value {
        PlanValue::Object(_) => Some(ContainerKind::Object),
        PlanValue::Array(_) => Some(ContainerKind::Array),
        PlanValue::Null | PlanValue::Bool(_) | PlanValue::Number(_) | PlanValue::String(_) => None,
    }
}

fn metadata_container_kind(value: Option<&PlanValue>) -> Option<ContainerKind> {
    value.and_then(value_container_kind)
}

const fn value_array_length(value: Option<&PlanValue>) -> Option<usize> {
    match value {
        Some(PlanValue::Array(values)) => Some(values.len()),
        _ => None,
    }
}

fn display_plan_value(value: &PlanValue) -> String {
    match value {
        PlanValue::Null => "null".to_owned(),
        PlanValue::Bool(value) => value.to_string(),
        PlanValue::Number(value) => value.clone(),
        PlanValue::String(value) => display_string(value),
        PlanValue::Array(values) => {
            let values = values.iter().map(display_plan_value).collect::<Vec<_>>();
            format!("[{}]", values.join(", "))
        }
        PlanValue::Object(values) => {
            let values = values
                .iter()
                .map(|(key, value)| {
                    format!("{} = {}", display_string(key), display_plan_value(value))
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", values.join(", "))
        }
    }
}

fn display_plan_value_with_markers(
    value: &PlanValue,
    sensitive_marker: Option<&PlanValue>,
    unknown_marker: Option<&PlanValue>,
) -> String {
    if marker_is_true(sensitive_marker) {
        return SENSITIVE_DISPLAY.to_owned();
    }
    if marker_is_true(unknown_marker) {
        return UNKNOWN_DISPLAY.to_owned();
    }

    match value {
        PlanValue::Object(_) => {
            if marker_contains_true(sensitive_marker)
                && !matches!(sensitive_marker, Some(PlanValue::Object(_)))
            {
                return SENSITIVE_DISPLAY.to_owned();
            }
            if marker_contains_true(unknown_marker)
                && !matches!(unknown_marker, Some(PlanValue::Object(_)))
            {
                return UNKNOWN_DISPLAY.to_owned();
            }
            let mut keys = BTreeSet::new();
            add_object_keys(&mut keys, Some(value));
            add_object_keys(&mut keys, sensitive_marker);
            add_object_keys(&mut keys, unknown_marker);
            let values = keys
                .into_iter()
                .map(|key| {
                    let segment = AttributePathSegment::Key(key.clone());
                    format!(
                        "{} = {}",
                        display_string(&key),
                        display_optional_plan_value_with_markers(
                            child_value(Some(value), &segment),
                            child_value(sensitive_marker, &segment),
                            child_value(unknown_marker, &segment),
                        )
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", values.join(", "))
        }
        PlanValue::Array(_) => {
            if marker_contains_true(sensitive_marker)
                && !matches!(sensitive_marker, Some(PlanValue::Array(_)))
            {
                return SENSITIVE_DISPLAY.to_owned();
            }
            if marker_contains_true(unknown_marker)
                && !matches!(unknown_marker, Some(PlanValue::Array(_)))
            {
                return UNKNOWN_DISPLAY.to_owned();
            }
            let length = [Some(value), sensitive_marker, unknown_marker]
                .into_iter()
                .filter_map(value_array_length)
                .max()
                .unwrap_or(0);
            let values = (0..length)
                .map(|index| {
                    let segment = AttributePathSegment::Index(index);
                    display_optional_plan_value_with_markers(
                        child_value(Some(value), &segment),
                        child_value(sensitive_marker, &segment),
                        child_value(unknown_marker, &segment),
                    )
                })
                .collect::<Vec<_>>();
            format!("[{}]", values.join(", "))
        }
        PlanValue::Null | PlanValue::Bool(_) | PlanValue::Number(_) | PlanValue::String(_) => {
            if marker_contains_true(sensitive_marker) {
                SENSITIVE_DISPLAY.to_owned()
            } else if marker_contains_true(unknown_marker) {
                UNKNOWN_DISPLAY.to_owned()
            } else {
                display_plan_value(value)
            }
        }
    }
}

fn display_optional_plan_value_with_markers(
    value: Option<&PlanValue>,
    sensitive_marker: Option<&PlanValue>,
    unknown_marker: Option<&PlanValue>,
) -> String {
    value.map_or_else(
        || display_missing_plan_value_with_markers(sensitive_marker, unknown_marker),
        |value| display_plan_value_with_markers(value, sensitive_marker, unknown_marker),
    )
}

fn display_missing_plan_value_with_markers(
    sensitive_marker: Option<&PlanValue>,
    unknown_marker: Option<&PlanValue>,
) -> String {
    if marker_is_true(sensitive_marker) {
        return SENSITIVE_DISPLAY.to_owned();
    }
    if marker_is_true(unknown_marker) {
        return UNKNOWN_DISPLAY.to_owned();
    }

    match (
        metadata_container_kind(sensitive_marker),
        metadata_container_kind(unknown_marker),
    ) {
        (Some(ContainerKind::Object) | None, Some(ContainerKind::Object))
        | (Some(ContainerKind::Object), None) => {
            let mut keys = BTreeSet::new();
            add_object_keys(&mut keys, sensitive_marker);
            add_object_keys(&mut keys, unknown_marker);
            let values = keys
                .into_iter()
                .map(|key| {
                    let segment = AttributePathSegment::Key(key.clone());
                    format!(
                        "{} = {}",
                        display_string(&key),
                        display_missing_plan_value_with_markers(
                            child_value(sensitive_marker, &segment),
                            child_value(unknown_marker, &segment),
                        )
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", values.join(", "))
        }
        (Some(ContainerKind::Array) | None, Some(ContainerKind::Array))
        | (Some(ContainerKind::Array), None) => {
            let length = [sensitive_marker, unknown_marker]
                .into_iter()
                .filter_map(value_array_length)
                .max()
                .unwrap_or(0);
            let values = (0..length)
                .map(|index| {
                    let segment = AttributePathSegment::Index(index);
                    display_missing_plan_value_with_markers(
                        child_value(sensitive_marker, &segment),
                        child_value(unknown_marker, &segment),
                    )
                })
                .collect::<Vec<_>>();
            format!("[{}]", values.join(", "))
        }
        (Some(_), Some(_)) | (None, None) => {
            if marker_contains_true(sensitive_marker) {
                SENSITIVE_DISPLAY.to_owned()
            } else if marker_contains_true(unknown_marker) {
                UNKNOWN_DISPLAY.to_owned()
            } else {
                ABSENT_DISPLAY.to_owned()
            }
        }
    }
}

fn display_string(value: &str) -> String {
    let mut displayed = String::with_capacity(value.len() + 2);
    displayed.push('"');
    for character in value.chars() {
        match character {
            '"' => displayed.push_str("\\\""),
            '\\' => displayed.push_str("\\\\"),
            '\u{08}' => displayed.push_str("\\b"),
            '\u{0c}' => displayed.push_str("\\f"),
            '\n' => displayed.push_str("\\n"),
            '\r' => displayed.push_str("\\r"),
            '\t' => displayed.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(displayed, "\\u{:04x}", character as u32);
            }
            character => displayed.push(character),
        }
    }
    displayed.push('"');
    displayed
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::app::plan::{PlanAction, ResourceMode};

    impl AttributeValue {
        fn revealed(&self) -> Option<&PlanValue> {
            self.original.as_ref()
        }
    }

    fn plan_value(value: Value) -> PlanValue {
        match value {
            Value::Null => PlanValue::Null,
            Value::Bool(value) => PlanValue::Bool(value),
            Value::Number(value) => PlanValue::Number(value.to_string()),
            Value::String(value) => PlanValue::String(value),
            Value::Array(values) => PlanValue::Array(values.into_iter().map(plan_value).collect()),
            Value::Object(values) => PlanValue::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, plan_value(value)))
                    .collect(),
            ),
        }
    }

    struct ChangeFixture {
        before: Value,
        after: Value,
        before_sensitive: Value,
        after_sensitive: Value,
        after_unknown: Value,
    }

    fn change(fixture: ChangeFixture) -> ResourceChange {
        ResourceChange {
            address: "aws_instance.example".to_owned(),
            provider: None,
            resource_type: None,
            resource_name: None,
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: Some(plan_value(fixture.before)),
            after: Some(plan_value(fixture.after)),
            before_sensitive: Some(plan_value(fixture.before_sensitive)),
            after_sensitive: Some(plan_value(fixture.after_sensitive)),
            after_unknown: Some(plan_value(fixture.after_unknown)),
            replace_paths: Some(vec![vec![ReplacePathSegment::Attribute("name".to_owned())]]),
            action_reason: Some("replace_because_cannot_update".to_owned()),
            previous_address: None,
            importing: None,
        }
    }

    fn path(segments: &[AttributePathSegment]) -> Vec<AttributePathSegment> {
        segments.to_vec()
    }

    fn attribute<'a>(
        diffs: &'a AttributeDiffs,
        path: &[AttributePathSegment],
    ) -> &'a AttributeDiff {
        diffs
            .attributes
            .iter()
            .find(|attribute| attribute.path == path)
            .expect("attribute path should exist")
    }

    #[test]
    fn compares_nested_objects_and_arrays_by_key_and_index() {
        let change = change(ChangeFixture {
            before: json!({
                "name": "old",
                "tags": {"keep": "same", "remove": "gone"},
                "ports": [80, 443],
                "removed_ports": [8080, 8443]
            }),
            after: json!({
                "name": "new",
                "tags": {"add": "new", "keep": "same"},
                "ports": [80, 8443, 9443],
                "removed_ports": [8080]
            }),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!(false),
        });

        let diffs = diff_resource_attributes(&change);

        assert_eq!(
            diffs
                .attributes
                .iter()
                .map(|attribute| (&attribute.path, attribute.kind))
                .collect::<Vec<_>>(),
            vec![
                (
                    &path(&[AttributePathSegment::Key("name".to_owned())]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("ports".to_owned()),
                        AttributePathSegment::Index(0)
                    ]),
                    AttributeChangeKind::Unchanged
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("ports".to_owned()),
                        AttributePathSegment::Index(1)
                    ]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("ports".to_owned()),
                        AttributePathSegment::Index(2)
                    ]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("removed_ports".to_owned()),
                        AttributePathSegment::Index(0)
                    ]),
                    AttributeChangeKind::Unchanged
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("removed_ports".to_owned()),
                        AttributePathSegment::Index(1)
                    ]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("tags".to_owned()),
                        AttributePathSegment::Key("add".to_owned())
                    ]),
                    AttributeChangeKind::Changed
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("tags".to_owned()),
                        AttributePathSegment::Key("keep".to_owned())
                    ]),
                    AttributeChangeKind::Unchanged
                ),
                (
                    &path(&[
                        AttributePathSegment::Key("tags".to_owned()),
                        AttributePathSegment::Key("remove".to_owned())
                    ]),
                    AttributeChangeKind::Changed
                ),
            ]
        );
        assert_eq!(diffs.changed_count, 6);
        assert_eq!(diffs.unchanged_count, 3);
    }

    #[test]
    fn distinguishes_null_from_absent() {
        let change = change(ChangeFixture {
            before: json!({"null_value": null}),
            after: json!({"null_value": null, "new_value": null}),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!(false),
        });

        let diffs = diff_resource_attributes(&change);
        let null_value = attribute(
            &diffs,
            &[AttributePathSegment::Key("null_value".to_owned())],
        );
        let new_value = attribute(&diffs, &[AttributePathSegment::Key("new_value".to_owned())]);

        assert_eq!(null_value.kind, AttributeChangeKind::Unchanged);
        assert_eq!(null_value.before.kind(), AttributeValueKind::Null);
        assert_eq!(null_value.after.kind(), AttributeValueKind::Null);
        assert_eq!(new_value.before.kind(), AttributeValueKind::Absent);
        assert_eq!(new_value.after.kind(), AttributeValueKind::Null);
        assert_eq!(new_value.before.display(), "<absent>");
        assert_eq!(new_value.after.display(), "null");
        assert!(!new_value.before.is_unmasked_unknown());
        assert!(!new_value.after.is_unmasked_unknown());
    }

    #[test]
    fn applies_sensitive_markers_to_each_side_and_inherits_parent_masks() {
        let change = change(ChangeFixture {
            before: json!({"credentials": {"user": "alice", "token": "old"}}),
            after: json!({"credentials": {"user": "bob", "token": "new"}}),
            before_sensitive: json!({"credentials": true}),
            after_sensitive: json!({"credentials": {"token": true}}),
            after_unknown: json!(false),
        });

        let diffs = diff_resource_attributes(&change);

        assert_eq!(diffs.attributes.len(), 1);
        let credentials = attribute(
            &diffs,
            &[AttributePathSegment::Key("credentials".to_owned())],
        );

        assert_eq!(credentials.before.kind(), AttributeValueKind::Known);
        assert_eq!(credentials.after.kind(), AttributeValueKind::Known);
        assert!(credentials.before.is_sensitive() && credentials.after.is_sensitive());
        assert!(!credentials.before.is_unmasked_unknown());
        assert!(!credentials.after.is_unmasked_unknown());
        assert_eq!(credentials.before.display(), "<sensitive>");
        assert_eq!(
            credentials.after.display(),
            "{\"token\" = <sensitive>, \"user\" = \"bob\"}"
        );
    }

    #[test]
    fn keeps_marker_only_children_in_masked_composite_displays() {
        let change = change(ChangeFixture {
            before: json!({
                "credentials": {"user": "alice", "token": "old"},
                "items": ["old"]
            }),
            after: json!({"credentials": {"user": "bob"}, "items": ["new"]}),
            before_sensitive: json!({"credentials": true, "items": true}),
            after_sensitive: json!(false),
            after_unknown: json!({
                "credentials": {"token": true},
                "items": [false, true]
            }),
        });

        let diffs = diff_resource_attributes(&change);
        let credentials = attribute(
            &diffs,
            &[AttributePathSegment::Key("credentials".to_owned())],
        );
        let items = attribute(&diffs, &[AttributePathSegment::Key("items".to_owned())]);

        assert_eq!(
            credentials.after.display(),
            "{\"token\" = <unknown>, \"user\" = \"bob\"}"
        );
        assert_eq!(items.after.display(), "[\"new\", <unknown>]");
    }

    #[test]
    fn counts_unknown_nested_values_in_parent_sensitive_changes() {
        let change = change(ChangeFixture {
            before: json!({"secrets": [null]}),
            after: json!({"secrets": [null]}),
            before_sensitive: json!({"secrets": true}),
            after_sensitive: json!({"secrets": true}),
            after_unknown: json!({"secrets": [true]}),
        });

        let diffs = diff_resource_attributes(&change);
        let secrets = attribute(&diffs, &[AttributePathSegment::Key("secrets".to_owned())]);

        assert_eq!(secrets.kind, AttributeChangeKind::Changed);
        assert_eq!(diffs.changed_count, 1);
        assert_eq!(diffs.unchanged_count, 0);
    }

    #[test]
    fn keeps_one_sided_sensitive_values_masked_only_on_that_side() {
        let change = change(ChangeFixture {
            before: json!({"public": "old"}),
            after: json!({"public": "new"}),
            before_sensitive: json!(false),
            after_sensitive: json!({"public": true}),
            after_unknown: json!(false),
        });

        let attribute = &diff_resource_attributes(&change).attributes[0];

        assert_eq!(attribute.before.display(), "\"old\"");
        assert_eq!(attribute.after.display(), "<sensitive>");
        assert_eq!(attribute.after.revealed(), Some(&plan_value(json!("new"))));
    }

    #[test]
    fn represents_unknown_values_and_unknown_missing_attributes() {
        let change = change(ChangeFixture {
            before: json!({"known": "old", "null_value": null}),
            after: json!({"known": "new", "null_value": null}),
            before_sensitive: json!(false),
            after_sensitive: json!({"known": true}),
            after_unknown: json!({"future": true}),
        });

        let diffs = diff_resource_attributes(&change);
        let known = attribute(&diffs, &[AttributePathSegment::Key("known".to_owned())]);
        let future = attribute(&diffs, &[AttributePathSegment::Key("future".to_owned())]);

        assert_eq!(known.after.kind(), AttributeValueKind::Known);
        assert!(known.after.is_sensitive());
        assert!(!known.after.is_unmasked_unknown());
        assert_eq!(known.after.display(), "<sensitive>");
        assert_eq!(future.before.kind(), AttributeValueKind::Absent);
        assert_eq!(future.after.kind(), AttributeValueKind::Unknown);
        assert!(future.after.is_unknown());
        assert!(future.after.is_unmasked_unknown());
        assert_eq!(future.after.display(), "<unknown>");
    }

    #[test]
    fn masks_unknown_values_without_losing_internal_value_or_state() {
        let change = change(ChangeFixture {
            before: json!({"token": "old"}),
            after: json!({"token": "planned"}),
            before_sensitive: json!(false),
            after_sensitive: json!({"token": true}),
            after_unknown: json!({"token": true}),
        });

        let attribute = &diff_resource_attributes(&change).attributes[0];

        assert_eq!(attribute.after.kind(), AttributeValueKind::Unknown);
        assert!(attribute.after.is_sensitive());
        assert!(!attribute.after.is_unmasked_unknown());
        assert_eq!(attribute.after.display(), "<sensitive>");
        assert_eq!(
            attribute.after.revealed(),
            Some(&plan_value(json!("planned")))
        );
    }

    #[test]
    fn retains_replacement_metadata_and_attribute_counts() {
        let change = change(ChangeFixture {
            before: json!({"name": "old", "region": "same"}),
            after: json!({"name": "new", "region": "same"}),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!(false),
        });

        let diffs = diff_resource_attributes(&change);

        assert_eq!(diffs.changed_count, 1);
        assert_eq!(diffs.unchanged_count, 1);
        assert_eq!(diffs.replace_paths, change.replace_paths);
        assert_eq!(diffs.action_reason, change.action_reason);
    }

    #[test]
    fn treats_resource_root_null_as_absent_only_for_create_and_delete() {
        let mut create = change(ChangeFixture {
            before: json!(null),
            after: json!({"id": "created", "name": "example"}),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!(false),
        });
        create.kind = ResourceChangeKind::Create;

        let create_diffs = diff_resource_attributes(&create);
        let created_id = attribute(&create_diffs, &[AttributePathSegment::Key("id".to_owned())]);
        assert_eq!(created_id.before.kind(), AttributeValueKind::Absent);
        assert_eq!(created_id.after.kind(), AttributeValueKind::Known);
        assert_eq!(create_diffs.attributes.len(), 2);

        let mut delete = change(ChangeFixture {
            before: json!({"id": "deleted", "name": "example"}),
            after: json!(null),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!(false),
        });
        delete.kind = ResourceChangeKind::Delete;

        let delete_diffs = diff_resource_attributes(&delete);
        let deleted_id = attribute(&delete_diffs, &[AttributePathSegment::Key("id".to_owned())]);
        assert_eq!(deleted_id.before.kind(), AttributeValueKind::Known);
        assert_eq!(deleted_id.after.kind(), AttributeValueKind::Absent);
        assert_eq!(delete_diffs.attributes.len(), 2);
    }

    #[test]
    fn keeps_scalar_and_null_values_at_the_parent_when_shape_changes() {
        let change = change(ChangeFixture {
            before: json!({"settings": null, "name": "old"}),
            after: json!({"settings": {"enabled": true}, "name": "new"}),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!(false),
        });

        let diffs = diff_resource_attributes(&change);
        let settings = attribute(&diffs, &[AttributePathSegment::Key("settings".to_owned())]);

        assert_eq!(settings.before.kind(), AttributeValueKind::Null);
        assert_eq!(settings.after.kind(), AttributeValueKind::Known);
        assert_eq!(settings.before.display(), "null");
        assert_eq!(settings.after.display(), "{\"enabled\" = true}");
        assert!(!diffs.attributes.iter().any(|attribute| {
            attribute.path
                == [
                    AttributePathSegment::Key("settings".to_owned()),
                    AttributePathSegment::Key("enabled".to_owned()),
                ]
        }));
    }

    #[test]
    fn represents_omitted_complex_unknown_as_a_parent_value() {
        let mut change = change(ChangeFixture {
            before: json!({"config": null}),
            after: json!(null),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!({"config": {"token": true}}),
        });
        change.after = None;

        let diffs = diff_resource_attributes(&change);
        let config = attribute(&diffs, &[AttributePathSegment::Key("config".to_owned())]);

        assert_eq!(config.before.kind(), AttributeValueKind::Null);
        assert_eq!(config.after.kind(), AttributeValueKind::Unknown);
        assert_eq!(config.after.display(), "<unknown>");
        assert_eq!(diffs.changed_count, 1);
        assert_eq!(diffs.attributes.len(), 1);
        assert_eq!(
            config.path,
            [AttributePathSegment::Key("config".to_owned())]
        );
    }

    #[test]
    fn keeps_omitted_unknown_shape_changes_at_the_parent() {
        let mut change = change(ChangeFixture {
            before: json!({"config": {"old": true}}),
            after: json!(null),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!({"config": [true]}),
        });
        change.after = None;

        let diffs = diff_resource_attributes(&change);
        let config = attribute(&diffs, &[AttributePathSegment::Key("config".to_owned())]);

        assert_eq!(config.before.kind(), AttributeValueKind::Known);
        assert_eq!(config.after.kind(), AttributeValueKind::Unknown);
        assert_eq!(config.after.display(), "<unknown>");
        assert_eq!(diffs.attributes.len(), 1);
    }

    #[test]
    fn quotes_and_escapes_known_strings_and_object_keys() {
        let change = change(ChangeFixture {
            before: json!({"settings": null}),
            after: json!({"settings": {"line\nkey": "<unknown>\n\"", "null": "null"}}),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!(false),
        });

        let diffs = diff_resource_attributes(&change);
        let settings = attribute(&diffs, &[AttributePathSegment::Key("settings".to_owned())]);

        assert_eq!(
            settings.after.display(),
            "{\"line\\nkey\" = \"<unknown>\\n\\\"\", \"null\" = \"null\"}"
        );
    }

    #[test]
    fn debug_output_does_not_include_original_attribute_values() {
        let change = change(ChangeFixture {
            before: json!({"token": "synthetic-secret"}),
            after: json!({"token": "synthetic-secret-after"}),
            before_sensitive: json!(false),
            after_sensitive: json!({"token": true}),
            after_unknown: json!(false),
        });

        let debug = format!("{:?}", diff_resource_attributes(&change));

        assert!(!debug.contains("synthetic-secret"));
        assert!(!debug.contains("synthetic-secret-after"));
        assert!(debug.contains("sensitive: true"));
    }

    #[test]
    fn handles_empty_containers_as_single_attributes() {
        let change = change(ChangeFixture {
            before: json!({"object": {}, "array": []}),
            after: json!({"object": {}, "array": []}),
            before_sensitive: json!(false),
            after_sensitive: json!(false),
            after_unknown: json!(false),
        });

        let diffs = diff_resource_attributes(&change);

        assert_eq!(diffs.attributes.len(), 2);
        assert!(
            diffs
                .attributes
                .iter()
                .all(|attribute| attribute.kind == AttributeChangeKind::Unchanged)
        );
    }
}
