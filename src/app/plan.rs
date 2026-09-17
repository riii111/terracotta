use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};

#[derive(Clone, PartialEq, Eq)]
pub enum PlanValue {
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
pub enum ReplacePathSegment {
    Attribute(String),
    Index(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceMode {
    Managed,
    Data,
}

impl ResourceMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Data => "data",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAction {
    Create,
    Read,
    Update,
    Delete,
    NoOp,
    Unknown(String),
}

impl PlanAction {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Create => "create",
            Self::Read => "read",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::NoOp => "no-op",
            Self::Unknown(action) => action,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceChangeKind {
    Create,
    Update,
    Replace,
    Delete,
}

impl ResourceChangeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Replace => "replace",
            Self::Delete => "delete",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ResourceChange {
    pub address: String,
    pub mode: ResourceMode,
    pub actions: Vec<PlanAction>,
    pub kind: ResourceChangeKind,
    pub before: Option<PlanValue>,
    pub after: Option<PlanValue>,
    pub before_sensitive: Option<PlanValue>,
    pub after_sensitive: Option<PlanValue>,
    pub after_unknown: Option<PlanValue>,
    pub replace_paths: Option<Vec<Vec<ReplacePathSegment>>>,
    pub action_reason: Option<String>,
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
pub struct PlanSummary {
    pub creates: usize,
    pub updates: usize,
    pub replaces: usize,
    pub deletes: usize,
}

impl PlanSummary {
    #[must_use]
    pub const fn total(self) -> usize {
        self.creates + self.updates + self.replaces + self.deletes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedChangeScope {
    Resource,
    Output,
    ResourceDrift,
}

impl UnsupportedChangeScope {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resource => "resource",
            Self::Output => "output",
            Self::ResourceDrift => "resource-drift",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedChangeKind {
    Output,
    Drift,
    Read,
    Move,
    Import,
    UnknownAction,
    UnsupportedActions,
}

impl UnsupportedChangeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Drift => "drift",
            Self::Read => "read",
            Self::Move => "move",
            Self::Import => "import",
            Self::UnknownAction => "unknown-action",
            Self::UnsupportedActions => "unsupported-actions",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedChange {
    pub scope: UnsupportedChangeScope,
    pub address: String,
    pub actions: Vec<PlanAction>,
    pub kind: UnsupportedChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub changes: Vec<ResourceChange>,
    pub summary: PlanSummary,
    pub unsupported_changes: Vec<UnsupportedChange>,
}

impl Plan {
    #[must_use]
    pub const fn has_changes(&self) -> bool {
        !self.changes.is_empty() || !self.unsupported_changes.is_empty()
    }

    #[must_use]
    pub const fn unsupported_change_count(&self) -> usize {
        self.unsupported_changes.len()
    }
}
