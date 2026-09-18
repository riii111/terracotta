use std::collections::BTreeMap;
use std::time::Instant;

use super::event::{
    Diagnostic, ExecutionEvent, ExecutionEventKind, ExecutionSummary, ProcessTermination,
    ResourceEventKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResourceProgress {
    pub(crate) kind: ResourceEventKind,
    pub(crate) completed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExecutionProgress {
    events: Vec<ExecutionEvent>,
    resources: BTreeMap<String, ResourceProgress>,
    summary: Option<ExecutionSummary>,
    diagnostics: Vec<Diagnostic>,
    termination: Option<ProcessTermination>,
    completed_resources: usize,
    total_resources: Option<usize>,
}

impl ExecutionProgress {
    pub(crate) fn record(&mut self, event: ExecutionEvent) {
        match &event.kind {
            ExecutionEventKind::Resource(resource) => {
                let was_completed = self
                    .resources
                    .get(&resource.address)
                    .is_some_and(|progress| progress.completed);
                let completed = was_completed || resource.kind.is_complete();
                self.resources.insert(
                    resource.address.clone(),
                    ResourceProgress {
                        kind: resource.kind,
                        completed,
                    },
                );
                if completed && !was_completed {
                    self.completed_resources += 1;
                }
            }
            ExecutionEventKind::Summary(summary) => self.summary = Some(summary.clone()),
            ExecutionEventKind::Diagnostic(diagnostic) => self.diagnostics.push(diagnostic.clone()),
            ExecutionEventKind::Phase(_)
            | ExecutionEventKind::Workspace(_)
            | ExecutionEventKind::Git(_)
            | ExecutionEventKind::Informational { .. } => {}
            ExecutionEventKind::Terminated(termination) => self.termination = Some(*termination),
        }
        self.events.push(event);
    }

    #[must_use]
    pub(crate) fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }

    #[must_use]
    pub(crate) const fn resources(&self) -> &BTreeMap<String, ResourceProgress> {
        &self.resources
    }

    #[must_use]
    pub(crate) const fn summary(&self) -> Option<&ExecutionSummary> {
        self.summary.as_ref()
    }

    #[must_use]
    pub(crate) fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub(crate) fn last_event_at(&self) -> Option<Instant> {
        self.events.last().map(|event| event.received_at)
    }

    #[must_use]
    pub(crate) const fn termination(&self) -> Option<ProcessTermination> {
        self.termination
    }

    #[must_use]
    pub(crate) const fn completed_resources(&self) -> usize {
        self.completed_resources
    }

    #[must_use]
    pub(crate) const fn total_resources(&self) -> Option<usize> {
        self.total_resources
    }

    #[must_use]
    pub(crate) const fn progress_ratio(&self) -> Option<(usize, usize)> {
        match self.total_resources {
            Some(total) => Some((self.completed_resources, total)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::event::{
        DiagnosticSeverity, DiagnosticSource, ProcessExitStatus, ResourceEvent,
    };
    use super::*;

    fn event(kind: ExecutionEventKind) -> ExecutionEvent {
        ExecutionEvent {
            received_at: Instant::now(),
            kind,
        }
    }

    #[test]
    fn tracks_interleaved_resources_without_inventing_total_or_ratio() {
        let mut progress = ExecutionProgress::default();
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::RefreshStart,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_subnet.private[0]".to_owned(),
            kind: ResourceEventKind::RefreshStart,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::RefreshComplete,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::ApplyStart,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::ApplyComplete,
        })));

        assert_eq!(progress.resources().len(), 2);
        assert_eq!(progress.completed_resources(), 1);
        assert_eq!(progress.total_resources(), None);
        assert_eq!(progress.progress_ratio(), None);
    }

    #[test]
    fn retains_summary_diagnostics_and_termination_in_event_order() {
        let mut progress = ExecutionProgress::default();
        progress.record(event(ExecutionEventKind::Summary(ExecutionSummary {
            adds: Some(1),
            changes: Some(2),
            removes: Some(3),
            operation: Some("plan".to_owned()),
        })));
        progress.record(event(ExecutionEventKind::Diagnostic(Diagnostic {
            severity: DiagnosticSeverity::Error,
            summary: "Planning failed".to_owned(),
            detail: Some("The configuration is invalid.".to_owned()),
            position: None,
            source: DiagnosticSource::Terraform,
            raw: None,
        })));
        progress.record(event(ExecutionEventKind::Terminated(ProcessTermination {
            status: ProcessExitStatus::Exited(1),
            interrupted: false,
        })));

        assert_eq!(progress.events().len(), 3);
        assert_eq!(
            progress.summary().expect("summary should be retained").adds,
            Some(1)
        );
        assert_eq!(progress.diagnostics().len(), 1);
        assert_eq!(
            progress.termination(),
            Some(ProcessTermination {
                status: ProcessExitStatus::Exited(1),
                interrupted: false,
            })
        );
    }
}
