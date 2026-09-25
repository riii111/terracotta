use std::fmt::{Debug, Formatter};
use std::time::Instant;

use super::{
    copy::{self, CopyEffect, CopyFeedback, CopyResult, CopyTarget},
    execution::{
        ApplyStatus, ExecutionAction, ExecutionEvent, ExecutionStage, ExecutionState,
        SuccessfulTarget,
    },
    review::{PlanMetadata, PlanReview, PlanReviewMessage},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionOutcome {
    Reviewed(PlanMetadata),
    NoChanges,
    ApplyCanceled,
    Applied {
        status: ApplyStatus,
        summary_line: Option<String>,
    },
    Failed(ExecutionStage),
    Interrupted(ExecutionStage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionState {
    Execution(Box<ExecutionState>),
    Review(Box<ReviewSessionState>),
    Overview(Box<OverviewSessionState>),
    ApplyConfirmation(Box<ApplyConfirmationState>),
    Apply(Box<ExecutionState>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewSessionState {
    review: PlanReview,
    from_overview: bool,
    restored_search_query: Option<String>,
    copy_feedback: CopyFeedback,
}

impl ReviewSessionState {
    #[must_use]
    pub(crate) fn new(review: PlanReview) -> Self {
        Self {
            review,
            from_overview: false,
            restored_search_query: None,
            copy_feedback: CopyFeedback::default(),
        }
    }

    #[must_use]
    pub(crate) fn new_from_overview(review: PlanReview) -> Self {
        Self {
            review,
            from_overview: true,
            restored_search_query: None,
            copy_feedback: CopyFeedback::default(),
        }
    }

    #[must_use]
    pub(crate) fn new_from_overview_with_search(
        review: PlanReview,
        restored_search_query: String,
    ) -> Self {
        Self {
            review,
            from_overview: true,
            restored_search_query: Some(restored_search_query),
            copy_feedback: CopyFeedback::default(),
        }
    }

    #[must_use]
    pub(crate) const fn review(&self) -> &PlanReview {
        &self.review
    }

    #[must_use]
    pub(crate) const fn is_from_overview(&self) -> bool {
        self.from_overview
    }

    #[must_use]
    pub(crate) const fn copy_feedback(&self) -> &CopyFeedback {
        &self.copy_feedback
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OverviewSessionState {
    review: PlanReview,
    copy_feedback: CopyFeedback,
}

impl OverviewSessionState {
    #[must_use]
    pub(crate) fn new(review: PlanReview) -> Self {
        Self {
            review,
            copy_feedback: CopyFeedback::default(),
        }
    }

    #[must_use]
    pub(crate) const fn review(&self) -> &PlanReview {
        &self.review
    }

    #[must_use]
    pub(crate) const fn copy_feedback(&self) -> &CopyFeedback {
        &self.copy_feedback
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApplyConfirmationState {
    review: PlanReview,
    from_overview: bool,
    restored_search_query: Option<String>,
}

impl ApplyConfirmationState {
    #[must_use]
    pub(crate) const fn new(review: PlanReview) -> Self {
        Self {
            review,
            from_overview: false,
            restored_search_query: None,
        }
    }

    fn from_review(review: &ReviewSessionState) -> Self {
        Self {
            from_overview: review.from_overview,
            restored_search_query: review.restored_search_query.clone(),
            ..Self::new(review.review.clone())
        }
    }

    fn into_review(self) -> ReviewSessionState {
        ReviewSessionState {
            review: self.review,
            from_overview: self.from_overview,
            restored_search_query: self.restored_search_query,
            copy_feedback: CopyFeedback::default(),
        }
    }

    #[must_use]
    pub(crate) const fn review(&self) -> &PlanReview {
        &self.review
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "review completion carries the complete plan into the session"
)]
pub(crate) enum Action {
    Execution(ExecutionAction),
    WorkerEvent(ExecutionEvent),
    ApplyWorkerEvent(ExecutionEvent),
    ReviewCompleted(PlanReview),
    ReviewFailed {
        message: String,
        interrupted: bool,
    },
    ReviewSearchChanged(String),
    OpenOverview,
    OpenReviewFromOverview {
        address: Option<String>,
    },
    ReturnToOverview,
    OpenApplyConfirmation,
    ConfirmApply(String),
    CancelApply,
    ApplyCompleted {
        status: ApplyStatus,
        summary_line: Option<String>,
    },
    ApplyFailed {
        message: String,
    },
    WorkerDisconnected,
    Copy(CopyTarget),
    CopyCompleted {
        target: CopyTarget,
        result: CopyResult,
    },
    Quit,
}

pub(crate) enum Effect {
    CancelExecution,
    StartApply,
    PersistHistory(Vec<SuccessfulTarget>),
    WriteClipboard(CopyEffect),
    Finish(SessionOutcome),
}

impl Debug for Effect {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CancelExecution => formatter.write_str("CancelExecution"),
            Self::StartApply => formatter.write_str("StartApply"),
            Self::PersistHistory(_) => formatter.write_str("PersistHistory(<redacted>)"),
            Self::WriteClipboard(effect) => formatter
                .debug_tuple("WriteClipboard")
                .field(&effect.target())
                .field(&"<redacted>")
                .finish(),
            Self::Finish(outcome) => formatter.debug_tuple("Finish").field(outcome).finish(),
        }
    }
}

impl SessionState {
    #[must_use]
    pub(crate) fn new(execution: ExecutionState) -> Self {
        Self::Execution(Box::new(execution))
    }

    #[must_use]
    pub(crate) const fn copy_feedback(&self) -> Option<&CopyFeedback> {
        match self {
            Self::Execution(state) | Self::Apply(state) => Some(state.copy_feedback()),
            Self::Review(state) => Some(state.copy_feedback()),
            Self::Overview(state) => Some(state.copy_feedback()),
            Self::ApplyConfirmation(_) => None,
        }
    }

    pub(crate) fn copy_feedback_mut(&mut self) -> Option<&mut CopyFeedback> {
        match self {
            Self::Execution(state) | Self::Apply(state) => Some(state.copy_feedback_mut()),
            Self::Review(state) => Some(&mut state.copy_feedback),
            Self::Overview(state) => Some(&mut state.copy_feedback),
            Self::ApplyConfirmation(_) => None,
        }
    }

    #[must_use]
    pub(crate) const fn execution(&self) -> Option<&ExecutionState> {
        match self {
            Self::Execution(state) => Some(state),
            Self::Review(_) | Self::Overview(_) | Self::ApplyConfirmation(_) | Self::Apply(_) => {
                None
            }
        }
    }

    #[must_use]
    pub(crate) const fn review(&self) -> Option<&ReviewSessionState> {
        match self {
            Self::Execution(_)
            | Self::Overview(_)
            | Self::ApplyConfirmation(_)
            | Self::Apply(_) => None,
            Self::Review(state) => Some(state),
        }
    }

    #[must_use]
    pub(crate) const fn overview(&self) -> Option<&OverviewSessionState> {
        match self {
            Self::Overview(state) => Some(state),
            Self::Execution(_) | Self::Review(_) | Self::ApplyConfirmation(_) | Self::Apply(_) => {
                None
            }
        }
    }

    #[must_use]
    pub(crate) const fn apply_confirmation(&self) -> Option<&ApplyConfirmationState> {
        match self {
            Self::ApplyConfirmation(state) => Some(state),
            Self::Execution(_) | Self::Review(_) | Self::Overview(_) | Self::Apply(_) => None,
        }
    }

    #[must_use]
    pub(crate) const fn apply(&self) -> Option<&ExecutionState> {
        match self {
            Self::Apply(state) => Some(state),
            Self::Execution(_)
            | Self::Review(_)
            | Self::Overview(_)
            | Self::ApplyConfirmation(_) => None,
        }
    }

    #[must_use]
    pub(crate) fn from_message(message: PlanReviewMessage) -> Action {
        match message {
            PlanReviewMessage::Event(event) => Action::WorkerEvent(event),
            PlanReviewMessage::ApplyEvent(event) => Action::ApplyWorkerEvent(event),
            PlanReviewMessage::Completed(review) => Action::ReviewCompleted(review),
            PlanReviewMessage::Failed {
                message,
                interrupted,
            } => Action::ReviewFailed {
                message,
                interrupted,
            },
            PlanReviewMessage::ApplyCompleted {
                status,
                summary_line,
            } => Action::ApplyCompleted {
                status,
                summary_line,
            },
            PlanReviewMessage::ApplyFailed { message } => Action::ApplyFailed { message },
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the session reducer keeps all user-visible state transitions together"
)]
pub(crate) fn update(state: &mut SessionState, action: Action, now: Instant) -> Option<Effect> {
    match action {
        Action::Execution(ExecutionAction::RequestCancellation) => {
            let execution = match state {
                SessionState::Execution(execution) | SessionState::Apply(execution) => execution,
                SessionState::Review(_)
                | SessionState::Overview(_)
                | SessionState::ApplyConfirmation(_) => return None,
            };
            if execution.cancellation_requested() {
                return None;
            }
            execution.apply(ExecutionAction::RequestCancellation);
            Some(Effect::CancelExecution)
        }
        Action::WorkerEvent(event) => {
            if let SessionState::Execution(execution) = state {
                execution.record(event);
            }
            None
        }
        Action::ApplyWorkerEvent(event) => {
            if let SessionState::Apply(execution) = state {
                execution.record(event);
            }
            None
        }
        Action::ReviewCompleted(review) => {
            let SessionState::Execution(execution) = state else {
                return None;
            };
            if execution.cancellation_requested() {
                return Some(Effect::Finish(SessionOutcome::Interrupted(
                    execution.stage(),
                )));
            }
            if review.apply_entry() && !review.metadata().applyable() {
                return Some(Effect::Finish(SessionOutcome::NoChanges));
            }
            *state = SessionState::Review(Box::new(ReviewSessionState::new(review)));
            None
        }
        Action::ReviewFailed {
            message,
            interrupted,
        } => {
            let SessionState::Execution(execution) = state else {
                return None;
            };
            if interrupted || execution.cancellation_requested() {
                return Some(Effect::Finish(SessionOutcome::Interrupted(
                    execution.stage(),
                )));
            }
            execution.fail(message, now);
            None
        }
        Action::ReviewSearchChanged(query) => {
            if let SessionState::Review(review) = state {
                review.review.set_search_query(query);
            }
            None
        }
        Action::OpenOverview => {
            let SessionState::Review(review) = state else {
                return None;
            };
            let mut overview = review.review.clone();
            if let Some(query) = &review.restored_search_query {
                overview.set_search_query(query.clone());
            }
            *state = SessionState::Overview(Box::new(OverviewSessionState::new(overview)));
            None
        }
        Action::OpenReviewFromOverview { address: _ } => {
            let SessionState::Overview(overview) = state else {
                return None;
            };
            let restored_search_query = overview.review.search_query().to_owned();
            let mut review = overview.review.clone();
            review.set_search_query(String::new());
            *state = SessionState::Review(Box::new(
                ReviewSessionState::new_from_overview_with_search(review, restored_search_query),
            ));
            None
        }
        Action::ReturnToOverview => {
            let SessionState::Review(review) = state else {
                return None;
            };
            if !review.is_from_overview() {
                return None;
            }
            let mut restored = review.review.clone();
            if let Some(query) = &review.restored_search_query {
                restored.set_search_query(query.clone());
            }
            *state = SessionState::Overview(Box::new(OverviewSessionState::new(restored)));
            None
        }
        Action::OpenApplyConfirmation => {
            let SessionState::Review(review) = state else {
                return None;
            };
            if review.review.apply_allowed() && review.review.metadata().applyable() {
                *state = SessionState::ApplyConfirmation(Box::new(
                    ApplyConfirmationState::from_review(review),
                ));
            }
            None
        }
        Action::ConfirmApply(input) => {
            let SessionState::ApplyConfirmation(confirmation) = state else {
                return None;
            };
            if input != confirmation.review.confirmation_input() {
                return None;
            }
            *state = SessionState::Apply(Box::new(ExecutionState::applying_with_previous(
                now,
                confirmation.review.context().clone(),
                confirmation.review.metadata().apply_targets().to_vec(),
                confirmation.review.metadata().sensitive_values().to_vec(),
                confirmation.review.previous_durations(),
            )));
            Some(Effect::StartApply)
        }
        Action::CancelApply => {
            let SessionState::ApplyConfirmation(confirmation) = state else {
                return None;
            };
            let review = confirmation.as_ref().clone().into_review();
            *state = SessionState::Review(Box::new(review));
            None
        }
        Action::ApplyCompleted {
            status,
            summary_line,
        } => {
            let SessionState::Apply(execution) = state else {
                return None;
            };
            execution.finish_apply(status, summary_line, None, now);
            Some(Effect::PersistHistory(execution.successful_history()))
        }
        Action::ApplyFailed { message } => {
            let SessionState::Apply(execution) = state else {
                return None;
            };
            execution.finish_apply(ApplyStatus::Failed, None, Some(message), now);
            Some(Effect::PersistHistory(execution.successful_history()))
        }
        Action::WorkerDisconnected => match state {
            SessionState::Execution(execution) if execution.cancellation_requested() => Some(
                Effect::Finish(SessionOutcome::Interrupted(execution.stage())),
            ),
            SessionState::Execution(execution) if execution.stage() == ExecutionStage::Failed => {
                None
            }
            SessionState::Execution(execution) => {
                Some(Effect::Finish(SessionOutcome::Failed(execution.stage())))
            }
            SessionState::Apply(execution) if execution.result().is_none() => {
                execution.finish_apply(
                    ApplyStatus::Failed,
                    None,
                    Some("Apply worker disconnected.".to_owned()),
                    now,
                );
                Some(Effect::PersistHistory(execution.successful_history()))
            }
            SessionState::Review(_)
            | SessionState::Overview(_)
            | SessionState::ApplyConfirmation(_)
            | SessionState::Apply(_) => None,
        },
        Action::Copy(target) => match state {
            SessionState::Execution(execution) | SessionState::Apply(execution) => {
                execution.copy_effect(target)
            }
            SessionState::Review(review) if target == CopyTarget::Plan => {
                Some(copy::plan_effect(&review.review))
            }
            SessionState::Overview(overview) if target == CopyTarget::Plan => {
                Some(copy::plan_effect(&overview.review))
            }
            SessionState::Review(_)
            | SessionState::Overview(_)
            | SessionState::ApplyConfirmation(_) => None,
        }
        .map(Effect::WriteClipboard),
        Action::CopyCompleted { target, result } => {
            let flash = match state {
                SessionState::Execution(_) | SessionState::Apply(_) => {
                    target == CopyTarget::Execution
                }
                SessionState::Review(_) | SessionState::Overview(_) => target == CopyTarget::Plan,
                SessionState::ApplyConfirmation(_) => false,
            };
            if let Some(feedback) = state.copy_feedback_mut() {
                feedback.record(target, result, now, flash);
            }
            None
        }
        Action::Quit => match state {
            SessionState::Execution(execution) if execution.stage() == ExecutionStage::Failed => {
                Some(Effect::Finish(SessionOutcome::Failed(
                    execution.result().map_or(
                        ExecutionStage::Failed,
                        super::execution::ExecutionResult::phase,
                    ),
                )))
            }
            SessionState::Review(review) if review.review.apply_entry() => {
                Some(Effect::Finish(SessionOutcome::ApplyCanceled))
            }
            SessionState::Review(review) => Some(Effect::Finish(SessionOutcome::Reviewed(
                review.review.metadata().clone(),
            ))),
            SessionState::Execution(_) => None,
            SessionState::Overview(overview) if overview.review.apply_entry() => {
                Some(Effect::Finish(SessionOutcome::ApplyCanceled))
            }
            SessionState::Overview(overview) => Some(Effect::Finish(SessionOutcome::Reviewed(
                overview.review.metadata().clone(),
            ))),
            SessionState::ApplyConfirmation(confirmation) if confirmation.review.apply_entry() => {
                Some(Effect::Finish(SessionOutcome::ApplyCanceled))
            }
            SessionState::ApplyConfirmation(confirmation) => Some(Effect::Finish(
                SessionOutcome::Reviewed(confirmation.review.metadata().clone()),
            )),
            SessionState::Apply(execution) => execution.result().map(|result| {
                Effect::Finish(SessionOutcome::Applied {
                    status: match execution.stage() {
                        ExecutionStage::ApplySucceeded => ApplyStatus::Succeeded,
                        ExecutionStage::ApplyInterrupted => ApplyStatus::Interrupted,
                        _ => ApplyStatus::Failed,
                    },
                    summary_line: result.summary_line().map(str::to_owned),
                })
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::super::copy::CopyNotice;
    use super::super::execution::{ExecutionContext, ExecutionTargetSpec};
    use super::super::plan::PlanAction;
    use super::super::review::{
        PlanBlock, PlanBlockKind,
        test_support::{plan_document, plan_document_with_blocks},
    };
    use super::*;

    fn review() -> PlanReview {
        PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document("No changes.\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
            Vec::new(),
        )
    }

    fn applyable_review() -> PlanReview {
        PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document("Terraform will perform actions.\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 1, 0, true),
            Vec::new(),
        )
    }

    #[test]
    fn overview_round_trip_clears_only_the_temporary_raw_filter() {
        let now = Instant::now();
        let mut review = applyable_review();
        review.set_search_query("api".to_owned());
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(review), now);

        assert!(update(&mut state, Action::OpenOverview, now).is_none());
        let SessionState::Overview(overview) = &state else {
            panic!("overview should be visible");
        };
        assert_eq!(overview.review().search_query(), "api");

        assert!(
            update(
                &mut state,
                Action::OpenReviewFromOverview {
                    address: Some("terraform_data.api".to_owned()),
                },
                now,
            )
            .is_none()
        );
        let SessionState::Review(review) = &state else {
            panic!("raw review should be visible");
        };
        assert!(review.is_from_overview());
        assert!(review.review().search_query().is_empty());

        assert!(update(&mut state, Action::OpenOverview, now).is_none());
        assert_eq!(
            state
                .overview()
                .expect("overview should return")
                .review()
                .search_query(),
            "api"
        );

        assert!(
            update(
                &mut state,
                Action::OpenReviewFromOverview {
                    address: Some("terraform_data.api".to_owned()),
                },
                now,
            )
            .is_none()
        );
        assert!(update(&mut state, Action::ReturnToOverview, now).is_none());
        assert_eq!(
            state
                .overview()
                .expect("overview should be restored")
                .review()
                .search_query(),
            "api"
        );
    }

    #[test]
    fn cancellation_is_requested_once_and_late_completion_stays_interrupted() {
        let now = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));

        assert!(matches!(
            update(
                &mut state,
                Action::Execution(ExecutionAction::RequestCancellation),
                now
            ),
            Some(Effect::CancelExecution)
        ));
        assert!(
            update(
                &mut state,
                Action::Execution(ExecutionAction::RequestCancellation),
                now
            )
            .is_none()
        );
        assert!(matches!(
            update(&mut state, Action::ReviewCompleted(review()), now),
            Some(Effect::Finish(SessionOutcome::Interrupted(_)))
        ));
    }

    #[test]
    fn filtered_review_copies_the_full_document_and_quits_without_apply() {
        let now = Instant::now();
        let source = "Terraform will perform actions.\n\n".to_owned()
            + "  # terraform_data.api will be created\n"
            + "  + resource \"terraform_data\" \"api\" {\n"
            + "      input = \"api\"\n"
            + "    }\n\n"
            + "Plan: 1 to add, 0 to change, 0 to destroy.\n";
        let mut filtered = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document_with_blocks(
                source.clone(),
                vec![
                    PlanBlock::new(0..2, PlanBlockKind::Common),
                    PlanBlock::with_addresses(
                        2..6,
                        PlanBlockKind::Resource,
                        vec!["terraform_data.api".to_owned()],
                    ),
                    PlanBlock::new(6..9, PlanBlockKind::Common),
                ],
            ),
            PlanMetadata::new(Vec::new(), Vec::new(), 1, 0, 0, false),
            Vec::new(),
        );
        filtered.set_search_query("not-present".to_owned());
        let visible = filtered.document().filter(filtered.search_query());
        assert_eq!(visible.matching_resources(), 0);
        assert!(
            visible
                .lines_with_indices()
                .all(|(_, line)| !line.contains("terraform_data.api"))
        );

        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(filtered), now);

        let Some(Effect::WriteClipboard(effect)) =
            update(&mut state, Action::Copy(CopyTarget::Plan), now)
        else {
            panic!("plan copy should be available");
        };
        assert_eq!(effect.text(), source);
        assert!(matches!(
            update(&mut state, Action::Quit, now),
            Some(Effect::Finish(SessionOutcome::Reviewed(_)))
        ));
        assert!(state.review().is_some());
    }

    #[test]
    fn review_copy_flash_expires_without_clearing_the_notice() {
        let now = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(review()), now);
        update(
            &mut state,
            Action::CopyCompleted {
                target: CopyTarget::Plan,
                result: CopyResult::Written,
            },
            now,
        );

        let SessionState::Review(review) = &state else {
            panic!("review should be visible");
        };
        assert!(
            review
                .copy_feedback()
                .flash_active(now + Duration::from_millis(100))
        );
        assert!(review.copy_feedback().notice().is_some());

        let SessionState::Review(review) = &mut state else {
            panic!("review should be visible");
        };
        review
            .copy_feedback
            .clear_expired(now + Duration::from_millis(200));
        assert!(
            !review
                .copy_feedback()
                .flash_active(now + Duration::from_millis(200))
        );
        assert!(review.copy_feedback().notice().is_some());
    }

    #[test]
    fn copy_notice_replacement_resets_the_success_and_failure_deadlines() {
        let started_at = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            started_at,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(review()), started_at);
        update(
            &mut state,
            Action::CopyCompleted {
                target: CopyTarget::Plan,
                result: CopyResult::Written,
            },
            started_at,
        );
        update(
            &mut state,
            Action::CopyCompleted {
                target: CopyTarget::Plan,
                result: CopyResult::Failed,
            },
            started_at + Duration::from_secs(1),
        );

        let SessionState::Review(review) = &state else {
            panic!("review should remain visible");
        };
        assert_eq!(
            review
                .copy_feedback()
                .notice_at(started_at + Duration::from_millis(3_999)),
            Some(CopyNotice::Failed)
        );
        assert_eq!(
            review
                .copy_feedback()
                .notice_at(started_at + Duration::from_secs(6)),
            None
        );
    }

    #[test]
    fn apply_requires_confirmation_and_cancel_preserves_the_review() {
        let now = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(applyable_review()), now);
        assert!(update(&mut state, Action::OpenApplyConfirmation, now).is_none());
        assert!(state.apply_confirmation().is_some());

        assert!(update(&mut state, Action::CancelApply, now).is_none());
        let SessionState::Review(review) = &state else {
            panic!("cancel should restore review");
        };
        assert_eq!(
            review.review().document().text(),
            "Terraform will perform actions.\n"
        );
    }

    #[test]
    fn plan_entry_opens_apply_from_the_overview_detail_and_cancel_returns_there() {
        let now = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        let review = applyable_review();
        assert!(!review.apply_entry());
        update(&mut state, Action::ReviewCompleted(review), now);
        update(&mut state, Action::OpenOverview, now);
        update(
            &mut state,
            Action::OpenReviewFromOverview { address: None },
            now,
        );

        assert!(update(&mut state, Action::OpenApplyConfirmation, now).is_none());
        assert!(state.apply_confirmation().is_some());
        assert!(update(&mut state, Action::CancelApply, now).is_none());
        assert!(
            state
                .review()
                .is_some_and(ReviewSessionState::is_from_overview)
        );
        assert!(update(&mut state, Action::ReturnToOverview, now).is_none());
        assert!(state.overview().is_some());
    }

    #[test]
    fn quitting_a_plan_entry_confirmation_reports_the_review_without_apply() {
        let now = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(applyable_review()), now);
        update(&mut state, Action::OpenApplyConfirmation, now);

        assert!(matches!(
            update(&mut state, Action::Quit, now),
            Some(Effect::Finish(SessionOutcome::Reviewed(_)))
        ));

        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(
            &mut state,
            Action::ReviewCompleted(applyable_review().with_apply_entry(true)),
            now,
        );
        update(&mut state, Action::OpenApplyConfirmation, now);
        assert!(matches!(
            update(&mut state, Action::Quit, now),
            Some(Effect::Finish(SessionOutcome::ApplyCanceled))
        ));
    }

    #[test]
    fn apply_confirmation_starts_once_and_completion_can_quit_with_status() {
        let now = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(applyable_review()), now);
        update(&mut state, Action::OpenApplyConfirmation, now);

        assert!(matches!(
            update(&mut state, Action::ConfirmApply("yes".to_owned()), now),
            Some(Effect::StartApply)
        ));
        assert!(update(&mut state, Action::ConfirmApply("yes".to_owned()), now).is_none());
        update(
            &mut state,
            Action::ApplyCompleted {
                status: ApplyStatus::Succeeded,
                summary_line: Some("Apply complete! Resources: 1 added.".to_owned()),
            },
            now,
        );

        assert_eq!(
            state.apply().map(ExecutionState::stage),
            Some(ExecutionStage::ApplySucceeded)
        );
        assert!(matches!(
            update(&mut state, Action::Quit, now),
            Some(Effect::Finish(SessionOutcome::Applied {
                status: ApplyStatus::Succeeded,
                ..
            }))
        ));
    }

    #[test]
    fn apply_confirmation_uses_the_named_target_for_destructive_changes() {
        let now = Instant::now();
        let review = PlanReview::new(
            PathBuf::from("/repo/prod"),
            "default".to_owned(),
            plan_document("Terraform will perform actions.\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 1, true),
            Vec::new(),
        );
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/repo/prod"),
        ));
        update(&mut state, Action::ReviewCompleted(review), now);
        update(&mut state, Action::OpenApplyConfirmation, now);

        assert!(update(&mut state, Action::ConfirmApply("yes".to_owned()), now).is_none());
        assert!(state.apply_confirmation().is_some());
        assert!(matches!(
            update(&mut state, Action::ConfirmApply("prod".to_owned()), now),
            Some(Effect::StartApply)
        ));
    }

    #[test]
    fn filtered_review_applies_the_complete_plan() {
        let now = Instant::now();
        let targets = vec![
            ExecutionTargetSpec {
                address: "terraform_data.api".to_owned(),
                actions: vec![PlanAction::Update],
            },
            ExecutionTargetSpec {
                address: "terraform_data.worker".to_owned(),
                actions: vec![PlanAction::Create],
            },
        ];
        let mut filtered = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document("Terraform will perform actions.\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 1, 1, 0, true).with_apply_targets(targets),
            Vec::new(),
        )
        .with_apply_entry(true);
        filtered.set_search_query("not-present".to_owned());
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(filtered), now);
        assert!(update(&mut state, Action::OpenApplyConfirmation, now).is_none());
        assert!(state.apply_confirmation().is_some());
        assert_eq!(
            state
                .apply_confirmation()
                .expect("apply confirmation should retain the review")
                .review()
                .search_query(),
            "not-present"
        );

        assert!(matches!(
            update(&mut state, Action::ConfirmApply("yes".to_owned()), now),
            Some(Effect::StartApply)
        ));
        let apply = state
            .apply()
            .expect("the full plan should enter apply state");
        let targets = apply.progress().targets();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].address(), "terraform_data.api");
        assert_eq!(targets[0].actions(), &[PlanAction::Update]);
        assert_eq!(targets[1].address(), "terraform_data.worker");
        assert_eq!(targets[1].actions(), &[PlanAction::Create]);
    }

    #[test]
    fn apply_cancellation_requests_the_worker_to_stop() {
        let now = Instant::now();
        let mut state = SessionState::Apply(Box::new(ExecutionState::applying(
            now,
            ExecutionContext::loading("/project"),
        )));

        assert!(matches!(
            update(
                &mut state,
                Action::Execution(ExecutionAction::RequestCancellation),
                now,
            ),
            Some(Effect::CancelExecution)
        ));
        assert!(state.apply().is_some_and(ExecutionState::is_cancelling));
    }
}
