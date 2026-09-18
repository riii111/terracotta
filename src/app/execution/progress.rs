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
    resources: Vec<(String, ResourceProgress)>,
    resource_indices: BTreeMap<String, usize>,
    summary: Option<ExecutionSummary>,
    diagnostics: Vec<Diagnostic>,
    termination: Option<ProcessTermination>,
    completed_resources: usize,
    last_event_at: Option<Instant>,
}

impl ExecutionProgress {
    pub(crate) fn record(&mut self, event: ExecutionEvent) {
        let ExecutionEvent { received_at, kind } = event;
        self.last_event_at = Some(received_at);
        match kind {
            ExecutionEventKind::Resource(resource) => {
                if let Some(&index) = self.resource_indices.get(&resource.address) {
                    let progress = &mut self.resources[index].1;
                    let was_completed = progress.completed;
                    progress.kind = resource.kind;
                    progress.completed = was_completed || resource.kind.is_complete();
                    if progress.completed && !was_completed {
                        self.completed_resources += 1;
                    }
                } else {
                    let completed = resource.kind.is_complete();
                    self.resource_indices
                        .insert(resource.address.clone(), self.resources.len());
                    self.resources.push((
                        resource.address.clone(),
                        ResourceProgress {
                            kind: resource.kind,
                            completed,
                        },
                    ));
                    if completed {
                        self.completed_resources += 1;
                    }
                }
            }
            ExecutionEventKind::Summary(summary) => self.summary = Some(summary),
            ExecutionEventKind::Diagnostic(diagnostic) => self.diagnostics.push(diagnostic),
            ExecutionEventKind::Phase(_)
            | ExecutionEventKind::Workspace(_)
            | ExecutionEventKind::Git(_)
            | ExecutionEventKind::Informational { .. } => {}
            ExecutionEventKind::Terminated(termination) => self.termination = Some(termination),
        }
    }

    pub(crate) fn resources(&self) -> impl Iterator<Item = (&str, ResourceProgress)> {
        self.resources
            .iter()
            .map(|(address, progress)| (address.as_str(), *progress))
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
    pub(crate) const fn last_event_at(&self) -> Option<Instant> {
        self.last_event_at
    }

    #[must_use]
    pub(crate) const fn termination(&self) -> Option<ProcessTermination> {
        self.termination
    }

    #[must_use]
    pub(crate) const fn completed_resources(&self) -> usize {
        self.completed_resources
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
                (
                    "aws_vpc.main",
                    ResourceProgress {
                        kind: ResourceEventKind::ApplyComplete,
                        completed: true,
                    },
                ),
                (
                    "aws_subnet.private[0]",
                    ResourceProgress {
                        kind: ResourceEventKind::RefreshStart,
                        completed: false,
                    },
                ),
            ]
        );
        assert_eq!(progress.completed_resources(), 1);
    }

    #[test]
    fn completion_count_stays_stable_for_duplicate_and_post_completion_events() {
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

        assert_eq!(progress.completed_resources(), 1);
        assert_eq!(
            progress.resources().collect::<Vec<_>>(),
            vec![(
                "aws_vpc.main",
                ResourceProgress {
                    kind: ResourceEventKind::ApplyStart,
                    completed: true,
                },
            )]
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
        assert_eq!(
            hundred.completed_resources(),
            ten_thousand.completed_resources()
        );
        assert_eq!(hundred.diagnostics(), ten_thousand.diagnostics());
        assert_eq!(hundred.summary(), ten_thousand.summary());
        assert_eq!(hundred.termination(), ten_thousand.termination());
    }

    #[test]
    fn retains_summary_diagnostics_termination_and_last_received_time() {
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
        progress.record(ExecutionEvent {
            received_at: diagnostic_at,
            kind: ExecutionEventKind::Diagnostic(Diagnostic {
                severity: DiagnosticSeverity::Error,
                summary: "Planning failed".to_owned(),
                detail: Some("The configuration is invalid.".to_owned()),
                position: None,
                source: DiagnosticSource::Terraform,
                raw: None,
            }),
        });
        progress.record(ExecutionEvent {
            received_at: termination_at,
            kind: ExecutionEventKind::Terminated(ProcessTermination {
                status: ProcessExitStatus::Exited(1),
                interrupted: false,
            }),
        });

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
        assert_eq!(progress.last_event_at(), Some(termination_at));
    }
}
