use std::{
    collections::BTreeMap,
    fmt::{Debug, Formatter},
    ops::Range,
    path::{Path, PathBuf},
    time::Duration,
};

use super::{
    execution::{
        ApplyStatus, Diagnostic, ExecutionContext, ExecutionContextValue, ExecutionEvent,
        ExecutionTargetSpec, SensitiveValue,
    },
    plan::{Plan, PlanRelations, PlanResource, ProviderSchemas, ResourceChangeKind},
};

#[cfg(test)]
pub(crate) mod git;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanBlockKind {
    Common,
    Resource,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanLineKind {
    Body,
    Intro,
    Note,
    Summary,
    OutputSection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanBlock {
    lines: Range<usize>,
    kind: PlanBlockKind,
    addresses: Vec<String>,
}

impl PlanBlock {
    #[must_use]
    pub(crate) const fn new(lines: Range<usize>, kind: PlanBlockKind) -> Self {
        Self {
            lines,
            kind,
            addresses: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) const fn with_addresses(
        lines: Range<usize>,
        kind: PlanBlockKind,
        addresses: Vec<String>,
    ) -> Self {
        Self {
            lines,
            kind,
            addresses,
        }
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

    #[must_use]
    pub(crate) fn addresses(&self) -> &[String] {
        &self.addresses
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PlanDocument {
    text: String,
    blocks: Vec<PlanBlock>,
    line_kinds: Vec<PlanLineKind>,
    address_blocks: BTreeMap<String, usize>,
}

struct FilteredLine<'a> {
    line_index: usize,
    text: &'a str,
}

pub(crate) struct FilteredPlan<'a> {
    lines: Vec<FilteredLine<'a>>,
    matching_resources: usize,
    matching_outputs: usize,
}

impl<'a> FilteredPlan<'a> {
    pub(crate) fn lines_with_indices(&self) -> impl Iterator<Item = (usize, &'a str)> + '_ {
        self.lines.iter().map(|line| (line.line_index, line.text))
    }

    #[must_use]
    pub(crate) const fn matching_resources(&self) -> usize {
        self.matching_resources
    }

    #[must_use]
    pub(crate) const fn matching_outputs(&self) -> usize {
        self.matching_outputs
    }
}

impl PlanDocument {
    #[must_use]
    pub(crate) fn with_blocks_and_line_kinds(
        text: String,
        blocks: Vec<PlanBlock>,
        line_kinds: Vec<PlanLineKind>,
    ) -> Self {
        let mut address_blocks = BTreeMap::new();
        for (index, block) in blocks.iter().enumerate() {
            for address in block.addresses() {
                address_blocks.entry(address.clone()).or_insert(index);
            }
        }
        Self {
            text,
            blocks,
            line_kinds,
            address_blocks,
        }
    }

    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub(crate) fn filter(&self, query: &str) -> FilteredPlan<'_> {
        let lines = self.text.split('\n').collect::<Vec<_>>();
        let mut filtered = Vec::new();
        let mut matching_resources = 0;
        let mut matching_outputs = 0;
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
            match block.kind {
                PlanBlockKind::Resource => matching_resources += 1,
                PlanBlockKind::Output => matching_outputs += 1,
                PlanBlockKind::Common => {}
            }
            for line in block.lines().clone() {
                filtered.push(FilteredLine {
                    line_index: line,
                    text: lines[line],
                });
            }
        }
        FilteredPlan {
            lines: filtered,
            matching_resources,
            matching_outputs,
        }
    }

    #[must_use]
    pub(crate) fn line_kind(&self, line: usize) -> PlanLineKind {
        self.line_kinds
            .get(line)
            .copied()
            .unwrap_or(PlanLineKind::Body)
    }

    #[must_use]
    pub(crate) fn block_for_address(&self, address: &str) -> Option<&PlanBlock> {
        self.address_blocks
            .get(address)
            .and_then(|index| self.blocks.get(*index))
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

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PlanMetadata {
    resource_addresses: Vec<String>,
    resource_changes: Vec<PlanResource>,
    output_names: Vec<String>,
    additions: usize,
    changes: usize,
    replacements: usize,
    deletions: usize,
    nonstandard_changes: usize,
    applyable: bool,
    apply_targets: Vec<ExecutionTargetSpec>,
    sensitive_values: Vec<SensitiveValue>,
}

impl Debug for PlanMetadata {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlanMetadata")
            .field("resource_addresses", &self.resource_addresses)
            .field("resource_changes", &self.resource_changes)
            .field("output_names", &self.output_names)
            .field("additions", &self.additions)
            .field("changes", &self.changes)
            .field("replacements", &self.replacements)
            .field("deletions", &self.deletions)
            .field("nonstandard_changes", &self.nonstandard_changes)
            .field("applyable", &self.applyable)
            .field("apply_targets", &self.apply_targets)
            .field("sensitive_values", &"<redacted>")
            .finish()
    }
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
            resource_changes: Vec::new(),
            output_names,
            additions,
            changes,
            replacements: 0,
            deletions,
            nonstandard_changes: 0,
            applyable,
            apply_targets: Vec::new(),
            sensitive_values: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn with_resource_changes(
        mut self,
        resource_changes: Vec<PlanResource>,
        replacements: usize,
    ) -> Self {
        self.resource_changes = resource_changes;
        self.replacements = replacements;
        self
    }

    #[must_use]
    pub(crate) fn with_apply_targets(mut self, apply_targets: Vec<ExecutionTargetSpec>) -> Self {
        self.apply_targets = apply_targets;
        self
    }

    #[must_use]
    pub(crate) fn with_sensitive_values(mut self, sensitive_values: Vec<SensitiveValue>) -> Self {
        self.sensitive_values = sensitive_values;
        self
    }

    #[must_use]
    pub(crate) const fn with_nonstandard_changes(mut self, count: usize) -> Self {
        self.nonstandard_changes = count;
        self
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
    pub(crate) const fn replacements(&self) -> usize {
        self.replacements
    }

    #[must_use]
    pub(crate) const fn deletions(&self) -> usize {
        self.deletions
    }

    #[must_use]
    pub(crate) const fn nonstandard_changes(&self) -> usize {
        self.nonstandard_changes
    }

    #[must_use]
    pub(crate) const fn has_changes(&self) -> bool {
        self.additions > 0
            || self.changes > 0
            || self.replacements > 0
            || self.deletions > 0
            || self.nonstandard_changes > 0
            || !self.output_names.is_empty()
    }

    #[must_use]
    pub(crate) const fn applyable(&self) -> bool {
        self.applyable
    }

    #[must_use]
    pub(crate) fn apply_targets(&self) -> &[ExecutionTargetSpec] {
        &self.apply_targets
    }

    #[must_use]
    pub(crate) fn sensitive_values(&self) -> &[SensitiveValue] {
        &self.sensitive_values
    }

    pub(crate) fn destructive_addresses(&self) -> impl Iterator<Item = &str> {
        self.resource_changes
            .iter()
            .filter(|resource| resource.kind == ResourceChangeKind::Delete)
            .map(|resource| resource.address.as_str())
    }

    pub(crate) fn replacement_addresses(&self) -> impl Iterator<Item = &str> {
        self.resource_changes
            .iter()
            .filter(|resource| resource.is_replacement())
            .map(|resource| resource.address.as_str())
    }

    #[must_use]
    pub(crate) fn has_destructive_changes(&self) -> bool {
        self.deletions > 0
            || self.replacements > 0
            || self.destructive_addresses().next().is_some()
            || self.replacement_addresses().next().is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanReview {
    root: PathBuf,
    workspace: String,
    context: ExecutionContext,
    document: PlanDocument,
    metadata: PlanMetadata,
    plan: Plan,
    relations: PlanRelations,
    provider_schemas: Option<ProviderSchemas>,
    diagnostics: Vec<Diagnostic>,
    search_query: String,
    apply_allowed: bool,
    apply_entry: bool,
    previous_durations: Vec<Option<Duration>>,
}

impl PlanReview {
    #[must_use]
    pub(crate) fn new(
        root: PathBuf,
        workspace: String,
        document: PlanDocument,
        metadata: PlanMetadata,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        let context =
            ExecutionContext::loading(root.display().to_string()).with_workspace(workspace.clone());
        Self {
            root,
            workspace,
            context,
            document,
            metadata,
            plan: Plan::empty(),
            relations: PlanRelations::not_collected(),
            provider_schemas: None,
            diagnostics,
            search_query: String::new(),
            apply_allowed: true,
            apply_entry: false,
            previous_durations: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) const fn with_apply_allowed(mut self, allowed: bool) -> Self {
        self.apply_allowed = allowed;
        self
    }

    #[must_use]
    pub(crate) const fn with_apply_entry(mut self, apply_entry: bool) -> Self {
        self.apply_entry = apply_entry;
        self
    }

    #[must_use]
    pub(crate) fn with_previous_durations(
        mut self,
        previous_durations: Vec<Option<Duration>>,
    ) -> Self {
        self.previous_durations = previous_durations;
        self
    }

    #[must_use]
    pub(crate) const fn apply_allowed(&self) -> bool {
        self.apply_allowed
    }

    #[must_use]
    pub(crate) const fn apply_entry(&self) -> bool {
        self.apply_entry
    }

    #[must_use]
    pub(crate) fn previous_durations(&self) -> &[Option<Duration>] {
        &self.previous_durations
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
    pub(crate) const fn context(&self) -> &ExecutionContext {
        &self.context
    }

    #[must_use]
    pub(crate) fn with_context(mut self, context: ExecutionContext) -> Self {
        self.context = context;
        self
    }

    #[must_use]
    pub(crate) fn with_plan(mut self, plan: Plan) -> Self {
        self.plan = plan;
        self
    }

    #[must_use]
    pub(crate) fn with_relations(mut self, relations: PlanRelations) -> Self {
        self.relations = relations;
        self
    }

    #[must_use]
    pub(crate) fn with_provider_schemas(mut self, schemas: Option<ProviderSchemas>) -> Self {
        self.provider_schemas = schemas;
        self
    }

    #[must_use]
    pub(crate) fn confirmation_input(&self) -> String {
        let named =
            self.metadata.has_destructive_changes() || self.context.is_production() == Some(true);
        if named {
            match self.context.display_name() {
                ExecutionContextValue::Known(name) => name.clone(),
                ExecutionContextValue::Loading => String::new(),
            }
        } else {
            "yes".to_owned()
        }
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
    pub(crate) const fn plan(&self) -> &Plan {
        &self.plan
    }

    #[must_use]
    #[expect(
        dead_code,
        reason = "relation evidence is consumed by the planned graph review"
    )]
    pub(crate) const fn relations(&self) -> &PlanRelations {
        &self.relations
    }

    #[must_use]
    pub(crate) const fn provider_schemas(&self) -> Option<&ProviderSchemas> {
        self.provider_schemas.as_ref()
    }

    #[must_use]
    pub(crate) fn with_diagnostics(mut self, diagnostics: Vec<Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "review completion carries the complete plan for the UI"
)]
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
    use super::{PlanBlock, PlanBlockKind, PlanDocument, PlanLineKind};

    pub(crate) fn plan_document_with_blocks(text: String, blocks: Vec<PlanBlock>) -> PlanDocument {
        let line_kinds = vec![PlanLineKind::Body; text.split('\n').count()];
        PlanDocument::with_blocks_and_line_kinds(text, blocks, line_kinds)
    }

    pub(crate) fn plan_document(text: String) -> PlanDocument {
        let end = text.split('\n').count();
        plan_document_with_blocks(text, vec![PlanBlock::new(0..end, PlanBlockKind::Common)])
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{plan_document, plan_document_with_blocks};
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
        let document = plan_document_with_blocks(
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
            filtered.lines_with_indices().collect::<Vec<_>>(),
            [
                (0, "preamble"),
                (3, "resource worker"),
                (4, "worker value"),
                (5, "summary"),
                (6, ""),
            ]
        );
        assert_eq!(filtered.matching_resources(), 1);
        assert_eq!(filtered.matching_outputs(), 0);
    }

    #[test]
    fn filter_counts_each_resource_and_output_block_once() {
        let document = plan_document_with_blocks(
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
            resource
                .lines_with_indices()
                .map(|(_, line)| line)
                .collect::<Vec<_>>(),
            ["common api", "resource api api api", "unknown endpoint", ""]
        );
        assert_eq!(resource.matching_resources(), 1);
        assert_eq!(resource.matching_outputs(), 0);

        let output = document.filter("endpoint");
        assert_eq!(
            output
                .lines_with_indices()
                .map(|(_, line)| line)
                .collect::<Vec<_>>(),
            ["common api", "output endpoint", "unknown endpoint", ""]
        );
        assert_eq!(output.matching_resources(), 0);
        assert_eq!(output.matching_outputs(), 1);

        let mixed = document.filter("e");
        assert_eq!(mixed.matching_resources(), 2);
        assert_eq!(mixed.matching_outputs(), 1);
        assert_eq!(mixed.matching_resources() + mixed.matching_outputs(), 3);

        let empty = document.filter("");
        assert_eq!(empty.matching_resources(), 2);
        assert_eq!(empty.matching_outputs(), 1);
    }

    #[test]
    fn filter_keeps_common_text_when_no_searchable_block_matches() {
        let document = plan_document_with_blocks(
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
            filtered
                .lines_with_indices()
                .map(|(_, line)| line)
                .collect::<Vec<_>>(),
            ["diagnostic only", "unknown boundary text", ""]
        );
        assert_eq!(
            filtered.matching_resources() + filtered.matching_outputs(),
            0
        );
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
