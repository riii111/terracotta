use std::time::{Duration, Instant};

use super::copy;

mod context;
mod event;
mod progress;

pub(crate) use context::{ExecutionContext, ExecutionContextValue};
pub(crate) use event::{
    Diagnostic, DiagnosticPoint, DiagnosticPosition, DiagnosticSeverity, DiagnosticSource,
    EventStream, ExecutionEvent, ExecutionEventKind, ExecutionPhase, ExecutionSummary,
    ProcessExitStatus, ProcessTermination, ResourceEvent, ResourceEventKind,
};
pub(crate) use progress::ExecutionProgress;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionStage {
    Planning,
    Reading,
    Matching,
    Failed,
}

impl ExecutionStage {
    #[must_use]
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Planning => "Planning",
            Self::Reading => "Reading",
            Self::Matching => "Matching",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionAction {
    RequestCancellation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionState {
    stage: ExecutionStage,
    context: ExecutionContext,
    started_at: Instant,
    finished_at: Option<Instant>,
    progress: ExecutionProgress,
    cancellation_requested: bool,
    failure_message: Option<String>,
    copy_notice: Option<copy::CopyNotice>,
}

impl ExecutionState {
    #[must_use]
    pub(crate) fn with_context(started_at: Instant, context: ExecutionContext) -> Self {
        Self {
            stage: ExecutionStage::Planning,
            context,
            started_at,
            finished_at: None,
            progress: ExecutionProgress::default(),
            cancellation_requested: false,
            failure_message: None,
            copy_notice: None,
        }
    }

    pub(crate) const fn apply(&mut self, action: ExecutionAction) {
        match action {
            ExecutionAction::RequestCancellation => self.cancellation_requested = true,
        }
    }

    pub(crate) fn record(&mut self, event: ExecutionEvent) {
        match &event.kind {
            ExecutionEventKind::Phase(ExecutionPhase::Reading) => {
                self.stage = ExecutionStage::Reading;
            }
            ExecutionEventKind::Phase(ExecutionPhase::Matching) => {
                self.stage = ExecutionStage::Matching;
            }
            ExecutionEventKind::RepositoryRoot(repository_root) => {
                self.context = self
                    .context
                    .clone()
                    .with_repository_root(repository_root.clone());
            }
            ExecutionEventKind::Workspace(workspace) => {
                self.context = self.context.clone().with_workspace(workspace.clone());
            }
            ExecutionEventKind::Git(git) => {
                self.context = self.context.clone().with_git(git.clone());
            }
            ExecutionEventKind::Terminated(termination) => {
                self.finished_at.get_or_insert(event.received_at);
                if !termination.interrupted
                    && !matches!(termination.status, ProcessExitStatus::Exited(0))
                {
                    self.stage = ExecutionStage::Failed;
                }
            }
            _ => {}
        }
        self.progress.record(event);
    }

    pub(crate) fn fail(&mut self, message: String, received_at: Instant) {
        self.stage = ExecutionStage::Failed;
        self.finished_at = Some(received_at);
        self.failure_message = Some(message.clone());
        self.record(ExecutionEvent {
            received_at,
            kind: ExecutionEventKind::Diagnostic(Diagnostic {
                severity: DiagnosticSeverity::Error,
                summary: message,
                detail: None,
                position: None,
                source: DiagnosticSource::Terraform,
            }),
        });
    }

    #[must_use]
    pub(crate) fn copy_effect(&self, target: copy::CopyTarget) -> Option<copy::CopyEffect> {
        let text = match target {
            copy::CopyTarget::Diagnostic => copy::failed_diagnostic_text(
                self.failure_message.as_deref(),
                self.progress.diagnostics(),
            ),
            copy::CopyTarget::Result => copy::failed_text(
                self.context(),
                self.failure_message.as_deref(),
                self.progress.diagnostics(),
            ),
            copy::CopyTarget::Resource | copy::CopyTarget::Plan => return None,
        };
        Some(copy::CopyEffect::new(target, 0, text))
    }

    #[must_use]
    pub(crate) const fn copy_notice(&self) -> Option<copy::CopyNotice> {
        self.copy_notice
    }

    pub(crate) const fn set_copy_notice(&mut self, notice: copy::CopyNotice) {
        self.copy_notice = Some(notice);
    }

    #[must_use]
    pub(crate) const fn stage(&self) -> ExecutionStage {
        self.stage
    }

    #[must_use]
    pub(crate) const fn context(&self) -> &ExecutionContext {
        &self.context
    }

    #[must_use]
    pub(crate) const fn progress(&self) -> &ExecutionProgress {
        &self.progress
    }

    pub(crate) fn take_review_diagnostics(&mut self) -> Vec<Diagnostic> {
        self.progress.take_review_diagnostics()
    }

    #[must_use]
    pub(crate) fn elapsed_at(&self, now: Instant) -> Duration {
        self.finished_at
            .unwrap_or(now)
            .saturating_duration_since(self.started_at)
    }

    #[must_use]
    pub(crate) fn waiting_at(&self, now: Instant) -> Duration {
        let last_event = self.progress.last_event_at().unwrap_or(self.started_at);
        now.saturating_duration_since(last_event)
    }

    #[must_use]
    pub(crate) const fn is_cancelling(&self) -> bool {
        self.cancellation_requested && self.progress.termination().is_none()
    }

    #[must_use]
    pub(crate) const fn cancellation_requested(&self) -> bool {
        self.cancellation_requested
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    impl ExecutionState {
        pub(crate) fn new(started_at: Instant) -> Self {
            Self::with_context(
                started_at,
                ExecutionContext::loading("loading...", "loading..."),
            )
        }
    }

    fn event(received_at: Instant, kind: ExecutionEventKind) -> ExecutionEvent {
        ExecutionEvent { received_at, kind }
    }

    #[test]
    fn reports_elapsed_and_waiting_time_from_injected_timestamps() {
        let started_at = Instant::now();
        let event_at = started_at + Duration::from_secs(2);
        let mut state = ExecutionState::new(started_at);

        assert_eq!(
            state.elapsed_at(started_at + Duration::from_secs(5)),
            Duration::from_secs(5)
        );
        assert_eq!(
            state.waiting_at(started_at + Duration::from_secs(5)),
            Duration::from_secs(5)
        );

        state.record(event(
            event_at,
            ExecutionEventKind::Informational {
                event_type: "log".to_owned(),
                message: None,
            },
        ));

        assert_eq!(
            state.waiting_at(started_at + Duration::from_secs(5)),
            Duration::from_secs(3)
        );
    }

    #[test]
    fn failed_elapsed_stays_at_the_termination_time() {
        let started_at = Instant::now();
        let finished_at = started_at + Duration::from_secs(2);
        let mut state = ExecutionState::new(started_at);

        state.record(event(
            finished_at,
            ExecutionEventKind::Terminated(ProcessTermination {
                status: ProcessExitStatus::Exited(1),
                interrupted: false,
            }),
        ));

        assert_eq!(
            state.elapsed_at(started_at + Duration::from_secs(10)),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn cancellation_stays_visible_until_a_termination_event_arrives() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);

        state.apply(ExecutionAction::RequestCancellation);
        assert!(state.is_cancelling());

        state.record(event(
            started_at + Duration::from_secs(1),
            ExecutionEventKind::Terminated(ProcessTermination {
                status: ProcessExitStatus::Exited(130),
                interrupted: true,
            }),
        ));
        assert!(!state.is_cancelling());
        assert_eq!(state.stage(), ExecutionStage::Planning);
    }

    #[test]
    fn failed_copy_effects_separate_diagnostic_from_full_result() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);
        state.fail("Terraform failed".to_owned(), started_at);

        let diagnostic = state
            .copy_effect(copy::CopyTarget::Diagnostic)
            .expect("diagnostic copy should be available");
        let result = state
            .copy_effect(copy::CopyTarget::Result)
            .expect("result copy should be available");

        assert_eq!(diagnostic.target(), copy::CopyTarget::Diagnostic);
        assert!(diagnostic.text().contains("Terraform failed"));
        assert_eq!(result.target(), copy::CopyTarget::Result);
        assert!(result.text().contains("Review result is unavailable."));
    }

    #[test]
    fn nonzero_termination_enters_failed_stage() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);
        let termination_at = started_at + Duration::from_secs(1);
        let termination = ProcessTermination {
            status: ProcessExitStatus::Exited(1),
            interrupted: false,
        };

