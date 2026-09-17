use std::fmt::{Display, Formatter};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use super::attribution::{AttributionStatus, ResourceAttribution};
use super::plan::{Plan, PlanSummary, ResourceChange, ResourceChangeKind, UnsupportedChangeKind};
use super::review::PlanReview;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanListError {
    AttributionCountMismatch {
        changes: usize,
        attributions: usize,
    },
    AttributionAddressMismatch {
        index: usize,
        change: String,
        attribution: String,
    },
}

impl Display for PlanListError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AttributionCountMismatch {
                changes,
                attributions,
            } => write!(
                formatter,
                "plan changes ({changes}) and attributions ({attributions}) differ"
            ),
            Self::AttributionAddressMismatch {
                index,
                change,
                attribution,
            } => write!(
                formatter,
                "plan change {index} ({change}) has attribution for {attribution}"
            ),
        }
    }
}

impl std::error::Error for PlanListError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanListAction {
    SelectPrevious,
    SelectNext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanListState {
    context: Option<PlanListContext>,
    comparison: String,
    summary: PlanSummary,
    items: Vec<PlanListItem>,
    unsupported: Vec<UnsupportedChangeKind>,
    analysis_issues: Vec<String>,
    selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanListContext {
    root: PathBuf,
    workspace: String,
    git: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanListItem {
    change: ResourceChange,
    attribution: ResourceAttribution,
}

impl PlanListState {
    pub(crate) fn from_review(review: &PlanReview) -> Result<Self, PlanListError> {
        let mut state = Self::from_plan(
            review.plan().clone(),
            review.attributions().to_vec(),
            review.comparison().label(),
        )?;

        if let Some(message) = review.comparison().status().message() {
            state.analysis_issues.push(message.to_owned());
        }
        for issue in review.analysis_issues() {
            if !state
                .analysis_issues
                .iter()
                .any(|known| known == issue.message())
            {
                state.analysis_issues.push(issue.message().to_owned());
            }
        }
        state.context = Some(PlanListContext {
            root: review.root().to_owned(),
            workspace: review.workspace().to_owned(),
            git: review.git().to_owned(),
        });

        Ok(state)
    }

    pub(crate) fn from_plan(
        plan: Plan,
        attributions: Vec<ResourceAttribution>,
        comparison: impl Into<String>,
    ) -> Result<Self, PlanListError> {
        if plan.changes.len() != attributions.len() {
            return Err(PlanListError::AttributionCountMismatch {
                changes: plan.changes.len(),
                attributions: attributions.len(),
            });
        }

        let items = plan
            .changes
            .into_iter()
            .zip(attributions)
            .enumerate()
            .map(|(index, (change, attribution))| {
                if change.address != attribution.address() {
                    return Err(PlanListError::AttributionAddressMismatch {
                        index,
                        change: change.address,
                        attribution: attribution.address().to_owned(),
                    });
                }

                Ok(PlanListItem {
                    change,
                    attribution,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            context: None,
            comparison: comparison.into(),
            summary: plan.summary,
            items,
            unsupported: plan
                .unsupported_changes
                .into_iter()
                .map(|change| change.kind)
                .collect(),
            analysis_issues: Vec::new(),
            selected: 0,
        })
    }

    pub(crate) fn empty(comparison: impl Into<String>) -> Self {
        Self {
            context: None,
            comparison: comparison.into(),
            summary: PlanSummary::default(),
            items: Vec::new(),
            unsupported: Vec::new(),
            analysis_issues: Vec::new(),
            selected: 0,
        }
    }

    pub(crate) fn apply(&mut self, action: PlanListAction) {
        match action {
            PlanListAction::SelectPrevious => {
                self.selected = self.selected.saturating_sub(1);
            }
            PlanListAction::SelectNext => {
                if let Some(last) = self.items.len().checked_sub(1) {
                    self.selected = (self.selected + 1).min(last);
                }
            }
        }
    }

    #[must_use]
    pub(crate) fn comparison(&self) -> &str {
        &self.comparison
    }

    #[must_use]
    pub(crate) const fn context(&self) -> Option<&PlanListContext> {
        self.context.as_ref()
    }

    #[must_use]
    pub(crate) const fn summary(&self) -> PlanSummary {
        self.summary
    }

    #[must_use]
    pub(crate) fn items(&self) -> &[PlanListItem] {
        &self.items
    }

    #[must_use]
    pub(crate) const fn selected(&self) -> usize {
        self.selected
    }

    #[must_use]
    pub(crate) fn needs_review_count(&self) -> usize {
        self.items.iter().filter(|item| item.needs_review()).count()
    }

    #[must_use]
    pub(crate) fn unsupported_summary(&self) -> Option<String> {
        if self.unsupported.is_empty() {
            return None;
        }

        let mut counts = BTreeMap::new();
        for kind in &self.unsupported {
            *counts.entry(kind.label()).or_insert(0_usize) += 1;
        }
        let kinds = counts
            .into_iter()
            .map(|(label, count)| format!("{label} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("Unshown changes: {kinds}"))
    }

    #[must_use]
    pub(crate) fn analysis_issues(&self) -> &[String] {
        &self.analysis_issues
    }
}

impl PlanListItem {
    #[must_use]
    pub(crate) fn address(&self) -> &str {
        &self.change.address
    }

    #[must_use]
    pub(crate) const fn kind(&self) -> ResourceChangeKind {
        self.change.kind
    }

    #[must_use]
    pub(crate) const fn needs_review(&self) -> bool {
        self.attribution.needs_review()
    }

    #[must_use]
    pub(crate) fn git_label(&self) -> String {
        if !self.attribution.analysis().is_complete() {
            return "incomplete".to_owned();
        }

        match self.attribution.status() {
            AttributionStatus::NoMatch => "no match".to_owned(),
            AttributionStatus::Direct => self.attribution.evidence().first().map_or_else(
                || "direct".to_owned(),
                |evidence| {
                    let range = evidence.range();
                    let location = if range.start_line() == range.end_line() {
                        format!("{}:{}", evidence.path().display(), range.start_line())
                    } else {
                        format!(
                            "{}:{}-{}",
                            evidence.path().display(),
                            range.start_line(),
                            range.end_line()
                        )
                    };
                    let additional = self.attribution.evidence().len().saturating_sub(1);
                    if additional == 0 {
                        format!("direct: {location}")
                    } else {
                        format!("direct: {location} (+{additional} more)")
                    }
                },
            ),
        }
    }
}

impl PlanListContext {
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
}

impl UnsupportedChangeKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Drift => "drift",
            Self::Read => "read",
            Self::Move => "move",
            Self::Import => "import",
            Self::UnknownAction => "unknown action",
            Self::UnsupportedActions => "unsupported actions",
            Self::Deferred => "deferred",
            Self::ActionInvocation => "action invocation",
            Self::DeferredActionInvocation => "deferred action invocation",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::attribution::AnalysisIssue;
    use super::super::review::{ReviewComparison, ReviewComparisonBasis, ReviewComparisonStatus};
    use super::*;

    fn state() -> PlanListState {
        let change = ResourceChange {
            address: "aws_instance.api".to_owned(),
            mode: super::super::plan::ResourceMode::Managed,
            actions: vec![super::super::plan::PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: None,
            after: None,
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
        };
        let attribution =
            super::super::attribution::attribute_changes(std::slice::from_ref(&change), &[], &[])
                .pop()
                .expect("one change should produce one attribution");
        PlanListState::from_plan(
            Plan {
                changes: vec![change],
                summary: PlanSummary {
                    updates: 1,
                    ..PlanSummary::default()
                },
                unsupported_changes: Vec::new(),
            },
            vec![attribution],
            "working tree vs HEAD",
        )
        .expect("matching attribution should build a list")
    }

    #[test]
    fn selection_stays_within_available_items() {
        let mut state = state();

        state.apply(PlanListAction::SelectPrevious);
        assert_eq!(state.selected(), 0);

        state.apply(PlanListAction::SelectNext);
        assert_eq!(state.selected(), 0);
    }

    #[test]
    fn mismatched_attribution_is_rejected() {
        let change = ResourceChange {
            address: "aws_instance.api".to_owned(),
            mode: super::super::plan::ResourceMode::Managed,
            actions: vec![super::super::plan::PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: None,
            after: None,
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
        };
        let error = PlanListState::from_plan(
            Plan {
                changes: vec![change],
                summary: PlanSummary::default(),
                unsupported_changes: Vec::new(),
            },
            Vec::new(),
            "working tree vs HEAD",
        )
        .expect_err("an attribution without a change must not be hidden");

        assert_eq!(
            error,
            PlanListError::AttributionCountMismatch {
                changes: 1,
                attributions: 0,
            }
        );
    }

    #[test]
    fn review_list_preserves_incomplete_comparison_and_analysis_reasons() {
        let source = state();
        let review = PlanReview::new(
            PathBuf::from("infra"),
            "default".to_owned(),
            Plan {
                changes: source
                    .items
                    .iter()
                    .map(|item| item.change.clone())
                    .collect(),
                summary: source.summary,
                unsupported_changes: Vec::new(),
            },
            Vec::new(),
            source
                .items
                .iter()
                .map(|item| item.attribution.clone())
                .collect(),
            ReviewComparison::new(
                ReviewComparisonBasis::WorkingTreeVsHead,
                None,
                None,
                None,
                None,
                ReviewComparisonStatus::Incomplete("comparison unavailable".to_owned()),
            ),
            vec![AnalysisIssue::git("analysis unavailable")],
        )
        .with_git("feature/review".to_owned());

        let list = PlanListState::from_review(&review).expect("review data should build a list");

        assert_eq!(
            list.analysis_issues(),
            &[
                "comparison unavailable".to_owned(),
                "analysis unavailable".to_owned()
            ]
        );
    }
}
