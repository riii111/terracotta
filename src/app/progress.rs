use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceEventKind {
    RefreshStart,
    RefreshComplete,
    ApplyStart,
    ApplyProgress,
    ApplyComplete,
    ApplyErrored,
    ProvisionStart,
    ProvisionProgress,
    ProvisionComplete,
    ProvisionErrored,
    ImportStart,
    ImportComplete,
    EphemeralStart,
    EphemeralProgress,
    EphemeralComplete,
    EphemeralErrored,
    ResourceDrift,
    PlannedChange,
}

impl ResourceEventKind {
    #[must_use]
    pub(crate) const fn is_complete(self) -> bool {
        matches!(
            self,
            Self::RefreshComplete
                | Self::ApplyComplete
                | Self::ApplyErrored
                | Self::ProvisionComplete
                | Self::ProvisionErrored
                | Self::ImportComplete
                | Self::EphemeralComplete
                | Self::EphemeralErrored
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceEvent {
    pub(crate) address: String,
    pub(crate) kind: ResourceEventKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExecutionSummary {
    pub(crate) adds: Option<usize>,
    pub(crate) changes: Option<usize>,
    pub(crate) removes: Option<usize>,
    pub(crate) operation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiagnosticPoint {
    pub(crate) line: u64,
    pub(crate) column: u64,
    pub(crate) byte: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiagnosticPosition {
    pub(crate) filename: String,
    pub(crate) start: DiagnosticPoint,
    pub(crate) end: DiagnosticPoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiagnosticSource {
    Terraform,
    UnknownEvent {
        stream: EventStream,
        event_type: Option<String>,
    },
    UnsupportedSchema {
        stream: EventStream,
        major: Option<u64>,
    },
    NonJson {
        stream: EventStream,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    pub(crate) severity: DiagnosticSeverity,
    pub(crate) summary: String,
    pub(crate) detail: Option<String>,
    pub(crate) position: Option<DiagnosticPosition>,
    pub(crate) source: DiagnosticSource,
    pub(crate) raw: Option<String>,
}

impl Debug for Diagnostic {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Diagnostic")
            .field("severity", &self.severity)
            .field("summary", &"<redacted>")
            .field("detail", &self.detail.as_ref().map(|_| "<redacted>"))
            .field("position", &self.position)
            .field("source", &self.source)
            .field("raw", &self.raw.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessExitStatus {
    Exited(i32),
    Signaled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessTermination {
    pub(crate) status: ProcessExitStatus,
    pub(crate) interrupted: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ExecutionEventKind {
    Resource(ResourceEvent),
    Summary(ExecutionSummary),
    Diagnostic(Diagnostic),
    Informational {
        event_type: String,
        message: Option<String>,
    },
    Terminated(ProcessTermination),
}

impl Debug for ExecutionEventKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resource(event) => formatter.debug_tuple("Resource").field(event).finish(),
            Self::Summary(summary) => formatter.debug_tuple("Summary").field(summary).finish(),
            Self::Diagnostic(diagnostic) => formatter
                .debug_tuple("Diagnostic")
                .field(diagnostic)
                .finish(),
            Self::Informational { event_type, .. } => formatter
                .debug_struct("Informational")
                .field("event_type", event_type)
                .field("message", &"<redacted>")
                .finish(),
            Self::Terminated(termination) => formatter
                .debug_tuple("Terminated")
                .field(termination)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionEvent {
    pub(crate) received_at: Instant,
    pub(crate) kind: ExecutionEventKind,
}

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
            ExecutionEventKind::Informational { .. } => {}
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