        state.record(event(
            termination_at,
            ExecutionEventKind::Terminated(termination),
        ));

        assert_eq!(state.stage(), ExecutionStage::Failed);
        assert_eq!(state.progress().termination(), Some(termination));
        assert_eq!(state.progress().last_event_at(), Some(termination_at));
    }

    #[test]
    fn review_events_update_workspace_and_execution_phase() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);

        state.record(event(
            started_at,
            ExecutionEventKind::Workspace("default".to_owned()),
        ));
        assert_eq!(
            state.context().workspace(),
            &ExecutionContextValue::Known("default".to_owned())
        );

        state.record(event(
            started_at,
            ExecutionEventKind::Phase(ExecutionPhase::Reading),
        ));
        assert_eq!(state.stage(), ExecutionStage::Reading);

        state.record(event(
            started_at,
            ExecutionEventKind::Phase(ExecutionPhase::Matching),
        ));
        assert_eq!(state.stage(), ExecutionStage::Matching);
    }

    #[test]
    fn repository_root_moves_from_loading_to_known_or_unavailable() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);

        assert_eq!(
            state.context.repository_root(),
            &ExecutionContextValue::Loading
        );

        state.record(event(
            started_at,
            ExecutionEventKind::RepositoryRoot(Some(PathBuf::from("/repo"))),
        ));
        assert_eq!(
            state.context.repository_root(),
            &ExecutionContextValue::Known("/repo".to_owned())
        );
        assert_eq!(
            state.context.repository_root_path(),
            Some(Path::new("/repo"))
        );

        state.record(event(started_at, ExecutionEventKind::RepositoryRoot(None)));
        assert_eq!(
            state.context.repository_root(),
            &ExecutionContextValue::Unavailable
        );
        assert_eq!(state.context.repository_root_path(), None);
    }
}
