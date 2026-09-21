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
    line_kinds: Vec<PlanLineKind>,
}

pub(crate) struct FilteredPlan<'a> {
    lines: Vec<&'a str>,
    line_indices: Vec<usize>,
    matching_resources: usize,
    matching_outputs: usize,
}

impl<'a> FilteredPlan<'a> {
    #[must_use]
    #[cfg(test)]
    pub(crate) fn lines(&self) -> &[&'a str] {
        &self.lines
    }

    pub(crate) fn lines_with_indices(&self) -> impl Iterator<Item = (usize, &'a str)> + '_ {
        self.line_indices
            .iter()
            .copied()
            .zip(self.lines.iter().copied())
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
    pub(crate) fn with_blocks(text: String, blocks: Vec<PlanBlock>) -> Self {
        let line_kinds = classify_display_lines(&text);
        Self {
            text,
            blocks,
            line_kinds,
        }
    }

    #[must_use]
    pub(crate) const fn with_blocks_and_line_kinds(
        text: String,
        blocks: Vec<PlanBlock>,
        line_kinds: Vec<PlanLineKind>,
    ) -> Self {
        Self {
            text,
            blocks,
            line_kinds,
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
        let mut line_indices = Vec::new();
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
                filtered.push(lines[line]);
                line_indices.push(line);
            }
        }
        FilteredPlan {
            lines: filtered,
            line_indices,
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
}

pub(crate) fn classify_display_lines(text: &str) -> Vec<PlanLineKind> {
    let lines = text.split('\n').collect::<Vec<_>>();
    let intro_end = leading_intro_end(&lines);
    let final_summary = lines
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| !line.trim().is_empty())
        .and_then(|(index, line)| is_terraform_summary(line).then_some(index));
    let mut kinds = vec![PlanLineKind::Body; lines.len()];
    for kind in kinds.iter_mut().take(intro_end) {
        *kind = PlanLineKind::Intro;
    }

    let mut heredoc_terminator: Option<String> = None;
    for (line_index, line) in lines.iter().enumerate().skip(intro_end) {
        if let Some(terminator) = &heredoc_terminator {
            if heredoc_end(line, terminator) {
                heredoc_terminator = None;
            }
            continue;
        }
        if line == &"Changes to Outputs:" {
            kinds[line_index] = PlanLineKind::OutputSection;
        } else if Some(line_index) == final_summary {
            kinds[line_index] = PlanLineKind::Summary;
        } else if is_note_line(line) {
            kinds[line_index] = PlanLineKind::Note;
        }
        heredoc_terminator = heredoc_start(line);
    }
    kinds
}

fn is_terraform_summary(line: &str) -> bool {
    let Some(summary) = line
        .strip_prefix("Plan: ")
        .and_then(|summary| summary.strip_suffix('.'))
    else {
        return false;
    };
    let mut parts = summary.split(", ");
    let Some(additions) = parts.next().and_then(|part| part.strip_suffix(" to add")) else {
        return false;
    };
    let Some(changes) = parts
        .next()
        .and_then(|part| part.strip_suffix(" to change"))
    else {
        return false;
    };
    let Some(deletions) = parts
        .next()
        .and_then(|part| part.strip_suffix(" to destroy"))
    else {
        return false;
    };
    parts.next().is_none()
        && !additions.is_empty()
        && !changes.is_empty()
        && !deletions.is_empty()
        && additions
            .chars()
            .all(|character| character.is_ascii_digit())
        && changes.chars().all(|character| character.is_ascii_digit())
        && deletions
            .chars()
            .all(|character| character.is_ascii_digit())
}

fn leading_intro_end(lines: &[&str]) -> usize {
    let mut index = 0;
    while lines.get(index).is_some_and(|line| line.trim().is_empty()) {
        index += 1;
    }
    let mut recognized = false;
    while let Some(line) = lines.get(index) {
        if is_intro_line(line) {
            recognized = true;
            index += 1;
        } else if recognized && line.trim().is_empty() {
            index += 1;
        } else {
            break;
        }
    }
    index
}

fn is_intro_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("Terraform used the selected providers")
        || trimmed.starts_with("Resource actions are indicated with the following symbols:")
        || trimmed.starts_with("plan. Resource actions are indicated with the following symbols:")
        || trimmed == "+ create"
        || trimmed == "~ update in-place"
        || trimmed == "-/+ destroy and then create replacement"
        || trimmed == "- destroy"
        || trimmed == "<= read (data resources)"
        || trimmed == "Terraform will perform the following actions:"
}

fn is_note_line(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

fn heredoc_start(line: &str) -> Option<String> {
    let mut quoted = false;
    let mut escaped = false;
    let marker = line.char_indices().find_map(|(index, character)| {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            return None;
        }
        if character == '"' {
            quoted = true;
            return None;
        }
        (character == '<'
            && line[index..].starts_with("<<")
            && line[..index].trim_end().ends_with('='))
        .then_some(index)
    })?;
    let mut value = line[marker + 2..].trim_start();
    value = value.strip_prefix('-').unwrap_or(value).trim_start();
    let terminator = value.split_whitespace().next()?;
    (!terminator.is_empty()
        && terminator
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-')))
    .then(|| terminator.to_owned())
}

fn heredoc_end(line: &str, terminator: &str) -> bool {
    let trimmed = line.trim();
    trimmed == terminator
        || trimmed
            .strip_prefix(terminator)
            .is_some_and(|suffix| suffix.trim_start().starts_with("->"))
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
        assert_eq!(filtered.matching_resources(), 1);
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
        assert_eq!(resource.matching_resources(), 1);
        assert_eq!(resource.matching_outputs(), 0);

        let output = document.filter("endpoint");
        assert_eq!(
            output.lines(),
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

    #[test]
    fn classifies_only_a_final_standard_summary_outside_a_heredoc() {
        let text = "  value = <<EOF\n".to_owned()
            + "Plan: 9 to add, 9 to change, 9 to destroy.\n"
            + "EOF\n"
            + "Plan: 1 to add, 2 to change, 3 to destroy.\n";
        let document = plan_document(text);
        let lines = document.text().split('\n').collect::<Vec<_>>();

        assert_eq!(document.line_kind(1), PlanLineKind::Body);
        assert_eq!(document.line_kind(3), PlanLineKind::Summary);
        assert_eq!(lines[1], "Plan: 9 to add, 9 to change, 9 to destroy.");
    }

    #[test]
    fn keeps_unknown_plan_text_when_it_is_not_the_final_standard_summary() {
        let document =
            plan_document("Plan: this is application text\nfollowing body text\n".to_owned());

        assert_eq!(document.line_kind(0), PlanLineKind::Body);
        assert_eq!(document.line_kind(1), PlanLineKind::Body);
    }
}
