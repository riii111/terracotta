use std::fmt::{Display, Formatter};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::app::attribution::{ResourceAttribution, SourceFileAnalysis};
use crate::app::copy::{self, CopyEffect, CopyTarget};
use crate::app::plan::{Plan, PlanSummary, UnsupportedChangeKind};
use crate::app::review::PlanReview;

use super::PlanListItem;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanListAction {
    SelectPrevious,
    SelectNext,
    SelectResource(usize),
    ToggleFilter,
    BeginSearch,
    SetSearch(String),
    ConfirmSearch,
    CancelSearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanListFilter {
    All,
    NeedsReview,
}

impl PlanListFilter {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::NeedsReview => "Needs review",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanListState {
    context: Option<PlanListContext>,
    comparison: super::ReviewComparison,
    summary: PlanSummary,
    items: Vec<PlanListItem>,
    source_files: Vec<SourceFileAnalysis>,
    unsupported: Vec<UnsupportedChangeKind>,
    analysis_issues: Vec<String>,
    filter: PlanListFilter,
    search: String,
    search_backup: Option<SearchBackup>,
    selected: Option<usize>,
    plan_copy_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchBackup {
    filter: PlanListFilter,
    search: String,
    selected_address: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanListContext {
    root: PathBuf,
    repository_root: Option<PathBuf>,
    workspace: String,
    git: String,
}

impl PlanListState {
    pub(crate) fn from_review(review: PlanReview) -> Result<Self, PlanListError> {
        let plan_copy_text = copy::plan_text(&review);
        let comparison = review.comparison().clone();
        let comparison_message = comparison.status().message().map(str::to_owned);
        let PlanReview {
            root,
            repository_root,
            workspace,
            git,
            plan,
            source_files,
            attributions,
            comparison,
            analysis_issues,
        } = review;

        let mut state = Self::from_plan(plan, attributions, comparison.clone())?;

        if let Some(message) = comparison_message {
            state.analysis_issues.push(message);
        }
        for issue in analysis_issues {
            if !state
                .analysis_issues
                .iter()
                .any(|known| known == issue.message())
            {
                state.analysis_issues.push(issue.message().to_owned());
            }
        }
        state.source_files = source_files;
        for item in &mut state.items {
            item.set_resource_copy_text(copy::resource_text(
                item.change(),
                item.attribution(),
                &comparison,
            ));
        }
        state.plan_copy_text = Some(plan_copy_text);
        state.context = Some(PlanListContext {
            root,
            repository_root,
            workspace,
            git,
        });

        Ok(state)
    }

    pub(crate) fn from_plan(
        plan: Plan,
        attributions: Vec<ResourceAttribution>,
        comparison: super::ReviewComparison,
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

                Ok(PlanListItem::new(change, attribution))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let selected = (!items.is_empty()).then_some(0);
        Ok(Self {
            context: None,
            comparison,
            summary: plan.summary,
            items,
            source_files: Vec::new(),
            unsupported: plan
                .unsupported_changes
                .into_iter()
                .map(|change| change.kind)
                .collect(),
            analysis_issues: Vec::new(),
            filter: PlanListFilter::All,
            search: String::new(),
            search_backup: None,
            selected,
            plan_copy_text: None,
        })
    }

    pub(crate) fn apply(&mut self, action: PlanListAction) {
        match action {
            PlanListAction::SelectPrevious => {
                self.selected = self.selected.map(|selected| selected.saturating_sub(1));
            }
            PlanListAction::SelectNext => {
                if let Some(last) = self.visible_count().checked_sub(1) {
                    self.selected =
                        Some(self.selected.map_or(0, |selected| (selected + 1).min(last)));
                } else {
                    self.selected = None;
                }
            }
            PlanListAction::SelectResource(index) => {
                if index < self.visible_count() {
                    self.selected = Some(index);
                }
            }
            PlanListAction::ToggleFilter => self.toggle_filter(),
            PlanListAction::BeginSearch => self.begin_search(),
            PlanListAction::SetSearch(search) => self.set_search(search),
            PlanListAction::ConfirmSearch => self.confirm_search(),
            PlanListAction::CancelSearch => self.cancel_search(),
        }
    }

    pub(crate) fn select_resource(&mut self, index: usize) {
        self.apply(PlanListAction::SelectResource(index));
    }

    #[must_use]
    pub(crate) const fn comparison(&self) -> &super::ReviewComparison {
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

    pub(crate) fn visible_items(&self) -> impl Iterator<Item = &PlanListItem> {
        let search = self.search.to_lowercase();
        self.items
            .iter()
            .filter(move |item| self.includes(item, &search))
    }

    #[must_use]
    pub(crate) fn visible_count(&self) -> usize {
        self.visible_items().count()
    }

    #[must_use]
    pub(crate) const fn filter(&self) -> PlanListFilter {
        self.filter
    }

    #[must_use]
    pub(crate) fn search(&self) -> &str {
        &self.search
    }

    #[must_use]
    pub(crate) const fn searching(&self) -> bool {
        self.search_backup.is_some()
    }

    #[must_use]
    pub(crate) fn source_files(&self) -> &[SourceFileAnalysis] {
        &self.source_files
    }

    #[must_use]
    pub(crate) const fn selected(&self) -> Option<usize> {
        self.selected
    }

    #[must_use]
    pub(crate) fn selected_item(&self) -> Option<&PlanListItem> {
        self.visible_items().nth(self.selected?)
    }

    #[must_use]
    pub(crate) fn can_copy(&self, target: CopyTarget) -> bool {
        match target {
            CopyTarget::Resource => self.selected_item().is_some_and(PlanListItem::can_copy),
            CopyTarget::Plan => self.plan_copy_text.is_some(),
            CopyTarget::Diagnostic | CopyTarget::Result => false,
        }
    }

    #[must_use]
    pub(crate) fn copy_effect(&self, target: CopyTarget) -> Option<CopyEffect> {
        match target {
            CopyTarget::Resource => self.selected_item()?.copy_effect(target, self.items.len()),
            CopyTarget::Plan => Some(CopyEffect::new(
                target,
                self.items.len(),
                self.plan_copy_text.clone()?,
            )),
            CopyTarget::Diagnostic | CopyTarget::Result => None,
        }
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

    fn includes(&self, item: &PlanListItem, search: &str) -> bool {
        (matches!(self.filter, PlanListFilter::All) || item.needs_review())
            && (search.is_empty() || item.address().to_lowercase().contains(search))
    }

    fn toggle_filter(&mut self) {
        let selected_address = self.selected_item().map(|item| item.address().to_owned());
        self.filter = match self.filter {
            PlanListFilter::All => PlanListFilter::NeedsReview,
            PlanListFilter::NeedsReview => PlanListFilter::All,
        };
        self.select_visible_address_or_first(selected_address);
    }

    fn begin_search(&mut self) {
        if self.searching() {
            return;
        }

        self.search_backup = Some(SearchBackup {
            filter: self.filter,
            search: self.search.clone(),
            selected_address: self.selected_item().map(|item| item.address().to_owned()),
        });
    }

    fn set_search(&mut self, search: String) {
        let selected_address = self.selected_item().map(|item| item.address().to_owned());
        self.search = search;
        self.select_visible_address_or_first(selected_address);
    }

    fn confirm_search(&mut self) {
        self.search_backup = None;
    }

    fn cancel_search(&mut self) {
        let Some(backup) = self.search_backup.take() else {
            return;
        };

        self.filter = backup.filter;
        self.search = backup.search;
        self.selected = backup.selected_address.and_then(|address| {
            self.visible_items()
                .position(|item| item.address() == address)
        });
    }

    fn select_visible_address_or_first(&mut self, address: Option<String>) {
        self.selected = address
            .and_then(|address| {
                self.visible_items()
                    .position(|item| item.address() == address)
            })
            .or_else(|| (self.visible_count() > 0).then_some(0));
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
    use super::*;
    use crate::app::attribution::{
        AnalysisIssue, ResourceAddress, ResourceSourceLocation, SourceLineChange, SourceRange,
        SourceSide, attribute_changes, mark_analysis_incomplete,
    };
    use crate::app::plan::{PlanAction, ResourceChange, ResourceChangeKind, ResourceMode};
    use crate::app::review::{ReviewComparison, ReviewComparisonBasis, ReviewComparisonStatus};

    fn state() -> PlanListState {
        let change = ResourceChange {
            address: "aws_instance.api".to_owned(),
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: None,
            after: None,
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
        };
        let attribution = attribute_changes(std::slice::from_ref(&change), &[], &[])
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
            ReviewComparison::working_tree(),
        )
        .expect("matching attribution should build a list")
    }

    #[test]
    fn selection_stays_within_available_items() {
        let mut state = state();

        state.apply(PlanListAction::SelectPrevious);
        assert_eq!(state.selected(), Some(0));

        state.apply(PlanListAction::SelectNext);
        assert_eq!(state.selected(), Some(0));
    }

    fn filtered_state() -> PlanListState {
        let changes = [
            change("aws_instance.direct"),
            change("aws_instance.incomplete"),
            change("aws_instance.no_match"),
        ]
        .into_iter()
        .collect::<Vec<_>>();
        let source_files = vec![SourceFileAnalysis::new(
            "main.tf".into(),
            SourceSide::After,
            vec![
                ResourceSourceLocation::new(
                    ResourceAddress::new("aws_instance", "direct"),
                    "main.tf".into(),
                    SourceSide::After,
                    SourceRange::new(1, 4),
                ),
                ResourceSourceLocation::new(
                    ResourceAddress::new("aws_instance", "incomplete"),
                    "main.tf".into(),
                    SourceSide::After,
                    SourceRange::new(6, 9),
                ),
            ],
            Vec::new(),
        )];
        let changed_lines = vec![
            SourceLineChange::new("main.tf", SourceSide::After, SourceRange::new(2, 2)),
            SourceLineChange::new("main.tf", SourceSide::After, SourceRange::new(7, 7)),
        ];
        let mut attributions = attribute_changes(&changes, &source_files, &changed_lines);
        mark_analysis_incomplete(
            &mut attributions[1..2],
            &[AnalysisIssue::git("partial source")],
        );

        PlanListState::from_plan(
            Plan {
                changes,
                summary: PlanSummary {
                    updates: 3,
                    ..PlanSummary::default()
                },
                unsupported_changes: Vec::new(),
            },
            attributions,
            ReviewComparison::working_tree(),
        )
        .expect("filter fixture should build a list")
    }

    fn change(address: &str) -> ResourceChange {
        ResourceChange {
            address: address.to_owned(),
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: None,
            after: None,
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
        }
    }

    #[test]
    fn needs_review_filter_keeps_full_counts_and_direct_incomplete_item() {
        let mut state = filtered_state();
        state.apply(PlanListAction::SelectNext);

        state.apply(PlanListAction::ToggleFilter);

        assert_eq!(state.filter(), PlanListFilter::NeedsReview);
        assert_eq!(state.visible_count(), 2);
        assert_eq!(state.needs_review_count(), 2);
        assert_eq!(state.selected(), Some(0));
        assert_eq!(
            state.selected_item().map(PlanListItem::address),
            Some("aws_instance.incomplete")
        );

        state.apply(PlanListAction::ToggleFilter);

        assert_eq!(state.filter(), PlanListFilter::All);
        assert_eq!(state.visible_count(), 3);
        assert_eq!(state.selected(), Some(1));
    }

    #[test]
    fn needs_review_filter_moves_disappearing_selection_to_first_item() {
        let mut state = filtered_state();

        state.apply(PlanListAction::ToggleFilter);

        assert_eq!(state.selected(), Some(0));
        assert_eq!(
            state.selected_item().map(PlanListItem::address),
            Some("aws_instance.incomplete")
        );
    }

    #[test]
    fn empty_needs_review_filter_has_no_selection_and_restores_first_item() {
        let mut state = direct_only_state();

        state.apply(PlanListAction::ToggleFilter);

        assert_eq!(state.filter(), PlanListFilter::NeedsReview);
        assert_eq!(state.selected(), None);
        assert_eq!(state.selected_item(), None);

        state.apply(PlanListAction::ToggleFilter);

        assert_eq!(state.filter(), PlanListFilter::All);
        assert_eq!(state.selected(), Some(0));
        assert_eq!(
            state.selected_item().map(PlanListItem::address),
            Some("aws_instance.direct")
        );
    }

    #[test]
    fn resource_search_matches_case_insensitive_address_fragments() {
        let mut state = filtered_state();

        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch(
            "AWS_INSTANCE.INCOMPLETE".to_owned(),
        ));

        assert!(state.searching());
        assert_eq!(state.visible_count(), 1);
        assert_eq!(state.selected(), Some(0));
        assert_eq!(
            state.selected_item().map(PlanListItem::address),
            Some("aws_instance.incomplete")
        );

        state.apply(PlanListAction::ConfirmSearch);

        assert!(!state.searching());
        assert_eq!(state.search(), "AWS_INSTANCE.INCOMPLETE");
    }

    #[test]
    fn resource_search_uses_filter_scope_and_cancel_restores_conditions_and_selection() {
        let mut state = filtered_state();
        state.apply(PlanListAction::ToggleFilter);
        state.apply(PlanListAction::SelectNext);

        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("direct".to_owned()));

        assert_eq!(state.visible_count(), 0);
        assert_eq!(state.selected(), None);

        state.apply(PlanListAction::CancelSearch);

        assert_eq!(state.filter(), PlanListFilter::NeedsReview);
        assert_eq!(state.search(), "");
        assert_eq!(state.selected(), Some(1));
        assert_eq!(
            state.selected_item().map(PlanListItem::address),
            Some("aws_instance.no_match")
        );
    }

    #[test]
    fn empty_search_confirm_clears_query_and_restores_all_matches() {
        let mut state = filtered_state();

        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch("direct".to_owned()));
        state.apply(PlanListAction::ConfirmSearch);
        assert_eq!(state.visible_count(), 1);

        state.apply(PlanListAction::BeginSearch);
        state.apply(PlanListAction::SetSearch(String::new()));
        state.apply(PlanListAction::ConfirmSearch);

        assert_eq!(state.search(), "");
        assert_eq!(state.visible_count(), 3);
        assert_eq!(state.selected(), Some(0));
    }

    fn direct_only_state() -> PlanListState {
        let change = change("aws_instance.direct");
        let source_files = vec![SourceFileAnalysis::new(
            "main.tf".into(),
            SourceSide::After,
            vec![ResourceSourceLocation::new(
                ResourceAddress::new("aws_instance", "direct"),
                "main.tf".into(),
                SourceSide::After,
                SourceRange::new(1, 4),
            )],
            Vec::new(),
        )];
        let attributions = attribute_changes(
            std::slice::from_ref(&change),
            &source_files,
            &[SourceLineChange::new(
                "main.tf",
                SourceSide::After,
                SourceRange::new(2, 2),
            )],
        );

        PlanListState::from_plan(
            Plan {
                changes: vec![change],
                summary: PlanSummary {
                    updates: 1,
                    ..PlanSummary::default()
                },
                unsupported_changes: Vec::new(),
            },
            attributions,
            ReviewComparison::working_tree(),
        )
        .expect("direct-only fixture should build a list")
    }

    #[test]
    fn mismatched_attribution_is_rejected() {
        let change = ResourceChange {
            address: "aws_instance.api".to_owned(),
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
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
            ReviewComparison::working_tree(),
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
    fn mismatched_attribution_address_is_rejected() {
        let api_change = change("aws_instance.api");
        let wrong_attribution = attribute_changes(
            std::slice::from_ref(&change("aws_instance.other")),
            &[],
            &[],
        )
        .pop()
        .expect("one attribution should be created");
        let error = PlanListState::from_plan(
            Plan {
                changes: vec![api_change],
                summary: PlanSummary::default(),
                unsupported_changes: Vec::new(),
            },
            vec![wrong_attribution],
            ReviewComparison::working_tree(),
        )
        .expect_err("an attribution for another address must not be hidden");

        assert_eq!(
            error,
            PlanListError::AttributionAddressMismatch {
                index: 0,
                change: "aws_instance.api".to_owned(),
                attribution: "aws_instance.other".to_owned(),
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
                    .map(|item| item.change().clone())
                    .collect(),
                summary: source.summary,
                unsupported_changes: Vec::new(),
            },
            Vec::new(),
            source
                .items
                .iter()
                .map(|item| item.attribution().clone())
                .collect(),
            ReviewComparison::new(
                ReviewComparisonBasis::WorkingTreeVsHead,
                None,
                ReviewComparisonStatus::Incomplete("comparison unavailable".to_owned()),
            ),
            vec![AnalysisIssue::git("analysis unavailable")],
        )
        .with_git("feature/review".to_owned());
        let review = review.with_repository_root(Some(PathBuf::from("/repo")));

        let list = PlanListState::from_review(review).expect("review data should build a list");

        assert_eq!(
            list.analysis_issues(),
            &[
                "comparison unavailable".to_owned(),
                "analysis unavailable".to_owned()
            ]
        );
        assert_eq!(
            list.context()
                .and_then(|context| context.repository_root.as_deref()),
            Some(Path::new("/repo"))
        );
    }
}
