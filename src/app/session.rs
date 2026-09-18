use std::fmt::{Debug, Formatter};
use std::time::Instant;

use super::{
    copy::{CopyEffect, CopyNotice, CopyResult, CopyTarget},
    execution::{ExecutionAction, ExecutionEvent, ExecutionStage, ExecutionState},
    review::{
        DetailAction, PlanListAction, PlanListState, PlanReview, PlanReviewMessage,
        ResourceNavigation, ReviewDetailState, ReviewDiagnosticsState,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionOutcome {
    Reviewed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "session states are kept as direct product-state aggregates"
)]
pub(crate) enum SessionState {
    Execution(ExecutionState),
    Review(ReviewSessionState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewSessionState {
    list: PlanListState,
    diagnostics: ReviewDiagnosticsState,
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
    pub(crate) const fn diagnostics(&self) -> &ReviewDiagnosticsState {
        &self.diagnostics
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
    OpenDiagnostics,
    CloseDiagnostics,
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
        Action::WorkerEvent(event) => record_worker_event(state, event),
        Action::ReviewCompleted(review) => complete_review(state, &review),
        Action::ReviewFailed {
            message,
            interrupted,
        } => fail_review(state, message, interrupted, now),
        Action::WorkerDisconnected => worker_disconnected(state),
        Action::List(action) => update_list(state, action),
        Action::OpenDiagnostics => open_diagnostics(state),
        Action::CloseDiagnostics => close_diagnostics(state),
        Action::OpenDetail => open_detail(state),
        Action::CloseDetail => close_detail(state),
        Action::Detail(action) => update_detail(state, action, now),
        Action::Navigate(navigation) => navigate_detail(state, navigation),
        Action::Copy(target) => copy_effect(state, target)
            .map_or_else(Vec::new, |effect| vec![Effect::WriteClipboard(effect)]),
        Action::CopyCompleted {
            target,
            resource_count,
            result,
        } => copy_completed(state, target, resource_count, result),
        Action::TimeUpdated => time_updated(state, now),
        Action::DetailAreaTooSmall => detail_area_too_small(state),
        Action::Quit => quit_effect(state),
    }
}

fn record_worker_event(state: &mut SessionState, event: ExecutionEvent) -> Vec<Effect> {
    if let SessionState::Execution(execution) = state {
        execution.record(event);
    }
    Vec::new()
}

fn update_list(state: &mut SessionState, action: PlanListAction) -> Vec<Effect> {
    if let SessionState::Review(review) = state {
        review.list.apply(action);
    }
    Vec::new()
}

const fn open_diagnostics(state: &mut SessionState) -> Vec<Effect> {
    if let SessionState::Review(review) = state
        && review.detail().is_none()
    {
        review.diagnostics.open();
    }
    Vec::new()
}

const fn close_diagnostics(state: &mut SessionState) -> Vec<Effect> {
    if let SessionState::Review(review) = state {
        review.diagnostics.close();
    }
    Vec::new()
}

fn open_detail(state: &mut SessionState) -> Vec<Effect> {
    if let SessionState::Review(review) = state
        && !review.diagnostics().is_open()
    {
        review.detail = ReviewDetailState::from_list(&review.list);
    }
    Vec::new()
}

fn close_detail(state: &mut SessionState) -> Vec<Effect> {
    if let SessionState::Review(review) = state
        && let Some(detail) = review.detail.take()
    {
        review.list.select_resource(detail.index());
    }
    Vec::new()
}

fn update_detail(state: &mut SessionState, action: DetailAction, now: Instant) -> Vec<Effect> {
    if let SessionState::Review(review) = state
        && let Some(detail) = review.detail.as_mut()
    {
        detail.apply(action, now);
    }
    Vec::new()
}

fn navigate_detail(state: &mut SessionState, navigation: ResourceNavigation) -> Vec<Effect> {
    if let SessionState::Review(review) = state
        && let Some(detail) = review.detail.as_mut()
    {
        detail.navigate(navigation, &mut review.list);
    }
    Vec::new()
}

const fn copy_completed(
    state: &mut SessionState,
    target: CopyTarget,
    resource_count: usize,
    result: CopyResult,
) -> Vec<Effect> {
    let notice = match result {
        CopyResult::Written => CopyNotice::Copied {
            target,
            resource_count,
        },
        CopyResult::Failed => CopyNotice::Failed,
    };
    match state {
        SessionState::Execution(execution) => execution.set_copy_notice(notice),
        SessionState::Review(review) => review.copy_notice = Some(notice),
    }
    Vec::new()
}

fn time_updated(state: &mut SessionState, now: Instant) -> Vec<Effect> {
    if let SessionState::Review(review) = state
        && let Some(detail) = review.detail.as_mut()
    {
        detail.clear_expired(now);
    }
    Vec::new()
}

fn detail_area_too_small(state: &mut SessionState) -> Vec<Effect> {
    if let SessionState::Review(review) = state
        && let Some(detail) = review.detail.as_mut()
    {
        detail.mask();
    }
    Vec::new()
}

fn quit_effect(state: &SessionState) -> Vec<Effect> {
    match state {
        SessionState::Execution(execution) if execution.stage() == ExecutionStage::Failed => {
            vec![Effect::Finish(SessionOutcome::Failed)]
        }
        SessionState::Execution(_) => Vec::new(),
        SessionState::Review(_) => vec![Effect::Finish(SessionOutcome::Reviewed)],
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
    let SessionState::Execution(execution) = &mut *state else {
        return Vec::new();
    };
    if execution.cancellation_requested() {
        return vec![Effect::Finish(SessionOutcome::Interrupted)];
    }

    let Ok(list) = PlanListState::from_review(review) else {
        return vec![Effect::Finish(SessionOutcome::Failed)];
    };
    let diagnostics = execution.take_review_diagnostics();
    *state = SessionState::Review(ReviewSessionState {
        list,
        diagnostics: ReviewDiagnosticsState::new(diagnostics),
        detail: None,
        copy_notice: None,
    });
    Vec::new()
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
    use std::{collections::BTreeMap, path::PathBuf};

    use super::super::attribution::attribute_changes;
    use super::super::execution::{
        Diagnostic, DiagnosticSeverity, DiagnosticSource, ExecutionEventKind, ProcessExitStatus,
        ProcessTermination,
    };
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
        review_with_changes(vec![sensitive_change(
            "aws_instance.api",
            "before-secret",
            "after-secret",
        )])
    }

    fn review_with_resources() -> PlanReview {
        review_with_changes(vec![
            sensitive_change("aws_instance.api", "before-secret", "after-secret"),
            sensitive_change("aws_instance.worker", "before-worker", "after-worker"),
        ])
    }

    fn sensitive_change(address: &str, before: &str, after: &str) -> ResourceChange {
        ResourceChange {
            address: address.to_owned(),
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: Some(PlanValue::Object(BTreeMap::from([
                ("password".to_owned(), PlanValue::String(before.to_owned())),
                ("public".to_owned(), PlanValue::String("old".to_owned())),
            ]))),
            after: Some(PlanValue::Object(BTreeMap::from([
                ("password".to_owned(), PlanValue::String(after.to_owned())),
                ("public".to_owned(), PlanValue::String("new".to_owned())),
            ]))),
            before_sensitive: Some(PlanValue::Object(BTreeMap::from([(
                "password".to_owned(),
                PlanValue::Bool(true),
            )]))),
            after_sensitive: Some(PlanValue::Object(BTreeMap::from([(
                "password".to_owned(),
                PlanValue::Bool(true),
            )]))),
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
        }
    }

    fn review_with_changes(changes: Vec<ResourceChange>) -> PlanReview {
        let attributions = attribute_changes(&changes, &[], &[]);
        PlanReview::new(
            PathBuf::from("/tmp/project"),
            "default".to_owned(),
            Plan {
                changes,
                summary: PlanSummary {
                    updates: attributions.len(),
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

    fn opened_detail_state() -> (SessionState, Instant) {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));
        update(
            &mut state,
            Action::ReviewCompleted(review_with_resources()),
            started_at,
        );
        update(&mut state, Action::OpenDetail, started_at);
        (state, started_at)
    }

    fn assert_revealed_at(state: &SessionState, at: Instant) {
        assert!(
            state
                .review()
                .and_then(ReviewSessionState::detail)
                .is_some_and(|detail| detail.is_revealed_at(at))
        );
    }

    fn diagnostic_event(severity: DiagnosticSeverity, summary: &str) -> Action {
        Action::WorkerEvent(ExecutionEvent {
            received_at: now(),
            kind: ExecutionEventKind::Diagnostic(Diagnostic {
                severity,
                summary: summary.to_owned(),
                detail: None,
                position: None,
                source: DiagnosticSource::Terraform,
                raw: None,
            }),
        })
    }

    #[test]
    fn successful_review_moves_review_diagnostics_in_order_and_keeps_plan_decisions_separate() {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));
        for (severity, summary) in [
            (DiagnosticSeverity::Info, "info"),
            (DiagnosticSeverity::Warning, "warning"),
            (DiagnosticSeverity::Error, "error"),
            (DiagnosticSeverity::Unknown, "unknown"),
        ] {
            update(&mut state, diagnostic_event(severity, summary), started_at);
        }

        update(
            &mut state,
            Action::ReviewCompleted(review_with_resource()),
            started_at,
        );

        let review = state.review().expect("successful review should be visible");
        assert_eq!(review.diagnostics().count(), 3);
        assert_eq!(
            review
                .diagnostics()
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.summary.as_str())
                .collect::<Vec<_>>(),
            vec!["warning", "error", "unknown"]
        );
        assert_eq!(review.list().needs_review_count(), 1);

        update(&mut state, Action::OpenDiagnostics, started_at);
        assert!(
            state
                .review()
                .is_some_and(|review| review.diagnostics().is_open())
        );
        update(&mut state, Action::CloseDiagnostics, started_at);
        assert!(
            state
                .review()
                .is_some_and(|review| !review.diagnostics().is_open())
        );
    }

    #[test]
    fn info_only_success_has_no_diagnostics_panel() {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));
        update(
            &mut state,
            diagnostic_event(DiagnosticSeverity::Info, "informational"),
            started_at,
        );
        update(
            &mut state,
            Action::ReviewCompleted(empty_review()),
            started_at,
        );

        assert!(
            state
                .review()
                .is_some_and(|review| review.diagnostics().count() == 0)
        );
        update(&mut state, Action::OpenDiagnostics, started_at);
        assert!(
            state
                .review()
                .is_some_and(|review| !review.diagnostics().is_open())
        );
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
        assert!(effect.text().contains("<sensitive>"));
        assert!(!effect.text().contains("before-secret"));
        assert!(!effect.text().contains("after-secret"));
        assert_eq!(
            state.review().and_then(ReviewSessionState::copy_notice),
            None
        );

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

        update(&mut state, Action::OpenDetail, started_at);
        let review = state.review().expect("review state");
        assert!(review.detail().is_some());
        assert_eq!(
            review.copy_notice(),
            Some(CopyNotice::Copied {
                target: CopyTarget::Resource,
                resource_count: 1,
            })
        );

        update(&mut state, Action::CloseDetail, started_at);
        let review = state.review().expect("review state");
        assert!(review.detail().is_none());
        assert_eq!(
            review.copy_notice(),
            Some(CopyNotice::Copied {
                target: CopyTarget::Resource,
                resource_count: 1,
            })
        );
    }

    #[test]
    fn plan_copy_ignores_search_scope_when_copy_action_runs() {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));
        update(
            &mut state,
            Action::ReviewCompleted(review_with_resource()),
            started_at,
        );
        update(
            &mut state,
            Action::List(PlanListAction::BeginSearch),
            started_at,
        );
        update(
            &mut state,
            Action::List(PlanListAction::SetSearch("not-present".to_owned())),
            started_at,
        );
        update(
            &mut state,
            Action::List(PlanListAction::ConfirmSearch),
            started_at,
        );

        let effects = update(&mut state, Action::Copy(CopyTarget::Plan), started_at);
        let [Effect::WriteClipboard(effect)] = effects.as_slice() else {
            panic!("plan copy should produce a clipboard effect");
        };
        assert_eq!(effect.resource_count(), 1);
        assert!(effect.text().contains("aws_instance.api"));
    }

    #[test]
    fn failed_copy_preserves_detail_and_selection_while_notifying_failure() {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));
        update(
            &mut state,
            Action::ReviewCompleted(review_with_resource()),
            started_at,
        );
        update(&mut state, Action::OpenDetail, started_at);
        update(&mut state, Action::Detail(DetailAction::Reveal), started_at);
        assert!(
            state
                .review()
                .and_then(ReviewSessionState::detail)
                .is_some_and(|detail| detail.is_revealed_at(started_at))
        );

        let selected_before = state.review().expect("review state").list().selected();
        let detail_before = state.review().and_then(ReviewSessionState::detail).cloned();

        let effects = update(&mut state, Action::Copy(CopyTarget::Resource), started_at);
        let [Effect::WriteClipboard(effect)] = effects.as_slice() else {
            panic!("resource copy should produce a clipboard effect");
        };
        assert!(!effect.text().contains("before-secret"));
        assert!(!effect.text().contains("after-secret"));
        assert_eq!(
            state.review().and_then(ReviewSessionState::copy_notice),
            None
        );

        update(
            &mut state,
            Action::CopyCompleted {
                target: CopyTarget::Resource,
                resource_count: 1,
                result: CopyResult::Failed,
            },
            started_at,
        );

        let review = state.review().expect("review state");
        assert_eq!(review.list().selected(), selected_before);
        assert_eq!(review.detail(), detail_before.as_ref());
        assert_eq!(review.copy_notice(), Some(CopyNotice::Failed));

        update(&mut state, Action::CloseDetail, started_at);
        let review = state.review().expect("review state");
        assert!(review.detail().is_none());
        assert_eq!(review.copy_notice(), Some(CopyNotice::Failed));

        update(&mut state, Action::OpenDetail, started_at);
        let review = state.review().expect("review state");
        assert!(review.detail().is_some());
        assert_eq!(review.copy_notice(), Some(CopyNotice::Failed));
    }

    #[test]
    fn detail_navigation_follows_filtered_order_stops_at_edges_and_reopens_cleanly() {
        let started_at = now();
        let mut state = SessionState::new(ExecutionState::new(started_at));
        update(
            &mut state,
            Action::ReviewCompleted(review_with_resources()),
            started_at,
        );
        update(
            &mut state,
            Action::List(PlanListAction::BeginSearch),
            started_at,
        );
        update(
            &mut state,
            Action::List(PlanListAction::SetSearch("aws_instance".to_owned())),
            started_at,
        );
        update(
            &mut state,
            Action::List(PlanListAction::ConfirmSearch),
            started_at,
        );
        update(&mut state, Action::OpenDetail, started_at);
        update(&mut state, Action::Detail(DetailAction::Reveal), started_at);

        update(
            &mut state,
            Action::Navigate(ResourceNavigation::Next),
            started_at,
        );
        let review = state.review().expect("review state");
        assert_eq!(review.list().selected(), Some(1));
        assert_eq!(review.detail().map(ReviewDetailState::index), Some(1));
        assert_eq!(review.detail().and_then(ReviewDetailState::reveal), None);

        update(
            &mut state,
            Action::Navigate(ResourceNavigation::Next),
            started_at,
        );
        assert_eq!(
            state.review().and_then(|review| review.list().selected()),
            Some(1)
        );

        update(
            &mut state,
            Action::Navigate(ResourceNavigation::Previous),
            started_at,
        );
        assert_eq!(
            state.review().and_then(|review| review.list().selected()),
            Some(0)
        );

        update(
            &mut state,
            Action::Detail(DetailAction::Reveal),
            started_at + std::time::Duration::from_secs(1),
        );
        assert!(
            state
                .review()
                .and_then(ReviewSessionState::detail)
                .is_some_and(
                    |detail| detail.is_revealed_at(started_at + std::time::Duration::from_secs(1))
                )
        );

        update(
            &mut state,
            Action::CloseDetail,
            started_at + std::time::Duration::from_secs(2),
        );
        assert!(
            state
                .review()
                .is_some_and(|review| review.detail().is_none())
        );
        update(
            &mut state,
            Action::OpenDetail,
            started_at + std::time::Duration::from_secs(3),
        );
        assert_eq!(
            state
                .review()
                .and_then(|review| review.detail())
                .map(ReviewDetailState::selected),
            Some(0)
        );
        assert_eq!(
            state
                .review()
                .and_then(ReviewSessionState::detail)
                .and_then(ReviewDetailState::reveal),
            None
        );
    }

    #[test]
    fn detail_reveal_repress_masks_the_current_value() {
        let (mut state, started_at) = opened_detail_state();
        update(&mut state, Action::Detail(DetailAction::Reveal), started_at);
        assert_revealed_at(&state, started_at);

        let pressed_again = started_at + std::time::Duration::from_secs(1);
        update(
            &mut state,
            Action::Detail(DetailAction::Reveal),
            pressed_again,
        );
        assert!(
            state
                .review()
                .and_then(ReviewSessionState::detail)
                .is_some_and(|detail| detail.reveal().is_none())
        );
    }

    #[test]
    fn detail_selection_change_masks_an_active_reveal() {
        let (mut state, started_at) = opened_detail_state();
        update(&mut state, Action::Detail(DetailAction::Reveal), started_at);
        assert_revealed_at(&state, started_at);

        let changed_at = started_at + std::time::Duration::from_secs(1);
        update(
            &mut state,
            Action::Detail(DetailAction::SelectNext),
            changed_at,
        );
        assert!(
            state
                .review()
                .and_then(ReviewSessionState::detail)
                .is_some_and(|detail| detail.reveal().is_none())
        );
    }

    #[test]
    fn detail_time_update_masks_an_expired_reveal() {
        let (mut state, started_at) = opened_detail_state();
        let revealed_at = started_at + std::time::Duration::from_secs(1);
        update(
            &mut state,
            Action::Detail(DetailAction::Reveal),
            revealed_at,
        );
        assert_revealed_at(&state, revealed_at);

        update(
            &mut state,
            Action::TimeUpdated,
            started_at + std::time::Duration::from_secs(11),
        );
        assert!(
            state
                .review()
                .and_then(ReviewSessionState::detail)
                .is_some_and(|detail| detail.reveal().is_none())
        );
    }

    #[test]
    fn detail_area_too_small_masks_an_active_reveal() {
        let (mut state, started_at) = opened_detail_state();
        let revealed_at = started_at + std::time::Duration::from_secs(1);
        update(
            &mut state,
            Action::Detail(DetailAction::Reveal),
            revealed_at,
        );
        assert_revealed_at(&state, revealed_at);

        update(
            &mut state,
            Action::DetailAreaTooSmall,
            started_at + std::time::Duration::from_secs(2),
        );
        assert!(
            state
                .review()
                .and_then(ReviewSessionState::detail)
                .is_some_and(|detail| detail.reveal().is_none())
        );
    }
}
