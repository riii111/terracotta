use std::fmt::{Debug, Formatter};
use std::time::Instant;

use super::{
    copy::{CopyEffect, CopyNotice, CopyResult, CopyTarget},
    execution::{ExecutionAction, ExecutionEvent, ExecutionStage, ExecutionState},
    review::{
        DetailAction, PlanListAction, PlanListState, PlanReview, PlanReviewMessage,
        ResourceNavigation, ReviewDetailState,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionOutcome {
    Reviewed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionState {
    Execution(ExecutionState),
    Review(ReviewSessionState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewSessionState {
    list: PlanListState,
    detail: Option<ReviewDetailState>,
    copy_notice: Option<CopyNotice>,
}

impl ReviewSessionState {
    #[must_use]
    pub(crate) const fn list(&self) -> &PlanListState {
        &self.list
    }

    #[must_use]
    pub(crate) const fn detail(&self) -> Option<&ReviewDetailState> {
        self.detail.as_ref()
    }

    #[must_use]
    pub(crate) const fn copy_notice(&self) -> Option<CopyNotice> {
        self.copy_notice
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
    WorkerDisconnected,
    List(PlanListAction),
    OpenDetail,
    CloseDetail,
    Detail(DetailAction),
    Navigate(ResourceNavigation),
    Copy(CopyTarget),
    CopyCompleted {
        target: CopyTarget,
        resource_count: usize,
        result: CopyResult,
    },
    TimeUpdated,
    DetailAreaTooSmall,
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
                .field(&effect.resource_count())
                .field(&"<redacted>")
                .finish(),
            Self::Finish(outcome) => formatter.debug_tuple("Finish").field(outcome).finish(),
        }
    }
}

impl SessionState {
    #[must_use]
    pub(crate) const fn new(execution: ExecutionState) -> Self {
        Self::Execution(execution)
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

pub(crate) fn update(state: &mut SessionState, action: Action, now: Instant) -> Vec<Effect> {
    match action {
        Action::Execution(action) => update_execution_action(state, action),
        Action::WorkerEvent(event) => {
            if let SessionState::Execution(execution) = state {
                execution.record(event);
            }
            Vec::new()
        }
        Action::ReviewCompleted(review) => complete_review(state, &review),
        Action::ReviewFailed {
            message,
            interrupted,
        } => fail_review(state, message, interrupted, now),
        Action::WorkerDisconnected => worker_disconnected(state),
        Action::List(action) => {
            if let SessionState::Review(review) = state {
                review.list.apply(action);
            }
            Vec::new()
        }
        Action::OpenDetail => {
            if let SessionState::Review(review) = state {
                review.detail = ReviewDetailState::from_list(&review.list);
            }
            Vec::new()
        }
        Action::CloseDetail => {
            if let SessionState::Review(review) = state
                && let Some(detail) = review.detail.take()
            {
                review.list.select_resource(detail.index());
            }
            Vec::new()
        }
        Action::Detail(action) => {
            if let SessionState::Review(review) = state
                && let Some(detail) = review.detail.as_mut()
            {
                detail.apply(action, now);
            }
            Vec::new()
        }
        Action::Navigate(navigation) => {
            if let SessionState::Review(review) = state
                && let Some(detail) = review.detail.as_mut()
            {
                detail.navigate(navigation, &mut review.list);
            }
            Vec::new()
        }
        Action::Copy(target) => copy_effect(state, target)
            .map_or_else(Vec::new, |effect| vec![Effect::WriteClipboard(effect)]),
        Action::CopyCompleted {
            target,
            resource_count,
            result,
        } => {
            let notice = match result {
                CopyResult::Written => CopyNotice::Copied {
                    target,
                    resource_count,
                },
                CopyResult::Failed => CopyNotice::Failed,
            };
            match state {
                SessionState::Execution(execution) => execution.set_copy_notice(notice),
                SessionState::Review(review) => {
                    review.copy_notice = Some(notice);
                    review.list.set_copy_notice(notice);
                }
            }
            Vec::new()
        }
        Action::TimeUpdated => {
            if let SessionState::Review(review) = state
                && let Some(detail) = review.detail.as_mut()
            {
                detail.clear_expired(now);
            }
            Vec::new()
        }
        Action::DetailAreaTooSmall => {
            if let SessionState::Review(review) = state
                && let Some(detail) = review.detail.as_mut()
            {
                detail.mask();
            }
            Vec::new()
        }
        Action::Quit => match state {
            SessionState::Execution(execution) if execution.stage() == ExecutionStage::Failed => {
                vec![Effect::Finish(SessionOutcome::Failed)]
            }
            SessionState::Execution(_) => Vec::new(),
            SessionState::Review(_) => vec![Effect::Finish(SessionOutcome::Reviewed)],
        },
    }
}

fn update_execution_action(state: &mut SessionState, action: ExecutionAction) -> Vec<Effect> {
    let SessionState::Execution(execution) = state else {
        return Vec::new();
    };

    if matches!(action, ExecutionAction::RequestCancellation) {
        if execution.cancellation_requested() {
            return Vec::new();
        }
        execution.apply(action);
        return vec![Effect::CancelExecution];
    }

    execution.apply(action);
    Vec::new()
}

fn complete_review(state: &mut SessionState, review: &PlanReview) -> Vec<Effect> {
    let SessionState::Execution(execution) = state else {
        return Vec::new();
    };
    if execution.cancellation_requested() {
        return vec![Effect::Finish(SessionOutcome::Interrupted)];
    }

    PlanListState::from_review(review).map_or_else(
        |_| vec![Effect::Finish(SessionOutcome::Failed)],
        |list| {
            *state = SessionState::Review(ReviewSessionState {
                list,
                detail: None,
                copy_notice: None,
            });
            Vec::new()
        },
    )
}

fn fail_review(
    state: &mut SessionState,
    message: String,
    interrupted: bool,
    now: Instant,
) -> Vec<Effect> {
    let SessionState::Execution(execution) = state else {
        return Vec::new();
    };
    if interrupted || execution.cancellation_requested() {
        return vec![Effect::Finish(SessionOutcome::Interrupted)];
    }

    execution.fail(message, now);
    Vec::new()
}

fn worker_disconnected(state: &SessionState) -> Vec<Effect> {
    match state {
        SessionState::Execution(execution) if execution.cancellation_requested() => {
            vec![Effect::Finish(SessionOutcome::Interrupted)]
        }
        SessionState::Execution(execution) if execution.stage() == ExecutionStage::Failed => {
            Vec::new()
        }
        SessionState::Execution(_) => vec![Effect::Finish(SessionOutcome::Failed)],
        SessionState::Review(_) => Vec::new(),
    }
}

fn copy_effect(state: &SessionState, target: CopyTarget) -> Option<CopyEffect> {
    match state {
        SessionState::Execution(execution) => execution.copy_effect(target),
        SessionState::Review(review) => review.list.copy_effect(target),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::attribution::attribute_changes;
    use super::super::execution::{ExecutionEventKind, ProcessExitStatus, ProcessTermination};
    use super::super::plan::{
        Plan, PlanAction, PlanSummary, PlanValue, ResourceChange, ResourceChangeKind, ResourceMode,
    };
    use super::super::review::{ReviewComparison, ReviewComparisonBasis, ReviewComparisonStatus};
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    fn empty_review() -> PlanReview {
        PlanReview::new(
            PathBuf::from("/tmp/project"),
            "default".to_owned(),
            Plan {
                changes: Vec::new(),
                summary: super::super::plan::PlanSummary::default(),
                unsupported_changes: Vec::new(),
            },
            Vec::new(),
            Vec::new(),
            ReviewComparison::new(
                ReviewComparisonBasis::WorkingTreeVsHead,
                None,
                None,
                None,
                None,
                ReviewComparisonStatus::Complete,
            ),
            Vec::new(),
        )
    }

    fn review_with_resource() -> PlanReview {
        let change = ResourceChange {
            address: "aws_instance.api".to_owned(),
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: Some(PlanValue::String("before".to_owned())),
            after: Some(PlanValue::String("after".to_owned())),
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
        };
        let changes = vec![change];
        let attributions = attribute_changes(&changes, &[], &[]);
        PlanReview::new(
            PathBuf::from("/tmp/project"),
            "default".to_owned(),
            Plan {
                changes,
                summary: PlanSummary {
                    updates: 1,
                    ..PlanSummary::default()
                },
                unsupported_changes: Vec::new(),
            },
            Vec::new(),
            attributions,
            ReviewComparison::new(
                ReviewComparisonBasis::WorkingTreeVsHead,
                None,
                None,
                None,
                None,
                ReviewComparisonStatus::Complete,
            ),
            Vec::new(),
        )
    }

    #[test]
    fn cancellation_effect_is_emitted_once_and_late_completion_finishes_interrupted() {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));

        assert_eq!(
            update(
                &mut state,
                Action::Execution(ExecutionAction::RequestCancellation),
                started_at,
            )
            .len(),
            1
        );
        assert!(
            update(
                &mut state,
                Action::Execution(ExecutionAction::RequestCancellation),
                started_at,
            )
            .is_empty()
        );
        assert!(matches!(
            update(
                &mut state,
                Action::ReviewCompleted(empty_review()),
                started_at
            )
            .as_slice(),
            [Effect::Finish(SessionOutcome::Interrupted)]
        ));
    }

    #[test]
    fn worker_failure_preserves_failure_screen_and_disconnect_is_terminal_after_quit() {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));
        assert!(
            update(
                &mut state,
                Action::ReviewFailed {
                    message: "invalid configuration".to_owned(),
                    interrupted: false,
                },
                started_at,
            )
            .is_empty()
        );
        assert!(matches!(state, SessionState::Execution(_)));
        assert!(update(&mut state, Action::WorkerDisconnected, started_at).is_empty());
        assert!(matches!(
            update(&mut state, Action::Quit, started_at).as_slice(),
            [Effect::Finish(SessionOutcome::Failed)]
        ));
    }

    #[test]
    fn termination_event_marks_execution_failure_without_replacing_worker_state() {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));
        update(
            &mut state,
            Action::WorkerEvent(ExecutionEvent {
                received_at: started_at,
                kind: ExecutionEventKind::Terminated(ProcessTermination {
                    status: ProcessExitStatus::Exited(1),
                    interrupted: false,
                }),
            }),
            started_at,
        );
        assert_eq!(
            state.execution().expect("execution state").stage(),
            ExecutionStage::Failed
        );
    }

    #[test]
    fn copy_effect_uses_current_selection_and_completion_sets_notice() {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));
        update(
            &mut state,
            Action::ReviewCompleted(review_with_resource()),
            started_at,
        );

        let effects = update(&mut state, Action::Copy(CopyTarget::Resource), started_at);
        let Effect::WriteClipboard(effect) = effects.into_iter().next().expect("copy effect")
        else {
            panic!("resource copy should produce a clipboard effect");
        };
        assert_eq!(effect.target(), CopyTarget::Resource);
        assert_eq!(effect.resource_count(), 1);
        assert!(effect.text().contains("aws_instance.api"));

        update(
            &mut state,
            Action::CopyCompleted {
                target: CopyTarget::Resource,
                resource_count: 1,
                result: CopyResult::Written,
            },
            started_at,
        );
        assert_eq!(
            state.review().and_then(ReviewSessionState::copy_notice),
            Some(CopyNotice::Copied {
                target: CopyTarget::Resource,
                resource_count: 1,
            })
        );
        assert!(
            state
                .review()
                .and_then(|review| review.list().copy_notice())
                .is_some()
        );
    }
}
