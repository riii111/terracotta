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
    Phase(ExecutionPhase),
    Workspace(String),
    Git(Option<String>),
    Informational {
        event_type: String,
        message: Option<String>,
    },
    Terminated(ProcessTermination),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionPhase {
    Reading,
    Matching,
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
            Self::Phase(phase) => formatter.debug_tuple("Phase").field(phase).finish(),
            Self::Workspace(_) => formatter.write_str("Workspace(<redacted>)"),
            Self::Git(_) => formatter.write_str("Git(<redacted>)"),
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
