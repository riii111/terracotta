use std::time::{Duration, Instant};

use super::progress::{
    Diagnostic, DiagnosticSource, ExecutionEvent, ExecutionEventKind, ExecutionPhase,
    ExecutionProgress, ProcessExitStatus,
};

const PAGE_SCROLL: u16 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionContextValue {
    Loading,
    Unavailable,
    Known(String),
}

impl ExecutionContextValue {
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Loading => "loading...",
            Self::Unavailable => "unavailable",
            Self::Known(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionContext {
    cwd: ExecutionContextValue,
    workspace: ExecutionContextValue,
    git: ExecutionContextValue,
    comparison: ExecutionContextValue,
}

impl ExecutionContext {
    #[must_use]
    pub(crate) const fn loading() -> Self {
        Self {
            cwd: ExecutionContextValue::Loading,
            workspace: ExecutionContextValue::Loading,
            git: ExecutionContextValue::Loading,
            comparison: ExecutionContextValue::Loading,
        }
    }

    #[must_use]
    pub(crate) const fn unavailable() -> Self {
        Self {
            cwd: ExecutionContextValue::Unavailable,
            workspace: ExecutionContextValue::Unavailable,
            git: ExecutionContextValue::Unavailable,
            comparison: ExecutionContextValue::Unavailable,
        }
    }

    #[must_use]
    pub(crate) fn known(
        cwd: impl Into<String>,
        workspace: impl Into<String>,
        git: impl Into<String>,
        comparison: impl Into<String>,
    ) -> Self {
        Self {
            cwd: ExecutionContextValue::Known(cwd.into()),
            workspace: ExecutionContextValue::Known(workspace.into()),
            git: ExecutionContextValue::Known(git.into()),
            comparison: ExecutionContextValue::Known(comparison.into()),
        }
    }

    pub(crate) fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = ExecutionContextValue::Known(workspace.into());
        self
    }

    #[must_use]
    pub(crate) const fn cwd(&self) -> &ExecutionContextValue {
        &self.cwd
    }

    #[must_use]
    pub(crate) const fn workspace(&self) -> &ExecutionContextValue {
        &self.workspace
    }

    #[must_use]
    pub(crate) const fn git(&self) -> &ExecutionContextValue {
        &self.git
    }

    #[must_use]
    pub(crate) const fn comparison(&self) -> &ExecutionContextValue {
        &self.comparison
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionStage {
    Planning,
    Reading,
    Matching,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionScroll {
    Up,
    Down,
    PageUp,
    PageDown,
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
    SetStage(ExecutionStage),
    End,
    RequestCancellation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionState {
    stage: ExecutionStage,
    context: ExecutionContext,
    started_at: Instant,
    progress: ExecutionProgress,
    scroll: u16,
    follow: bool,
    cancellation_requested: bool,
}

impl ExecutionState {
    #[must_use]
    pub(crate) fn new(started_at: Instant) -> Self {
        Self::with_context(started_at, ExecutionContext::loading())
    }

    #[must_use]
    pub(crate) fn with_context(started_at: Instant, context: ExecutionContext) -> Self {
        Self {
            stage: ExecutionStage::Planning,
            context,
            started_at,
            progress: ExecutionProgress::default(),
            scroll: 0,
            follow: true,
            cancellation_requested: false,
        }
    }

    pub(crate) const fn apply(&mut self, action: ExecutionAction) {
        match action {
            ExecutionAction::SetStage(stage) => self.stage = stage,
            ExecutionAction::End => {
                self.follow = true;
                self.scroll = 0;
            }
            ExecutionAction::RequestCancellation => self.cancellation_requested = true,
        }
    }

    pub(crate) fn apply_scroll(
        &mut self,
        action: ExecutionScroll,
        current_offset: u16,
        max_offset: u16,
    ) {
        let offset = match action {
            ExecutionScroll::Up => current_offset.saturating_sub(1),
            ExecutionScroll::Down => current_offset.saturating_add(1).min(max_offset),
            ExecutionScroll::PageUp => current_offset.saturating_sub(PAGE_SCROLL),
            ExecutionScroll::PageDown => current_offset.saturating_add(PAGE_SCROLL).min(max_offset),
        };
        self.follow = false;
        self.scroll = offset;
    }

    pub(crate) fn record(&mut self, event: ExecutionEvent) {
        match &event.kind {
            ExecutionEventKind::Phase(ExecutionPhase::Reading) => {
                self.stage = ExecutionStage::Reading;
            }
            ExecutionEventKind::Phase(ExecutionPhase::Matching) => {
                self.stage = ExecutionStage::Matching;
            }
            ExecutionEventKind::Workspace(workspace) => {
                self.context = self.context.clone().with_workspace(workspace.clone());
            }
            ExecutionEventKind::Terminated(termination)
                if !termination.interrupted
                    && !matches!(termination.status, ProcessExitStatus::Exited(0)) =>
            {
                self.stage = ExecutionStage::Failed;
            }
            _ => {}
        }
        self.progress.record(event);
    }

    pub(crate) fn fail(&mut self, message: String, received_at: Instant) {
        self.stage = ExecutionStage::Failed;
        self.record(ExecutionEvent {
            received_at,
            kind: ExecutionEventKind::Diagnostic(Diagnostic {
                severity: super::progress::DiagnosticSeverity::Error,
                summary: message,
                detail: None,
                position: None,
                source: DiagnosticSource::Terraform,
                raw: None,
            }),
        });
    }

    #[must_use]
    pub(crate) const fn stage(&self) -> ExecutionStage {
        self.stage
    }

    #[must_use]
    pub(crate) const fn started_at(&self) -> Instant {
        self.started_at
    }

    #[must_use]
    pub(crate) const fn context(&self) -> &ExecutionContext {
        &self.context
    }

    #[must_use]
    pub(crate) const fn progress(&self) -> &ExecutionProgress {
        &self.progress
    }

    #[must_use]
    pub(crate) const fn scroll(&self) -> u16 {
        self.scroll
    }

    #[must_use]
    pub(crate) const fn follows_latest(&self) -> bool {
        self.follow
    }

    #[must_use]
    pub(crate) fn elapsed_at(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.started_at)
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
    use super::super::progress::ProcessTermination;
    use super::*;

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
    fn manual_scroll_stops_following_until_end_is_pressed() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);

        state.apply_scroll(ExecutionScroll::Down, 0, 10);
        assert!(!state.follows_latest());
        assert_eq!(state.scroll(), 1);

        state.apply(ExecutionAction::End);
        assert!(state.follows_latest());
        assert_eq!(state.scroll(), 0);
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
    fn nonzero_termination_enters_failed_stage() {
        let started_at = Instant::now();
        let mut state = ExecutionState::new(started_at);

        state.record(event(
            started_at + Duration::from_secs(1),
            ExecutionEventKind::Terminated(ProcessTermination {
                status: ProcessExitStatus::Exited(1),
                interrupted: false,
            }),
        ));

        assert_eq!(state.stage(), ExecutionStage::Failed);
        assert_eq!(state.progress().events().len(), 1);
        assert!(matches!(
            state.progress().events()[0].kind,
            ExecutionEventKind::Terminated(ProcessTermination { .. })
        ));
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
}
