use std::{
    fmt::{Debug, Formatter},
    path::{Path, PathBuf},
};

use super::execution::{Diagnostic, ExecutionEvent};

#[cfg(test)]
pub(crate) mod git;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PlanDocument {
    text: String,
}

impl PlanDocument {
    #[must_use]
    pub(crate) const fn new(text: String) -> Self {
        Self { text }
    }

    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

impl Debug for PlanDocument {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlanDocument")
            .field("text", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanMetadata {
    resource_addresses: Vec<String>,
    output_names: Vec<String>,
    additions: usize,
    changes: usize,
    deletions: usize,
    applyable: bool,
}

impl PlanMetadata {
    #[must_use]
    pub(crate) const fn new(
        resource_addresses: Vec<String>,
        output_names: Vec<String>,
        additions: usize,
        changes: usize,
        deletions: usize,
        applyable: bool,
    ) -> Self {
        Self {
            resource_addresses,
            output_names,
            additions,
            changes,
            deletions,
            applyable,
        }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn resource_addresses(&self) -> &[String] {
        &self.resource_addresses
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn output_names(&self) -> &[String] {
        &self.output_names
    }

    #[must_use]
    pub(crate) const fn additions(&self) -> usize {
        self.additions
    }

    #[must_use]
    pub(crate) const fn changes(&self) -> usize {
        self.changes
    }

    #[must_use]
    pub(crate) const fn deletions(&self) -> usize {
        self.deletions
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn contains_deletions(&self) -> bool {
        self.deletions > 0
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn applyable(&self) -> bool {
        self.applyable
    }

    #[must_use]
    pub(crate) const fn has_changes(&self) -> bool {
        self.applyable
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanReview {
    root: PathBuf,
    workspace: String,
    document: PlanDocument,
    metadata: PlanMetadata,
    diagnostics: Vec<Diagnostic>,
}

impl PlanReview {
    #[must_use]
    pub(crate) const fn new(
        root: PathBuf,
        workspace: String,
        document: PlanDocument,
        metadata: PlanMetadata,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            root,
            workspace,
            document,
            metadata,
            diagnostics,
        }
    }

    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub(crate) fn workspace(&self) -> &str {
        &self.workspace
    }

    #[must_use]
    pub(crate) const fn document(&self) -> &PlanDocument {
        &self.document
    }

    #[must_use]
    pub(crate) const fn metadata(&self) -> &PlanMetadata {
        &self.metadata
    }

    #[must_use]
    pub(crate) fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanReviewMessage {
    Event(ExecutionEvent),
    Completed(PlanReview),
    Failed { message: String, interrupted: bool },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_plan_text() {
        let document = PlanDocument::new("password = secret".to_owned());

        let debug = format!("{document:?}");

        assert!(!debug.contains("secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn metadata_exposes_summary_and_applyability_without_values() {
        let metadata = PlanMetadata::new(
            vec!["terraform_data.example".to_owned()],
            vec!["endpoint".to_owned()],
            1,
            0,
            1,
            true,
        );

        assert_eq!(metadata.additions(), 1);
        assert_eq!(metadata.deletions(), 1);
        assert!(metadata.contains_deletions());
        assert!(metadata.applyable());
        assert_eq!(metadata.output_names(), ["endpoint"]);
    }
}
