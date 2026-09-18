use std::collections::BTreeMap;
use std::time::Instant;

use super::event::{
    Diagnostic, DiagnosticSeverity, ExecutionEvent, ExecutionEventKind, ProcessTermination,
    ResourceEventKind,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExecutionProgress {
    resources: Vec<(String, ResourceEventKind)>,
    resource_indices: BTreeMap<String, usize>,
    diagnostics: Vec<Diagnostic>,
    termination: Option<ProcessTermination>,
    last_event_at: Option<Instant>,
}

impl ExecutionProgress {
    pub(crate) fn record(&mut self, event: ExecutionEvent) {
        let ExecutionEvent { received_at, kind } = event;
        self.last_event_at = Some(received_at);
        match kind {
            ExecutionEventKind::Resource(resource) => {
                if let Some(&index) = self.resource_indices.get(&resource.address) {
                    self.resources[index].1 = resource.kind;
                } else {
                    self.resource_indices
                        .insert(resource.address.clone(), self.resources.len());
                    self.resources.push((resource.address, resource.kind));
                }
            }
            ExecutionEventKind::Diagnostic(diagnostic) => self.diagnostics.push(diagnostic),
            ExecutionEventKind::Summary(_)
            | ExecutionEventKind::Phase(_)
            | ExecutionEventKind::Workspace(_)
            | ExecutionEventKind::Git(_)
            | ExecutionEventKind::Informational { .. } => {}
            ExecutionEventKind::Terminated(termination) => self.termination = Some(termination),
        }
    }

    pub(crate) fn resources(&self) -> impl Iterator<Item = (&str, ResourceEventKind)> {
        self.resources
            .iter()
            .map(|(address, kind)| (address.as_str(), *kind))
    }

    #[must_use]
    pub(crate) fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub(crate) fn take_review_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
            .into_iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.severity,
                    DiagnosticSeverity::Warning
                        | DiagnosticSeverity::Error
                        | DiagnosticSeverity::Unknown
                )
            })
            .collect()
    }

    #[must_use]
    pub(crate) const fn last_event_at(&self) -> Option<Instant> {
        self.last_event_at
    }

    #[must_use]
    pub(crate) const fn termination(&self) -> Option<ProcessTermination> {
        self.termination
    }
}

#[cfg(test)]
mod tests {
    use super::super::event::{
        DiagnosticSource, ExecutionSummary, ProcessExitStatus, ResourceEvent,
    };
    use super::*;

    fn event(kind: ExecutionEventKind) -> ExecutionEvent {
        ExecutionEvent {
            received_at: Instant::now(),
            kind,
        }
    }

    #[test]
    fn preserves_first_seen_order_and_latest_state_for_interleaved_resources() {
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

        assert_eq!(
            progress.resources().collect::<Vec<_>>(),
            vec![
                ("aws_vpc.main", ResourceEventKind::ApplyComplete,),
                ("aws_subnet.private[0]", ResourceEventKind::RefreshStart,),
            ]
        );
    }

    #[test]
    fn post_completion_event_updates_latest_state_without_new_resource() {
        let mut progress = ExecutionProgress::default();
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::RefreshComplete,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::RefreshComplete,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::ApplyStart,
        })));

        assert_eq!(
            progress.resources().collect::<Vec<_>>(),
            vec![("aws_vpc.main", ResourceEventKind::ApplyStart)]
        );
    }

    #[test]
    fn repeated_events_have_the_same_final_state_without_retaining_history() {
        fn progress_after_repeated_events(event_count: usize) -> ExecutionProgress {
            let mut progress = ExecutionProgress::default();
            for _ in 0..event_count {
                progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
                    address: "aws_vpc.main".to_owned(),
                    kind: ResourceEventKind::RefreshStart,
                })));
            }
            progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
                address: "aws_vpc.main".to_owned(),
                kind: ResourceEventKind::RefreshComplete,
            })));
            progress
        }

        let hundred = progress_after_repeated_events(100);
        let ten_thousand = progress_after_repeated_events(10_000);

        assert_eq!(
            hundred.resources().collect::<Vec<_>>(),
            ten_thousand.resources().collect::<Vec<_>>()
        );
        assert_eq!(hundred.diagnostics(), ten_thousand.diagnostics());
        assert_eq!(hundred.termination(), ten_thousand.termination());
    }

    #[test]
    fn summary_updates_last_received_time_and_diagnostics_survive_termination() {
        let started_at = Instant::now();
        let summary_at = started_at + std::time::Duration::from_secs(1);
        let diagnostic_at = started_at + std::time::Duration::from_secs(2);
        let termination_at = started_at + std::time::Duration::from_secs(3);
        let mut progress = ExecutionProgress::default();
        progress.record(ExecutionEvent {
            received_at: summary_at,
            kind: ExecutionEventKind::Summary(ExecutionSummary {
                adds: Some(1),
                changes: Some(2),
                removes: Some(3),
                operation: Some("plan".to_owned()),
            }),
        });
        assert_eq!(progress.last_event_at(), Some(summary_at));
        progress.record(ExecutionEvent {
            received_at: diagnostic_at,
            kind: ExecutionEventKind::Diagnostic(Diagnostic {
                severity: DiagnosticSeverity::Error,
                summary: "Planning failed".to_owned(),
                detail: Some("The configuration is invalid.".to_owned()),
                position: None,
                source: DiagnosticSource::Terraform,
            }),
        });
        progress.record(ExecutionEvent {
            received_at: termination_at,
            kind: ExecutionEventKind::Terminated(ProcessTermination {
                status: ProcessExitStatus::Exited(1),
                interrupted: false,
            }),
        });

        assert_eq!(progress.diagnostics().len(), 1);
        assert_eq!(
            progress.termination(),
            Some(ProcessTermination {
                status: ProcessExitStatus::Exited(1),
                interrupted: false,
            })
        );
        assert_eq!(progress.last_event_at(), Some(termination_at));
    }

    #[test]
    fn takes_review_diagnostics_in_receive_order_and_discards_info() {
        let mut progress = ExecutionProgress::default();
        for (severity, summary) in [
            (DiagnosticSeverity::Info, "info"),
            (DiagnosticSeverity::Warning, "warning"),
            (DiagnosticSeverity::Error, "error"),
            (DiagnosticSeverity::Unknown, "unknown"),
        ] {
            progress.record(event(ExecutionEventKind::Diagnostic(Diagnostic {
                severity,
                summary: summary.to_owned(),
                detail: None,
                position: None,
                source: DiagnosticSource::Terraform,
            })));
        }

        let diagnostics = progress.take_review_diagnostics();

        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.summary.as_str())
                .collect::<Vec<_>>(),
            vec!["warning", "error", "unknown"]
        );
        assert!(progress.diagnostics().is_empty());
    }
}
