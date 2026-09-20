use std::{
    fmt::{Debug, Formatter},
    ops::Range,
    path::{Path, PathBuf},
};

use super::execution::{ApplyStatus, Diagnostic, ExecutionEvent};

#[cfg(test)]
pub(crate) mod git;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanBlockKind {
    Common,
    Resource,
    Output,
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
        matches!(self.kind, PlanBlockKind::Common)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PlanDocument {
    text: String,
    blocks: Vec<PlanBlock>,
}

pub(crate) struct FilteredPlan<'a> {
    lines: Vec<&'a str>,
    resource_count: usize,
    matching_resources: usize,
    output_count: usize,
    matching_outputs: usize,
}

impl<'a> FilteredPlan<'a> {
    #[must_use]
    pub(crate) fn lines(&self) -> &[&'a str] {
        &self.lines
    }

    #[must_use]
    pub(crate) const fn resource_count(&self) -> usize {
        self.resource_count
    }

    #[must_use]
    pub(crate) const fn matching_resources(&self) -> usize {
        self.matching_resources
    }

    #[must_use]
    pub(crate) const fn output_count(&self) -> usize {
        self.output_count
    }

    #[must_use]
    pub(crate) const fn matching_outputs(&self) -> usize {
        self.matching_outputs
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
        let mut resource_count = 0;
        let mut matching_resources = 0;
        let mut output_count = 0;
        let mut matching_outputs = 0;
        for block in &self.blocks {
            match block.kind {
                PlanBlockKind::Resource => resource_count += 1,
                PlanBlockKind::Output => output_count += 1,
                PlanBlockKind::Common => {}
            }
            let matches = query.is_empty()
                || block.is_common()
                || block
                    .lines()
                    .clone()
                    .any(|line| lines[line].contains(query));
            if !matches {
                continue;
            }
            match block.kind {
                PlanBlockKind::Resource => matching_resources += 1,
                PlanBlockKind::Output => matching_outputs += 1,
                PlanBlockKind::Common => {}
            }
            filtered.extend(block.lines().clone().map(|line| lines[line]));
        }
        FilteredPlan {
            lines: filtered,
            resource_count,
            matching_resources,
            output_count,
            matching_outputs,
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
    fn document_keeps_common_lines_and_matching_blocks_in_original_order() {
        let document = PlanDocument::with_blocks(
            "preamble\nresource api\napi value\nresource worker\nworker value\nsummary\n"
                .to_owned(),
            vec![
                PlanBlock::new(0..1, PlanBlockKind::Common),
                PlanBlock::new(1..3, PlanBlockKind::Resource),
                PlanBlock::new(3..5, PlanBlockKind::Resource),
                PlanBlock::new(5..7, PlanBlockKind::Common),
            ],
        );

        let filtered = document.filter("worker");

        assert_eq!(
            filtered.lines(),
            ["preamble", "resource worker", "worker value", "summary", ""]
        );
        assert_eq!(filtered.resource_count(), 2);
        assert_eq!(filtered.matching_resources(), 1);
        assert_eq!(filtered.output_count(), 0);
        assert_eq!(filtered.matching_outputs(), 0);
    }

    #[test]
    fn filter_counts_each_resource_and_output_block_once() {
        let document = PlanDocument::with_blocks(
            "common api\nresource api api api\nresource worker\noutput endpoint\nunknown endpoint\n"
                .to_owned(),
            vec![
                PlanBlock::new(0..1, PlanBlockKind::Common),
                PlanBlock::new(1..2, PlanBlockKind::Resource),
                PlanBlock::new(2..3, PlanBlockKind::Resource),
                PlanBlock::new(3..4, PlanBlockKind::Output),
                PlanBlock::new(4..6, PlanBlockKind::Common),
            ],
        );

        let resource = document.filter("api");
        assert_eq!(
            resource.lines(),
            ["common api", "resource api api api", "unknown endpoint", ""]
        );
        assert_eq!(resource.resource_count(), 2);
        assert_eq!(resource.matching_resources(), 1);
        assert_eq!(resource.output_count(), 1);
        assert_eq!(resource.matching_outputs(), 0);

        let output = document.filter("endpoint");
        assert_eq!(
            output.lines(),
            ["common api", "output endpoint", "unknown endpoint", ""]
        );
        assert_eq!(output.resource_count(), 2);
        assert_eq!(output.matching_resources(), 0);
        assert_eq!(output.output_count(), 1);
        assert_eq!(output.matching_outputs(), 1);

        let mixed = document.filter("e");
        assert_eq!(mixed.matching_resources(), 2);
        assert_eq!(mixed.matching_outputs(), 1);
        assert_eq!(mixed.matching_resources() + mixed.matching_outputs(), 3);

        let empty = document.filter("");
        assert_eq!(empty.resource_count(), 2);
        assert_eq!(empty.matching_resources(), 2);
        assert_eq!(empty.output_count(), 1);
        assert_eq!(empty.matching_outputs(), 1);
    }

    #[test]
    fn filter_keeps_common_text_when_no_searchable_block_matches() {
        let document = PlanDocument::with_blocks(
            "diagnostic only\nresource api\noutput endpoint\nunknown boundary text\n".to_owned(),
            vec![
                PlanBlock::new(0..1, PlanBlockKind::Common),
                PlanBlock::new(1..2, PlanBlockKind::Resource),
                PlanBlock::new(2..3, PlanBlockKind::Output),
                PlanBlock::new(3..5, PlanBlockKind::Common),
            ],
        );

        let filtered = document.filter("missing");

        assert_eq!(
            filtered.lines(),
            ["diagnostic only", "unknown boundary text", ""]
        );
        assert_eq!(
            filtered.matching_resources() + filtered.matching_outputs(),
            0
        );
        assert_eq!(filtered.resource_count(), 1);
        assert_eq!(filtered.output_count(), 1);
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
