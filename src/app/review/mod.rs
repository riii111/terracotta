use std::{
    fmt::{self, Display, Formatter},
    path::{Path, PathBuf},
};

use super::{
    attribution::{AnalysisIssue, ResourceAttribution, SourceFileAnalysis},
    execution::ExecutionEvent,
    plan::Plan,
};

mod detail;
mod diagnostics;
mod item;
mod list;

pub(crate) use detail::{
    AttributeGroup, DetailAction, DetailRow, ResourceNavigation, ReviewDetailState,
};
pub(crate) use diagnostics::ReviewDiagnosticsState;
pub(crate) use item::PlanListItem;
pub(crate) use list::{PlanListAction, PlanListFilter, PlanListState};

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
    status: ReviewComparisonStatus,
}

impl ReviewComparison {
    #[must_use]
    pub(crate) const fn new(
        basis: ReviewComparisonBasis,
        compare_ref: Option<String>,
        status: ReviewComparisonStatus,
    ) -> Self {
        Self {
            basis,
            compare_ref,
            status,
        }
    }

    #[must_use]
    pub(crate) const fn status(&self) -> &ReviewComparisonStatus {
        &self.status
    }

    #[must_use]
    pub(crate) const fn basis(&self) -> ReviewComparisonBasis {
        self.basis
    }

    #[must_use]
    pub(crate) fn compare_ref(&self) -> Option<&str> {
        self.compare_ref.as_deref()
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) const fn working_tree() -> Self {
        Self::new(
            ReviewComparisonBasis::WorkingTreeVsHead,
            None,
            ReviewComparisonStatus::Complete,
        )
    }

    #[must_use]
    pub(crate) fn label(&self) -> String {
        match self.basis() {
            ReviewComparisonBasis::WorkingTreeVsHead => "working tree vs HEAD".to_owned(),
            ReviewComparisonBasis::HeadVsMergeBase => self.compare_ref().map_or_else(
                || "HEAD vs merge-base".to_owned(),
                |compare_ref| format!("HEAD vs merge-base({compare_ref})"),
            ),
        }
    }
}

impl Display for ReviewComparison {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanReview {
    root: PathBuf,
    repository_root: Option<PathBuf>,
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
    pub(crate) fn new(
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
            repository_root: None,
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

    pub(crate) fn with_repository_root(mut self, repository_root: Option<PathBuf>) -> Self {
        self.repository_root = repository_root;
        self
    }

    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn repository_root(&self) -> Option<&Path> {
        self.repository_root.as_deref()
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

#[cfg(test)]
mod tests {
    use super::*;

    impl ReviewComparisonStatus {
        pub(crate) const fn is_complete(&self) -> bool {
            matches!(self, Self::Complete)
        }
    }
}
