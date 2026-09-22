use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};

#[cfg(test)]
mod attribute_diff;
#[cfg(test)]
mod path;

#[cfg(test)]
pub(crate) use attribute_diff::AttributePathSegment;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum PlanValue {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl Debug for PlanValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<redacted>")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReplacePathSegment {
    Attribute(String),
    Index(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceMode {
    Managed,
    Data,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanAction {
    Create,
    Read,
    Update,
    Delete,
    NoOp,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanResource {
    pub(crate) address: String,
    pub(crate) actions: Vec<PlanAction>,
}

impl PlanResource {
    #[must_use]
    pub(crate) fn has_action(&self, action: &PlanAction) -> bool {
        self.actions.iter().any(|candidate| candidate == action)
    }

    #[must_use]
    pub(crate) fn is_replacement(&self) -> bool {
        self.actions.len() == 2
            && self.has_action(&PlanAction::Create)
            && self.has_action(&PlanAction::Delete)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceChangeKind {
    Create,
    Update,
    Replace,
    Delete,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ResourceChange {
    pub(crate) address: String,
    pub(crate) mode: ResourceMode,
    pub(crate) actions: Vec<PlanAction>,
    pub(crate) kind: ResourceChangeKind,
    pub(crate) before: Option<PlanValue>,
    pub(crate) after: Option<PlanValue>,
    pub(crate) before_sensitive: Option<PlanValue>,
    pub(crate) after_sensitive: Option<PlanValue>,
    pub(crate) after_unknown: Option<PlanValue>,
    pub(crate) replace_paths: Option<Vec<Vec<ReplacePathSegment>>>,
    pub(crate) action_reason: Option<String>,
}

impl Debug for ResourceChange {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResourceChange")
            .field("address", &self.address)
            .field("mode", &self.mode)
            .field("actions", &self.actions)
            .field("kind", &self.kind)
            .field("before", &self.before.as_ref().map(|_| "<redacted>"))
            .field("after", &self.after.as_ref().map(|_| "<redacted>"))
            .field(
                "before_sensitive",
                &self.before_sensitive.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "after_sensitive",
                &self.after_sensitive.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "after_unknown",
                &self.after_unknown.as_ref().map(|_| "<redacted>"),
            )
            .field("replace_paths", &self.replace_paths)
            .field("action_reason", &self.action_reason)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PlanSummary {
    pub(crate) creates: usize,
    pub(crate) updates: usize,
    pub(crate) replaces: usize,
    pub(crate) deletes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsupportedChangeScope {
    Resource,
    Output,
    ResourceDrift,
    DeferredResource,
    ActionInvocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsupportedChangeKind {
    Output,
    Drift,
    Read,
    Move,
    Import,
    UnknownAction,
    UnsupportedActions,
    Deferred,
    ActionInvocation,
    DeferredActionInvocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnsupportedChange {
    pub(crate) scope: UnsupportedChangeScope,
    pub(crate) address: String,
    pub(crate) actions: Vec<PlanAction>,
    pub(crate) kind: UnsupportedChangeKind,
    pub(crate) reason: Option<String>,
    pub(crate) action_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) changes: Vec<ResourceChange>,
    pub(crate) summary: PlanSummary,
    pub(crate) unsupported_changes: Vec<UnsupportedChange>,
}
