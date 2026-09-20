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

impl PlanDocument {
    #[must_use]
    #[cfg(test)]
    pub(crate) fn new(text: String) -> Self {
        let end = line_count(&text);
        Self {
            text,
            blocks: vec![PlanBlock::new(0..end, PlanBlockKind::Common)],
        }
    }

    #[must_use]
    pub(crate) const fn with_blocks(text: String, blocks: Vec<PlanBlock>) -> Self {
        Self { text, blocks }
    }

    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn blocks(&self) -> &[PlanBlock] {
        &self.blocks
    }

    #[must_use]
    pub(crate) fn visible_lines(&self, query: &str) -> Vec<&str> {
        let lines = self.text.split('\n').collect::<Vec<_>>();
        self.blocks
            .iter()
            .filter(|block| {
                query.is_empty()
                    || block.is_common()
                    || block
                        .lines()
                        .clone()
                        .any(|line| lines[line].contains(query))
            })
            .flat_map(|block| block.lines().clone().map(|line| lines[line]))
            .collect()
    }

    #[must_use]
    pub(crate) fn matching_block_count(&self, query: &str) -> usize {
        if query.is_empty() {
            return self
                .blocks
                .iter()
                .filter(|block| !block.is_common())
                .count();
        }
        let lines = self.text.split('\n').collect::<Vec<_>>();
        self.blocks
            .iter()
            .filter(|block| {
                !block.is_common()
                    && block
                        .lines()
                        .clone()
                        .any(|line| lines[line].contains(query))
            })
            .count()
    }
}

#[cfg(test)]
fn line_count(text: &str) -> usize {
    text.split('\n').count()
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
    pub(crate) fn visible_document_lines(&self) -> Vec<&str> {
        self.document.visible_lines(&self.search_query)
    }

    #[must_use]
    pub(crate) fn matching_block_count(&self) -> usize {
        self.document.matching_block_count(&self.search_query)
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
    use super::PlanMetadata;

    pub(crate) const fn resource_count(metadata: &PlanMetadata) -> usize {
        metadata.resource_addresses.len()
    }

    pub(crate) fn has_output(metadata: &PlanMetadata, name: &str) -> bool {
        metadata.output_names.iter().any(|output| output == name)
    }

    pub(crate) const fn contains_deletions(metadata: &PlanMetadata) -> bool {
        metadata.deletions > 0
    }

    pub(crate) const fn applyable(metadata: &PlanMetadata) -> bool {
        metadata.applyable
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{applyable, contains_deletions, has_output};
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
        assert!(contains_deletions(&metadata));
        assert!(applyable(&metadata));
        assert!(has_output(&metadata, "endpoint"));
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

        assert_eq!(
            document.visible_lines("worker"),
            ["preamble", "resource worker", "worker value", "summary", ""]
        );
        assert_eq!(document.matching_block_count("worker"), 1);
    }

    #[test]
    fn search_query_is_not_included_in_the_copy_document() {
        let mut review = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            PlanDocument::new("Terraform body\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
            Vec::new(),
        );

        review.set_search_query("body".to_owned());

        assert_eq!(review.search_query(), "body");
        assert_eq!(review.document().text(), "Terraform body\n");
    }
}
