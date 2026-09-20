use std::fmt::{Debug, Formatter};
use std::time::Instant;

use super::{
    copy::{self, CopyEffect, CopyNotice, CopyResult, CopyTarget},
    execution::{ExecutionAction, ExecutionEvent, ExecutionStage, ExecutionState},
    review::{PlanMetadata, PlanReview, PlanReviewMessage},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionOutcome {
    Reviewed(PlanMetadata),
    Failed(ExecutionStage),
    Interrupted(ExecutionStage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionState {
    Execution(Box<ExecutionState>),
    Review(Box<ReviewSessionState>),
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    Execution(ExecutionAction),
    WorkerEvent(ExecutionEvent),
    ReviewCompleted(PlanReview),
    ReviewFailed {
        message: String,
        interrupted: bool,
    },
    ReviewSearchChanged(String),
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
    WriteClipboard(CopyEffect),
    Finish(SessionOutcome),
}

impl Debug for Effect {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CancelExecution => formatter.write_str("CancelExecution"),
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
            Self::Review(_) => None,
        }
    }

    #[must_use]
    pub(crate) const fn review(&self) -> Option<&ReviewSessionState> {
        match self {
            Self::Execution(_) => None,
            Self::Review(state) => Some(state),
        }
    }

    #[must_use]
    pub(crate) fn from_message(message: PlanReviewMessage) -> Action {
        match message {
            PlanReviewMessage::Event(event) => Action::WorkerEvent(event),
            PlanReviewMessage::Completed(review) => Action::ReviewCompleted(review),
            PlanReviewMessage::Failed {
                message,
                interrupted,
            } => Action::ReviewFailed {
                message,
                interrupted,
            },
        }
    }
}

pub(crate) fn update(state: &mut SessionState, action: Action, now: Instant) -> Option<Effect> {
    match action {
        Action::Execution(ExecutionAction::RequestCancellation) => {
            let SessionState::Execution(execution) = state else {
                return None;
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
            SessionState::Review(_) => None,
        },
        Action::Copy(target) => match state {
            SessionState::Execution(execution) => execution.copy_effect(target),
            SessionState::Review(review) if target == CopyTarget::Plan => {
                Some(copy::plan_effect(&review.review))
            }
            SessionState::Review(_) => None,
        }
        .map(Effect::WriteClipboard),
        Action::CopyCompleted { target, result } => {
            let notice = match result {
                CopyResult::Written => CopyNotice::Copied { target },
                CopyResult::Failed => CopyNotice::Failed,
            };
            match state {
                SessionState::Execution(execution) => execution.set_copy_notice(notice),
                SessionState::Review(review) => {
                    review.copy_notice = Some(notice);
                    review.copy_flash_until = (result == CopyResult::Written)
                        .then(|| now + std::time::Duration::from_millis(200));
                }
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
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::{execution::ExecutionContext, review::PlanDocument};
    use super::*;

    fn review() -> PlanReview {
        PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            PlanDocument::new("No changes.\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
            Vec::new(),
        )
    }

    #[test]
    fn cancellation_is_requested_once_and_late_completion_stays_interrupted() {
        let now = Instant::now();
        let mut state = SessionState::new(ExecutionState::with_context(
            now,
            ExecutionContext::loading("/project", "comparison unavailable"),
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
            ExecutionContext::loading("/project", "comparison unavailable"),
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
}
