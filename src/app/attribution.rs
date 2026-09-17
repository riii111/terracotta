#![allow(
    clippy::redundant_pub_crate,
    reason = "attribution types are shared only within the crate"
)]

use std::path::{Path, PathBuf};

use super::plan::{ResourceChange, ResourceChangeKind, ResourceMode};
use super::source_location::{
    ResourceAddress, ResourceSourceLocation, SourceFileAnalysis, SourceIssue, SourceIssueKind,
    SourceRange, SourceSide,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttributionStatus {
    Direct,
    NoMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnalysisIssueKind {
    UnsupportedAddress,
    UnsupportedResourceMode,
    Source(SourceIssueKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnalysisIssue {
    kind: AnalysisIssueKind,
    path: Option<PathBuf>,
    side: Option<SourceSide>,
    message: String,
}

impl AnalysisIssue {
    #[must_use]
    fn unsupported_address(address: &str) -> Self {
        Self {
            kind: AnalysisIssueKind::UnsupportedAddress,
            path: None,
            side: None,
            message: format!("resource address is outside direct matching: {address}"),
        }
    }

    #[must_use]
    fn unsupported_resource_mode(address: &str) -> Self {
        Self {
            kind: AnalysisIssueKind::UnsupportedResourceMode,
            path: None,
            side: None,
            message: format!("resource mode is outside direct matching: {address}"),
        }
    }

    #[must_use]
    fn source(path: &Path, side: SourceSide, issue: &SourceIssue) -> Self {
        Self {
            kind: AnalysisIssueKind::Source(issue.kind()),
            path: Some(path.to_owned()),
            side: Some(side),
            message: issue.message().to_owned(),
        }
    }

    #[must_use]
    pub(crate) const fn kind(&self) -> AnalysisIssueKind {
        self.kind
    }

    #[must_use]
    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    #[must_use]
    pub(crate) const fn side(&self) -> Option<SourceSide> {
        self.side
    }

    #[must_use]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AnalysisStatus {
    Complete,
    Incomplete(Vec<AnalysisIssue>),
}

impl AnalysisStatus {
    #[must_use]
    fn from_issues(issues: Vec<AnalysisIssue>) -> Self {
        if issues.is_empty() {
            Self::Complete
        } else {
            Self::Incomplete(issues)
        }
    }

    #[must_use]
    pub(crate) const fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    #[must_use]
    pub(crate) fn issues(&self) -> &[AnalysisIssue] {
        match self {
            Self::Complete => &[],
            Self::Incomplete(issues) => issues,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLineChange {
    path: PathBuf,
    side: SourceSide,
    range: SourceRange,
}

impl SourceLineChange {
    #[must_use]
    pub(crate) fn new(path: impl Into<PathBuf>, side: SourceSide, range: SourceRange) -> Self {
        Self {
            path: path.into(),
            side,
            range,
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
    pub(crate) const fn range(&self) -> SourceRange {
        self.range
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributionEvidence {
    path: PathBuf,
    side: SourceSide,
    range: SourceRange,
}

impl AttributionEvidence {
    #[must_use]
    fn from_location(location: &ResourceSourceLocation) -> Self {
        Self {
            path: location.path().to_owned(),
            side: location.side(),
            range: location.range(),
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
    pub(crate) const fn range(&self) -> SourceRange {
        self.range
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceAttribution {
    address: String,
    status: AttributionStatus,
    evidence: Vec<AttributionEvidence>,
    analysis: AnalysisStatus,
}

impl ResourceAttribution {
    #[must_use]
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    #[must_use]
    pub(crate) const fn status(&self) -> AttributionStatus {
        self.status
    }

    #[must_use]
    pub(crate) fn evidence(&self) -> &[AttributionEvidence] {
        &self.evidence
    }

    #[must_use]
    pub(crate) const fn analysis(&self) -> &AnalysisStatus {
        &self.analysis
    }

    #[must_use]
    pub(crate) const fn needs_review(&self) -> bool {
        matches!(self.status, AttributionStatus::NoMatch) || !self.analysis.is_complete()
    }
}

pub(crate) fn attribute_changes(
    changes: &[ResourceChange],
    source_files: &[SourceFileAnalysis],
    changed_lines: &[SourceLineChange],
) -> Vec<ResourceAttribution> {
    changes
        .iter()
        .map(|change| attribute_change(change, source_files, changed_lines))
        .collect()
}

fn attribute_change(
    change: &ResourceChange,
    source_files: &[SourceFileAnalysis],
    changed_lines: &[SourceLineChange],
) -> ResourceAttribution {
    let mut issues = source_issues(change.kind, source_files);
    let block_address = root_resource_address(&change.address);

    if block_address.is_none() {
        issues.push(AnalysisIssue::unsupported_address(&change.address));
    }
    if change.mode != ResourceMode::Managed {
        issues.push(AnalysisIssue::unsupported_resource_mode(&change.address));
    }

    let evidence = block_address
        .filter(|_| change.mode == ResourceMode::Managed)
        .map_or_else(Vec::new, |block_address| {
            matching_evidence(&block_address, change.kind, source_files, changed_lines)
        });

    ResourceAttribution {
        address: change.address.clone(),
        status: if evidence.is_empty() {
            AttributionStatus::NoMatch
        } else {
            AttributionStatus::Direct
        },
        evidence,
        analysis: AnalysisStatus::from_issues(issues),
    }
}

fn source_issues(
    kind: ResourceChangeKind,
    source_files: &[SourceFileAnalysis],
) -> Vec<AnalysisIssue> {
    source_files
        .iter()
        .filter(|file| relevant_side(kind, file.side()))
        .flat_map(|file| {
            file.issues()
                .iter()
                .map(|issue| AnalysisIssue::source(file.path(), file.side(), issue))
        })
        .collect()
}

fn matching_evidence(
    address: &ResourceAddress,
    kind: ResourceChangeKind,
    source_files: &[SourceFileAnalysis],
    changed_lines: &[SourceLineChange],
) -> Vec<AttributionEvidence> {
    source_files
        .iter()
        .filter(|file| relevant_side(kind, file.side()))
        .flat_map(SourceFileAnalysis::resources)
        .filter(|location| location.address() == address)
        .filter(|location| {
            changed_lines.iter().any(|change| {
                change.side() == location.side()
                    && change.path() == location.path()
                    && ranges_overlap(change.range(), location.range())
            })
        })
        .map(AttributionEvidence::from_location)
        .collect()
}

fn relevant_side(kind: ResourceChangeKind, side: SourceSide) -> bool {
    match kind {
        ResourceChangeKind::Create => side == SourceSide::After,
        ResourceChangeKind::Delete => side == SourceSide::Before,
        ResourceChangeKind::Update | ResourceChangeKind::Replace => true,
    }
}

const fn ranges_overlap(left: SourceRange, right: SourceRange) -> bool {
    left.start_line() <= right.end_line() && right.start_line() <= left.end_line()
}

fn root_resource_address(address: &str) -> Option<ResourceAddress> {
    if address.starts_with("module.") {
        return None;
    }

    let (resource_type, name_and_instance) = address.split_once('.')?;
    let instance_start = name_and_instance.find('[');
    let name_end = instance_start.unwrap_or(name_and_instance.len());
    let name = &name_and_instance[..name_end];
    let instance = &name_and_instance[name_end..];

    if resource_type.is_empty()
        || name.is_empty()
        || name.contains('.')
        || (!instance.is_empty() && (!instance.starts_with('[') || !instance.ends_with(']')))
    {
        return None;
    }

    Some(ResourceAddress::new(resource_type, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(address: &str, kind: ResourceChangeKind) -> ResourceChange {
        let actions = match kind {
            ResourceChangeKind::Create => vec![super::super::plan::PlanAction::Create],
            ResourceChangeKind::Update => vec![super::super::plan::PlanAction::Update],
            ResourceChangeKind::Replace => vec![
                super::super::plan::PlanAction::Delete,
                super::super::plan::PlanAction::Create,
            ],
            ResourceChangeKind::Delete => vec![super::super::plan::PlanAction::Delete],
        };

        ResourceChange {
            address: address.to_owned(),
            mode: ResourceMode::Managed,
            actions,
            kind,
            before: None,
            after: None,
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
        }
    }

    fn location(
        resource_type: &str,
        name: &str,
        path: &str,
        side: SourceSide,
        start_line: usize,
        end_line: usize,
    ) -> ResourceSourceLocation {
        ResourceSourceLocation::new(
            ResourceAddress::new(resource_type, name),
            PathBuf::from(path),
            side,
            SourceRange::new(start_line, end_line),
        )
    }

    fn source_file(
        path: &str,
        side: SourceSide,
        resources: Vec<ResourceSourceLocation>,
        issues: Vec<SourceIssue>,
    ) -> SourceFileAnalysis {
        SourceFileAnalysis::new(PathBuf::from(path), side, resources, issues)
    }

    fn changed_line(
        path: &str,
        side: SourceSide,
        start_line: usize,
        end_line: usize,
    ) -> SourceLineChange {
        SourceLineChange::new(
            PathBuf::from(path),
            side,
            SourceRange::new(start_line, end_line),
        )
    }

    #[test]
    fn create_matches_changed_lines_inside_after_block() {
        let changes = [change("aws_instance.api", ResourceChangeKind::Create)];
        let sources = [source_file(
            "main.tf",
            SourceSide::After,
            vec![location(
                "aws_instance",
                "api",
                "main.tf",
                SourceSide::After,
                10,
                20,
            )],
            Vec::new(),
        )];
        let changed_lines = [changed_line("main.tf", SourceSide::After, 20, 20)];

        let result = attribute_changes(&changes, &sources, &changed_lines);

        assert_eq!(result[0].status(), AttributionStatus::Direct);
        assert_eq!(result[0].evidence().len(), 1);
        assert_eq!(result[0].evidence()[0].path(), Path::new("main.tf"));
        assert_eq!(result[0].evidence()[0].side(), SourceSide::After);
        assert_eq!(result[0].evidence()[0].range(), SourceRange::new(10, 20));
        assert!(result[0].analysis().is_complete());
        assert!(!result[0].needs_review());
    }

    #[test]
    fn delete_matches_changed_lines_inside_before_block() {
        let changes = [change("aws_instance.old", ResourceChangeKind::Delete)];
        let sources = [source_file(
            "main.tf",
            SourceSide::Before,
            vec![location(
                "aws_instance",
                "old",
                "main.tf",
                SourceSide::Before,
                2,
                4,
            )],
            Vec::new(),
        )];
        let changed_lines = [changed_line("main.tf", SourceSide::Before, 2, 2)];

        let result = attribute_changes(&changes, &sources, &changed_lines);

        assert_eq!(result[0].status(), AttributionStatus::Direct);
        assert_eq!(result[0].evidence()[0].side(), SourceSide::Before);
    }

    #[test]
    fn update_and_replace_consider_both_source_sides() {
        let changes = [
            change("aws_instance.api", ResourceChangeKind::Update),
            change("aws_instance.worker", ResourceChangeKind::Replace),
        ];
        let sources = [
            source_file(
                "before.tf",
                SourceSide::Before,
                vec![location(
                    "aws_instance",
                    "api",
                    "before.tf",
                    SourceSide::Before,
                    1,
                    4,
                )],
                Vec::new(),
            ),
            source_file(
                "after.tf",
                SourceSide::After,
                vec![location(
                    "aws_instance",
                    "api",
                    "after.tf",
                    SourceSide::After,
                    1,
                    4,
                )],
                Vec::new(),
            ),
            source_file(
                "worker.tf",
                SourceSide::Before,
                vec![location(
                    "aws_instance",
                    "worker",
                    "worker.tf",
                    SourceSide::Before,
                    10,
                    14,
                )],
                Vec::new(),
            ),
            source_file(
                "worker.tf",
                SourceSide::After,
                vec![location(
                    "aws_instance",
                    "worker",
                    "worker.tf",
                    SourceSide::After,
                    11,
                    15,
                )],
                Vec::new(),
            ),
        ];
        let changed_lines = [
            changed_line("before.tf", SourceSide::Before, 2, 2),
            changed_line("after.tf", SourceSide::After, 3, 3),
            changed_line("worker.tf", SourceSide::Before, 14, 14),
            changed_line("worker.tf", SourceSide::After, 11, 11),
        ];

        let result = attribute_changes(&changes, &sources, &changed_lines);

        assert_eq!(result[0].status(), AttributionStatus::Direct);
        assert_eq!(result[0].evidence().len(), 2);
        assert_eq!(result[1].status(), AttributionStatus::Direct);
        assert_eq!(result[1].evidence().len(), 2);
    }

    #[test]
    fn instances_match_their_root_resource_block() {
        let changes = [
            change("aws_instance.api[0]", ResourceChangeKind::Update),
            change("aws_instance.api[\"blue\"]", ResourceChangeKind::Update),
        ];
        let sources = [source_file(
            "main.tf",
            SourceSide::After,
            vec![location(
                "aws_instance",
                "api",
                "main.tf",
                SourceSide::After,
                1,
                6,
            )],
            Vec::new(),
        )];
        let changed_lines = [changed_line("main.tf", SourceSide::After, 3, 3)];

        let result = attribute_changes(&changes, &sources, &changed_lines);

        assert!(
            result
                .iter()
                .all(|attribution| attribution.status() == AttributionStatus::Direct)
        );
    }

    #[test]
    fn module_addresses_are_not_mapped_to_root_resources() {
        let changes = [change(
            "module.network.aws_instance.api[0]",
            ResourceChangeKind::Update,
        )];
        let sources = [source_file(
            "main.tf",
            SourceSide::After,
            vec![location(
                "aws_instance",
                "api",
                "main.tf",
                SourceSide::After,
                1,
                6,
            )],
            Vec::new(),
        )];
        let changed_lines = [changed_line("main.tf", SourceSide::After, 3, 3)];

        let result = attribute_changes(&changes, &sources, &changed_lines);

        assert_eq!(result[0].status(), AttributionStatus::NoMatch);
        assert!(matches!(
            result[0].analysis(),
            AnalysisStatus::Incomplete(issues)
                if issues.iter().any(|issue| issue.kind() == AnalysisIssueKind::UnsupportedAddress)
        ));
        assert!(result[0].needs_review());
    }

    #[test]
    fn non_overlapping_change_is_complete_no_match() {
        let changes = [change("aws_instance.api", ResourceChangeKind::Update)];
        let sources = [source_file(
            "main.tf",
            SourceSide::After,
            vec![location(
                "aws_instance",
                "api",
                "main.tf",
                SourceSide::After,
                10,
                20,
            )],
            Vec::new(),
        )];
        let changed_lines = [changed_line("main.tf", SourceSide::After, 21, 21)];

        let result = attribute_changes(&changes, &sources, &changed_lines);

        assert_eq!(result[0].status(), AttributionStatus::NoMatch);
        assert!(result[0].analysis().is_complete());
        assert!(result[0].needs_review());
    }

    #[test]
    fn source_issue_keeps_evidence_and_marks_analysis_incomplete() {
        let changes = [change("aws_instance.api", ResourceChangeKind::Update)];
        let sources = [
            source_file(
                "main.tf",
                SourceSide::After,
                vec![location(
                    "aws_instance",
                    "api",
                    "main.tf",
                    SourceSide::After,
                    1,
                    6,
                )],
                Vec::new(),
            ),
            source_file(
                "broken.tf",
                SourceSide::After,
                Vec::new(),
                vec![SourceIssue::new(
                    SourceIssueKind::SyntaxError,
                    "broken source",
                )],
            ),
        ];
        let changed_lines = [changed_line("main.tf", SourceSide::After, 3, 3)];

        let result = attribute_changes(&changes, &sources, &changed_lines);

        assert_eq!(result[0].status(), AttributionStatus::Direct);
        assert_eq!(result[0].evidence().len(), 1);
        assert!(!result[0].analysis().is_complete());
        assert_eq!(result[0].analysis().issues().len(), 1);
        assert_eq!(
            result[0].analysis().issues()[0].kind(),
            AnalysisIssueKind::Source(SourceIssueKind::SyntaxError)
        );
        assert!(result[0].needs_review());
    }

    #[test]
    fn multiple_matching_blocks_are_retained_as_evidence() {
        let changes = [change("aws_instance.api", ResourceChangeKind::Update)];
        let sources = [
            source_file(
                "main.tf",
                SourceSide::After,
                vec![location(
                    "aws_instance",
                    "api",
                    "main.tf",
                    SourceSide::After,
                    1,
                    5,
                )],
                Vec::new(),
            ),
            source_file(
                "extra.tf",
                SourceSide::After,
                vec![location(
                    "aws_instance",
                    "api",
                    "extra.tf",
                    SourceSide::After,
                    8,
                    12,
                )],
                Vec::new(),
            ),
        ];
        let changed_lines = [
            changed_line("main.tf", SourceSide::After, 2, 2),
            changed_line("extra.tf", SourceSide::After, 10, 10),
        ];

        let result = attribute_changes(&changes, &sources, &changed_lines);

        assert_eq!(result[0].status(), AttributionStatus::Direct);
        assert_eq!(result[0].evidence().len(), 2);
    }

    #[test]
    fn non_managed_resources_are_not_directly_matched() {
        let mut resource_change = change("aws_instance.api", ResourceChangeKind::Update);
        resource_change.mode = ResourceMode::Data;
        let sources = [source_file(
            "main.tf",
            SourceSide::After,
            vec![location(
                "aws_instance",
                "api",
                "main.tf",
                SourceSide::After,
                1,
                6,
            )],
            Vec::new(),
        )];
        let changed_lines = [changed_line("main.tf", SourceSide::After, 3, 3)];

        let result = attribute_changes(&[resource_change], &sources, &changed_lines);

        assert_eq!(result[0].status(), AttributionStatus::NoMatch);
        assert!(matches!(
            result[0].analysis(),
            AnalysisStatus::Incomplete(issues)
                if issues.iter().any(|issue| issue.kind() == AnalysisIssueKind::UnsupportedResourceMode)
        ));
    }
}
