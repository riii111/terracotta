use std::{
    fmt::{Debug, Formatter},
    ops::Range,
    path::{Path, PathBuf},
};

use super::execution::{ApplyStatus, Diagnostic, ExecutionEvent};

#[cfg(test)]
pub(crate) mod git;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanBlockKind {
    Common,
    Resource(String),
    Output(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanBlock {
    lines: Range<usize>,
    kind: PlanBlockKind,
}

impl PlanBlock {
    #[must_use]
    pub(crate) const fn new(lines: Range<usize>, kind: PlanBlockKind) -> Self {
        Self { lines, kind }
    }

    #[must_use]
    pub(crate) const fn lines(&self) -> &Range<usize> {
        &self.lines
    }

    pub(crate) const fn lines_mut(&mut self) -> &mut Range<usize> {
        &mut self.lines
    }

    #[must_use]
    pub(crate) const fn is_common(&self) -> bool {
        matches!(&self.kind, PlanBlockKind::Common)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PlanDocument {
    text: String,
    blocks: Vec<PlanBlock>,
}

pub(crate) struct FilteredPlan<'a> {
    lines: Vec<&'a str>,
    matching_blocks: usize,
}

impl<'a> FilteredPlan<'a> {
    #[must_use]
    pub(crate) fn lines(&self) -> &[&'a str] {
        &self.lines
    }

    #[must_use]
    pub(crate) const fn matching_blocks(&self) -> usize {
        self.matching_blocks
    }
}

impl PlanDocument {
    #[must_use]
    pub(crate) const fn with_blocks(text: String, blocks: Vec<PlanBlock>) -> Self {
        Self { text, blocks }
    }

    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub(crate) fn filter(&self, query: &str) -> FilteredPlan<'_> {
        let lines = self.text.split('\n').collect::<Vec<_>>();
        let mut filtered = Vec::new();
        let mut matching_blocks = 0;
        for block in &self.blocks {
            let matches = query.is_empty()
                || block.is_common()
                || block
                    .lines()
                    .clone()
                    .any(|line| lines[line].contains(query));
            if !matches {
                continue;
            }
            if !block.is_common() {
                matching_blocks += 1;
            }
            filtered.extend(block.lines().clone().map(|line| lines[line]));
        }
        FilteredPlan {
            lines: filtered,
            matching_blocks,
        }
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
    pub(crate) fn resource_addresses(&self) -> &[String] {
        &self.resource_addresses
    }

    #[must_use]
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
    pub(crate) const fn has_changes(&self) -> bool {
        self.additions > 0
            || self.changes > 0
            || self.deletions > 0
            || !self.output_names.is_empty()
    }

    #[must_use]
    pub(crate) const fn applyable(&self) -> bool {
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
    search_query: String,
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
            search_query: String::new(),
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

    #[must_use]
    pub(crate) fn search_query(&self) -> &str {
        &self.search_query
    }

    pub(crate) fn set_search_query(&mut self, query: String) {
        self.search_query = query;
    }

    #[must_use]
    pub(crate) fn filtered_document(&self) -> FilteredPlan<'_> {
        self.document.filter(&self.search_query)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanReviewMessage {
    Event(ExecutionEvent),
    Completed(PlanReview),
    Failed {
        message: String,
        interrupted: bool,
    },
    ApplyEvent(ExecutionEvent),
    ApplyCompleted {
        status: ApplyStatus,
        summary_line: Option<String>,
    },
    ApplyFailed {
        message: String,
    },
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{PlanBlock, PlanBlockKind, PlanDocument};

    pub(crate) fn plan_document(text: String) -> PlanDocument {
        let end = text.split('\n').count();
        PlanDocument::with_blocks(text, vec![PlanBlock::new(0..end, PlanBlockKind::Common)])
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::plan_document;
    use super::*;

    #[test]
    fn debug_output_never_contains_plan_text() {
        let document = plan_document("password = secret".to_owned());

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
        assert!(metadata.deletions() > 0);
        assert!(metadata.applyable());
        assert!(
            metadata
                .output_names()
                .iter()
                .any(|output| output == "endpoint")
        );
    }

    #[test]
    fn document_keeps_common_lines_and_matching_blocks_in_original_order() {
        let document = PlanDocument::with_blocks(
            "preamble\nresource api\napi value\nresource worker\nworker value\nsummary\n"
                .to_owned(),
            vec![
                PlanBlock::new(0..1, PlanBlockKind::Common),
                PlanBlock::new(
                    1..3,
                    PlanBlockKind::Resource("terraform_data.api".to_owned()),
                ),
                PlanBlock::new(
                    3..5,
                    PlanBlockKind::Resource("terraform_data.worker".to_owned()),
                ),
                PlanBlock::new(5..7, PlanBlockKind::Common),
            ],
        );

        let filtered = document.filter("worker");

        assert_eq!(
            filtered.lines(),
            ["preamble", "resource worker", "worker value", "summary", ""]
        );
        assert_eq!(filtered.matching_blocks(), 1);
    }

    #[test]
    fn search_query_is_not_included_in_the_copy_document() {
        let mut review = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document("Terraform body\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
            Vec::new(),
        );

        review.set_search_query("body".to_owned());

        assert_eq!(review.search_query(), "body");
        assert_eq!(review.document().text(), "Terraform body\n");
    }
}
