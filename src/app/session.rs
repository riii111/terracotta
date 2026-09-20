use std::fmt::{Debug, Formatter};
use std::time::Instant;

use super::{
    copy::{self, CopyEffect, CopyNotice, CopyResult, CopyTarget},
    execution::{
        ApplyStatus, ExecutionAction, ExecutionContext, ExecutionEvent, ExecutionStage,
        ExecutionState,
    },
    review::{PlanMetadata, PlanReview, PlanReviewMessage},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionOutcome {
    Reviewed(PlanMetadata),
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
    ApplyConfirmation(Box<ApplyConfirmationState>),
    Apply(Box<ExecutionState>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewSessionState {
    review: PlanReview,
    copy_notice: Option<CopyNotice>,
    copy_flash_until: Option<Instant>,
}

impl ReviewSessionState {
    #[must_use]
    pub(crate) const fn new(review: PlanReview) -> Self {
        Self {
            review,
            copy_notice: None,
            copy_flash_until: None,
        }
    }

    #[must_use]
    pub(crate) const fn review(&self) -> &PlanReview {
        &self.review
    }

    #[must_use]
    pub(crate) const fn copy_notice(&self) -> Option<CopyNotice> {
        self.copy_notice
    }

    #[must_use]
    pub(crate) fn copy_flash_active(&self, now: Instant) -> bool {
        self.copy_flash_until.is_some_and(|until| now < until)
    }

    #[must_use]
    pub(crate) const fn copy_flash_pending(&self) -> bool {
        self.copy_flash_until.is_some()
    }

    pub(crate) const fn clear_copy_flash(&mut self) {
        self.copy_flash_until = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApplyConfirmationState {
    review: PlanReview,
}

impl ApplyConfirmationState {
    #[must_use]
    pub(crate) const fn new(review: PlanReview) -> Self {
        Self { review }
    }

    #[must_use]
    pub(crate) const fn review(&self) -> &PlanReview {
        &self.review
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    OpenApplyConfirmation,
    ConfirmApply,
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
    WriteClipboard(CopyEffect),
    Finish(SessionOutcome),
}

impl Debug for Effect {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CancelExecution => formatter.write_str("CancelExecution"),
            Self::StartApply => formatter.write_str("StartApply"),
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
    pub(crate) const fn execution(&self) -> Option<&ExecutionState> {
        match self {
            Self::Execution(state) => Some(state),
            Self::Review(_) | Self::ApplyConfirmation(_) | Self::Apply(_) => None,
        }
    }

    #[must_use]
    pub(crate) const fn review(&self) -> Option<&ReviewSessionState> {
        match self {
            Self::Execution(_) | Self::ApplyConfirmation(_) | Self::Apply(_) => None,
            Self::Review(state) => Some(state),
        }
    }

    #[must_use]
    pub(crate) const fn apply_confirmation(&self) -> Option<&ApplyConfirmationState> {
        match self {
            Self::ApplyConfirmation(state) => Some(state),
            Self::Execution(_) | Self::Review(_) | Self::Apply(_) => None,
        }
    }

    #[must_use]
    pub(crate) const fn apply(&self) -> Option<&ExecutionState> {
        match self {
            Self::Apply(state) => Some(state),
            Self::Execution(_) | Self::Review(_) | Self::ApplyConfirmation(_) => None,
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

#[allow(
    clippy::too_many_lines,
    reason = "the session reducer keeps all user-visible state transitions together"
)]
pub(crate) fn update(state: &mut SessionState, action: Action, now: Instant) -> Option<Effect> {
    match action {
        Action::Execution(ExecutionAction::RequestCancellation) => {
            let execution = match state {
                SessionState::Execution(execution) | SessionState::Apply(execution) => execution,
                SessionState::Review(_) | SessionState::ApplyConfirmation(_) => return None,
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
        Action::OpenApplyConfirmation => {
            let SessionState::Review(review) = state else {
                return None;
            };
            if review.review.metadata().applyable() {
                let review = review.review.clone();
                *state =
                    SessionState::ApplyConfirmation(Box::new(ApplyConfirmationState::new(review)));
            }
            None
        }
        Action::ConfirmApply => {
            let SessionState::ApplyConfirmation(confirmation) = state else {
                return None;
            };
            let review = confirmation.review.clone();
            let context = ExecutionContext::loading(review.root().display().to_string())
                .with_workspace(review.workspace());
            *state = SessionState::Apply(Box::new(ExecutionState::applying(now, context)));
            Some(Effect::StartApply)
        }
        Action::CancelApply => {
            let SessionState::ApplyConfirmation(confirmation) = state else {
                return None;
            };
            let review = confirmation.review.clone();
            *state = SessionState::Review(Box::new(ReviewSessionState::new(review)));
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
            None
        }
        Action::ApplyFailed { message } => {
            let SessionState::Apply(execution) = state else {
                return None;
            };
            execution.finish_apply(ApplyStatus::Failed, None, Some(message), now);
            None
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
                None
            }
            SessionState::Review(_)
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
            SessionState::Review(_) | SessionState::ApplyConfirmation(_) => None,
        }
        .map(Effect::WriteClipboard),
        Action::CopyCompleted { target, result } => {
            let notice = match result {
                CopyResult::Written => CopyNotice::Copied { target },
                CopyResult::Failed => CopyNotice::Failed,
            };
            match state {
                SessionState::Execution(execution) | SessionState::Apply(execution) => {
                    execution.set_copy_notice(notice, now);
                }
                SessionState::Review(review) => {
                    review.copy_notice = Some(notice);
                    review.copy_flash_until = (result == CopyResult::Written)
                        .then(|| now + std::time::Duration::from_millis(200));
                }
                SessionState::ApplyConfirmation(_) => {}
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
            SessionState::Execution(_) => None,
            SessionState::Review(review) => Some(Effect::Finish(SessionOutcome::Reviewed(
                review.review.metadata().clone(),
            ))),
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

    use super::super::review::test_support::plan_document;
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
    fn completed_review_copies_the_full_document_and_quits_without_apply() {
        let now = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(review()), now);

        let Some(Effect::WriteClipboard(effect)) =
            update(&mut state, Action::Copy(CopyTarget::Plan), now)
        else {
            panic!("plan copy should be available");
        };
        assert_eq!(effect.text(), "No changes.\n");
        assert!(matches!(
            update(&mut state, Action::Quit, now),
            Some(Effect::Finish(SessionOutcome::Reviewed(_)))
        ));
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
        assert!(review.copy_flash_active(now + std::time::Duration::from_millis(100)));
        assert!(review.copy_flash_pending());
        assert!(review.copy_notice().is_some());

        let SessionState::Review(review) = &mut state else {
            panic!("review should be visible");
        };
        review.clear_copy_flash();
        assert!(!review.copy_flash_pending());
        assert!(review.copy_notice().is_some());
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
    fn apply_confirmation_starts_once_and_completion_can_quit_with_status() {
        let now = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project"),
        ));
        update(&mut state, Action::ReviewCompleted(applyable_review()), now);
        update(&mut state, Action::OpenApplyConfirmation, now);

        assert!(matches!(
            update(&mut state, Action::ConfirmApply, now),
            Some(Effect::StartApply)
        ));
        assert!(update(&mut state, Action::ConfirmApply, now).is_none());
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
