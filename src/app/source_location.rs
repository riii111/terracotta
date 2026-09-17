use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceSide {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceAddress {
    pub resource_type: String,
    pub name: String,
}

impl ResourceAddress {
    #[must_use]
    pub fn new(resource_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            name: name.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    pub start_line: usize,
    pub end_line: usize,
}

impl SourceRange {
    #[must_use]
    pub const fn new(start_line: usize, end_line: usize) -> Self {
        Self {
            start_line,
            end_line,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSourceLocation {
    pub address: ResourceAddress,
    pub path: PathBuf,
    pub side: SourceSide,
    pub range: SourceRange,
}

impl ResourceSourceLocation {
    #[must_use]
    pub fn resource_type(&self) -> &str {
        &self.address.resource_type
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.address.name
    }

    #[must_use]
    pub fn file(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn start_line(&self) -> usize {
        self.range.start_line
    }

    #[must_use]
    pub const fn end_line(&self) -> usize {
        self.range.end_line
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceIssueKind {
    SyntaxError,
    UnsupportedInput,
    DuplicateResource,
    ReadError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIssue {
    pub kind: SourceIssueKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFileAnalysis {
    pub path: PathBuf,
    pub side: SourceSide,
    pub resources: Vec<ResourceSourceLocation>,
    pub issues: Vec<SourceIssue>,
}

impl SourceFileAnalysis {
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }

    #[must_use]
    pub fn has_issue(&self, kind: SourceIssueKind) -> bool {
        self.issues.iter().any(|issue| issue.kind == kind)
    }
}
