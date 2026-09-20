use std::collections::BTreeMap;
use std::time::Instant;

use super::event::{
    Diagnostic, EventStream, ExecutionEvent, ExecutionEventKind, ExecutionLogLine,
    ProcessTermination, ResourceEventKind,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExecutionProgress {
    resources: Vec<(String, ResourceEventKind)>,
    resource_indices: BTreeMap<String, usize>,
    diagnostics: Vec<Diagnostic>,
    log: Vec<ExecutionLogLine>,
    first_error_line: Option<usize>,
    termination: Option<ProcessTermination>,
    last_event_at: Option<Instant>,
}

impl ExecutionProgress {
    pub(crate) fn record(&mut self, event: ExecutionEvent) {
        let ExecutionEvent { received_at, kind } = event;
        self.last_event_at = Some(received_at);
        match kind {
            ExecutionEventKind::Log(line) => self.log.push(line),
            ExecutionEventKind::Resource(resource) => {
                if let Some(message) = resource.message.clone() {
                    self.log.push(ExecutionLogLine {
                        stream: EventStream::Stdout,
                        text: message,
                    });
                }
                if let Some(&index) = self.resource_indices.get(&resource.address) {
                    self.resources[index].1 = resource.kind;
                } else {
                    self.resource_indices
                        .insert(resource.address.clone(), self.resources.len());
                    self.resources.push((resource.address, resource.kind));
                }
            }
            ExecutionEventKind::Diagnostic(diagnostic) => {
                if diagnostic.severity == super::event::DiagnosticSeverity::Error
                    && self.first_error_line.is_none()
                {
                    self.first_error_line = Some(rendered_line_count(&self.log));
                }
                self.log.push(ExecutionLogLine {
                    stream: EventStream::Stderr,
                    text: diagnostic.detail.as_ref().map_or_else(
                        || diagnostic.summary.clone(),
                        |detail| format!("{}\n{detail}", diagnostic.summary),
                    ),
                });
                self.diagnostics.push(diagnostic);
            }
            ExecutionEventKind::Informational {
                message: Some(message),
                ..
            } => self.log.push(ExecutionLogLine {
                stream: EventStream::Stdout,
                text: message,
            }),
            ExecutionEventKind::Summary(summary) => {
                if let Some(message) = summary.message {
                    self.log.push(ExecutionLogLine {
                        stream: EventStream::Stdout,
                        text: message,
                    });
                }
            }
            ExecutionEventKind::Phase(_)
            | ExecutionEventKind::Workspace(_)
            | ExecutionEventKind::Informational { message: None, .. } => {}
            ExecutionEventKind::Terminated(termination) => self.termination = Some(termination),
        }
    }

    #[must_use]
    pub(crate) fn log(&self) -> &[ExecutionLogLine] {
        &self.log
    }

    #[must_use]
    pub(crate) fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub(crate) const fn first_error_line(&self) -> Option<usize> {
        self.first_error_line
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

fn rendered_line_count(log: &[ExecutionLogLine]) -> usize {
    log.iter().map(|line| line.text.lines().count()).sum()
}

#[cfg(test)]
mod tests {
    use super::super::event::{
        DiagnosticSeverity, DiagnosticSource, ExecutionSummary, ProcessExitStatus, ResourceEvent,
    };
    use super::*;

    fn resources(progress: &ExecutionProgress) -> Vec<(&str, ResourceEventKind)> {
        progress
            .resources
            .iter()
            .map(|(address, kind)| (address.as_str(), *kind))
            .collect()
    }

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
            message: None,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_subnet.private[0]".to_owned(),
            kind: ResourceEventKind::RefreshStart,
            message: None,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::RefreshComplete,
            message: None,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::ApplyStart,
            message: None,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::ApplyComplete,
            message: None,
        })));

        assert_eq!(
            resources(&progress),
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
            message: None,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::RefreshComplete,
            message: None,
        })));
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "aws_vpc.main".to_owned(),
            kind: ResourceEventKind::ApplyStart,
            message: None,
        })));

        assert_eq!(
            resources(&progress),
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
                    message: None,
                })));
            }
            progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
                address: "aws_vpc.main".to_owned(),
                kind: ResourceEventKind::RefreshComplete,
                message: None,
            })));
            progress
        }

        let hundred = progress_after_repeated_events(100);
        let ten_thousand = progress_after_repeated_events(10_000);

        assert_eq!(resources(&hundred), resources(&ten_thousand));
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
                message: Some("Plan: 1 to add, 2 to change, 3 to destroy.".to_owned()),
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
    fn appends_the_exact_terraform_messages_without_synthesizing_log_lines() {
        let mut progress = ExecutionProgress::default();
        progress.record(event(ExecutionEventKind::Resource(ResourceEvent {
            address: "terraform_data.api".to_owned(),
            kind: ResourceEventKind::PlannedChange,
            message: Some("terraform_data.api will be updated in-place".to_owned()),
        })));
        progress.record(event(ExecutionEventKind::Summary(ExecutionSummary {
            adds: Some(0),
            changes: Some(1),
            removes: None,
            operation: Some("plan".to_owned()),
            message: Some("Plan: 0 to add, 1 to change, 0 to destroy.".to_owned()),
        })));

        assert_eq!(
            progress
                .log()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            [
                "terraform_data.api will be updated in-place",
                "Plan: 0 to add, 1 to change, 0 to destroy."
            ]
        );
    }
}
