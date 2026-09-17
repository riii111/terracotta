use std::path::{Path, PathBuf};

use super::{
    attribution::{AnalysisIssue, ResourceAttribution},
    plan::Plan,
    progress::ExecutionEvent,
    source_location::SourceFileAnalysis,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReviewComparisonBasis {
    WorkingTreeVsHead,
    HeadVsMergeBase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReviewComparisonStatus {
    Complete,
    Incomplete(String),
}

impl ReviewComparisonStatus {
    #[must_use]
    pub(crate) const fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    #[must_use]
    pub(crate) fn message(&self) -> Option<&str> {
        match self {
            Self::Complete => None,
            Self::Incomplete(message) => Some(message),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewComparison {
    basis: ReviewComparisonBasis,
    compare_ref: Option<String>,
    resolved_commit: Option<String>,
    head_commit: Option<String>,
    merge_base: Option<String>,
    status: ReviewComparisonStatus,
}

impl ReviewComparison {
    #[must_use]
    pub(crate) fn new(
        basis: ReviewComparisonBasis,
        compare_ref: Option<String>,
        resolved_commit: Option<String>,
        head_commit: Option<String>,
        merge_base: Option<String>,
        status: ReviewComparisonStatus,
    ) -> Self {
        Self {
            basis,
            compare_ref,
            resolved_commit,
            head_commit,
            merge_base,
            status,
        }
    }

    #[must_use]
    pub(crate) const fn basis(&self) -> ReviewComparisonBasis {
        self.basis
    }

    #[must_use]
    pub(crate) fn compare_ref(&self) -> Option<&str> {
        self.compare_ref.as_deref()
    }

    #[must_use]
    pub(crate) fn resolved_commit(&self) -> Option<&str> {
        self.resolved_commit.as_deref()
    }

    #[must_use]
    pub(crate) fn head_commit(&self) -> Option<&str> {
        self.head_commit.as_deref()
    }

    #[must_use]
    pub(crate) fn merge_base(&self) -> Option<&str> {
        self.merge_base.as_deref()
    }

    #[must_use]
    pub(crate) const fn status(&self) -> &ReviewComparisonStatus {
        &self.status
    }

    #[must_use]
    pub(crate) fn label(&self) -> String {
        match self.basis {
            ReviewComparisonBasis::WorkingTreeVsHead => "working tree vs HEAD".to_owned(),
            ReviewComparisonBasis::HeadVsMergeBase => self.compare_ref.as_deref().map_or_else(
                || "HEAD vs merge-base".to_owned(),
                |compare_ref| format!("HEAD vs merge-base({compare_ref})"),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanReview {
    root: PathBuf,
    workspace: String,
    git: String,
    plan: Plan,
    source_files: Vec<SourceFileAnalysis>,
    attributions: Vec<ResourceAttribution>,
    comparison: ReviewComparison,
    analysis_issues: Vec<AnalysisIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanReviewMessage {
    Event(ExecutionEvent),
    Completed(PlanReview),
    Failed { message: String, interrupted: bool },
}

impl PlanReview {
    #[must_use]
    pub(crate) const fn new(
        root: PathBuf,
        workspace: String,
        plan: Plan,
        source_files: Vec<SourceFileAnalysis>,
        attributions: Vec<ResourceAttribution>,
        comparison: ReviewComparison,
        analysis_issues: Vec<AnalysisIssue>,
    ) -> Self {
        Self {
            root,
            workspace,
            git: "unavailable".to_owned(),
            plan,
            source_files,
            attributions,
            comparison,
            analysis_issues,
        }
    }

    pub(crate) fn with_git(mut self, git: String) -> Self {
        self.git = git;
        self
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
    pub(crate) fn git(&self) -> &str {
        &self.git
    }

    #[must_use]
    pub(crate) const fn plan(&self) -> &Plan {
        &self.plan
    }

    #[must_use]
    pub(crate) fn source_files(&self) -> &[SourceFileAnalysis] {
        &self.source_files
    }

    #[must_use]
    pub(crate) fn attributions(&self) -> &[ResourceAttribution] {
        &self.attributions
    }

    #[must_use]
    pub(crate) const fn comparison(&self) -> &ReviewComparison {
        &self.comparison
    }

    #[must_use]
    pub(crate) fn analysis_issues(&self) -> &[AnalysisIssue] {
        &self.analysis_issues
    }

    #[must_use]
    pub(crate) fn needs_review_count(&self) -> usize {
        self.attributions
            .iter()
            .filter(|attribution| attribution.needs_review())
            .count()
    }
}
