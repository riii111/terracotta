use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SourceSide {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ResourceAddress {
    resource_type: String,
    name: String,
}

impl ResourceAddress {
    #[must_use]
    pub(crate) fn new(resource_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            name: name.into(),
        }
    }

    #[must_use]
    pub(crate) fn resource_type(&self) -> &str {
        &self.resource_type
    }

    #[must_use]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceRange {
    start_line: usize,
    end_line: usize,
}

impl SourceRange {
    #[must_use]
    pub(crate) const fn new(start_line: usize, end_line: usize) -> Self {
        Self {
            start_line,
            end_line,
        }
    }

    #[must_use]
    pub(crate) const fn start_line(self) -> usize {
        self.start_line
    }

    #[must_use]
    pub(crate) const fn end_line(self) -> usize {
        self.end_line
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceSourceLocation {
    address: ResourceAddress,
    path: PathBuf,
    side: SourceSide,
    range: SourceRange,
}

impl ResourceSourceLocation {
    #[must_use]
    pub(crate) const fn new(
        address: ResourceAddress,
        path: PathBuf,
        side: SourceSide,
        range: SourceRange,
    ) -> Self {
        Self {
            address,
            path,
            side,
            range,
        }
    }

    #[must_use]
    pub(crate) const fn address(&self) -> &ResourceAddress {
        &self.address
    }

    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn set_address(&mut self, address: ResourceAddress) {
        self.address = address;
    }

    #[must_use]
    pub(crate) const fn side(&self) -> SourceSide {
        self.side
    }

    #[must_use]
    pub(crate) const fn range(&self) -> SourceRange {
        self.range
    }

    #[must_use]
    pub(crate) const fn start_line(&self) -> usize {
        self.range.start_line()
    }

    #[must_use]
    pub(crate) const fn end_line(&self) -> usize {
        self.range.end_line()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceIssueKind {
    SyntaxError,
    UnsupportedInput,
    DuplicateResource,
    ReadError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceIssue {
    kind: SourceIssueKind,
    message: String,
}

impl SourceIssue {
    #[must_use]
    pub(crate) fn new(kind: SourceIssueKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    #[must_use]
    pub(crate) const fn kind(&self) -> SourceIssueKind {
        self.kind
    }

    #[must_use]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceFileAnalysis {
    path: PathBuf,
    side: SourceSide,
    resources: Vec<ResourceSourceLocation>,
    issues: Vec<SourceIssue>,
}

impl SourceFileAnalysis {
    #[must_use]
    pub(crate) const fn new(
        path: PathBuf,
        side: SourceSide,
        resources: Vec<ResourceSourceLocation>,
        issues: Vec<SourceIssue>,
    ) -> Self {
        Self {
            path,
            side,
            resources,
            issues,
        }
    }

    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub(crate) const fn side(&self) -> SourceSide {
        self.side
    }

    #[must_use]
    pub(crate) fn resources(&self) -> &[ResourceSourceLocation] {
        &self.resources
    }

    #[must_use]
    pub(crate) fn issues(&self) -> &[SourceIssue] {
        &self.issues
    }

    #[must_use]
    pub(crate) const fn issues_mut(&mut self) -> &mut Vec<SourceIssue> {
        &mut self.issues
    }

    #[must_use]
    pub(crate) const fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }

    #[must_use]
    pub(crate) fn has_issue(&self, kind: SourceIssueKind) -> bool {
        self.issues.iter().any(|issue| issue.kind() == kind)
    }
}
