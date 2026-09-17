use std::time::{Duration, Instant};

use super::progress::{ExecutionEvent, ExecutionEventKind, ExecutionProgress, ProcessExitStatus};

const PAGE_SCROLL: u16 = 8;

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
    SetStage(ExecutionStage),
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    End,
    RequestCancellation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionState {
    stage: ExecutionStage,
    started_at: Instant,
    progress: ExecutionProgress,
    scroll: u16,
    follow: bool,
    cancellation_requested: bool,
}

impl ExecutionState {
    #[must_use]
    pub(crate) fn new(started_at: Instant) -> Self {
        Self {
            stage: ExecutionStage::Planning,
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
            ExecutionAction::ScrollUp => {
                self.follow = false;
                self.scroll = self.scroll.saturating_sub(1);
            }
            ExecutionAction::ScrollDown => {
                self.follow = false;
                self.scroll = self.scroll.saturating_add(1);
            }
            ExecutionAction::PageUp => {
                self.follow = false;
                self.scroll = self.scroll.saturating_sub(PAGE_SCROLL);
            }
            ExecutionAction::PageDown => {
                self.follow = false;
                self.scroll = self.scroll.saturating_add(PAGE_SCROLL);
            }
            ExecutionAction::End => {
                self.follow = true;
                self.scroll = 0;
            }
            ExecutionAction::RequestCancellation => self.cancellation_requested = true,
        }
    }

    pub(crate) fn record(&mut self, event: ExecutionEvent) {
        if let ExecutionEventKind::Terminated(termination) = &event.kind
            && !termination.interrupted
            && !matches!(termination.status, ProcessExitStatus::Exited(0))
        {
            self.stage = ExecutionStage::Failed;
        }
        self.progress.record(event);
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

        state.apply(ExecutionAction::ScrollDown);
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
}
