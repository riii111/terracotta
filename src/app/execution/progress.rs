#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "execution target accessors are consumed by the SBI03-03 execution UI"
    )
)]

use std::fmt::{Debug, Formatter};
use std::time::Instant;

use crate::app::plan::PlanAction;

use super::event::{
    Diagnostic, EventStream, ExecutionEvent, ExecutionEventKind, ExecutionLogLine,
    ExecutionTargetSpec, ProcessTermination, ResourceEvent, ResourceEventKind, SensitiveValue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionTargetStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
    Incomplete,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ExecutionTargetState {
    spec: ExecutionTargetSpec,
    status: ExecutionTargetStatus,
    completed_stages: usize,
    log_ids: Vec<usize>,
}

impl Debug for ExecutionTargetState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExecutionTargetState")
            .field("spec", &self.spec)
            .field("status", &self.status)
            .field("completed_stages", &self.completed_stages)
            .field("log_ids", &self.log_ids)
            .finish()
    }
}

impl ExecutionTargetState {
    #[must_use]
    pub(crate) fn address(&self) -> &str {
        &self.spec.address
    }

    #[must_use]
    pub(crate) fn actions(&self) -> &[PlanAction] {
        &self.spec.actions
    }

    #[must_use]
    pub(crate) const fn status(&self) -> ExecutionTargetStatus {
        self.status
    }

    #[must_use]
    pub(crate) const fn completed_stages(&self) -> usize {
        self.completed_stages
    }

    #[must_use]
    pub(crate) fn log_ids(&self) -> &[usize] {
        &self.log_ids
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ExecutionProgress {
    diagnostics: Vec<Diagnostic>,
    log: Vec<ExecutionLogLine>,
    targets: Vec<ExecutionTargetState>,
    sensitive_values: Vec<SensitiveValue>,
    first_error_line: Option<usize>,
    termination: Option<ProcessTermination>,
    last_event_at: Option<Instant>,
}

impl Debug for ExecutionProgress {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExecutionProgress")
            .field("diagnostics", &self.diagnostics)
            .field("log", &self.log)
            .field("targets", &self.targets)
            .field("sensitive_values", &"<redacted>")
            .field("first_error_line", &self.first_error_line)
            .field("termination", &self.termination)
            .field("last_event_at", &self.last_event_at)
            .finish()
    }
}

impl Default for ExecutionProgress {
    fn default() -> Self {
        Self::new(Vec::new(), Vec::new())
    }
}

impl ExecutionProgress {
    #[must_use]
    pub(crate) fn new(
        targets: Vec<ExecutionTargetSpec>,
        sensitive_values: Vec<SensitiveValue>,
    ) -> Self {
        Self {
            diagnostics: Vec::new(),
            log: Vec::new(),
            targets: targets
                .into_iter()
                .map(|spec| ExecutionTargetState {
                    spec,
                    status: ExecutionTargetStatus::Pending,
                    completed_stages: 0,
                    log_ids: Vec::new(),
                })
                .collect(),
            sensitive_values,
            first_error_line: None,
            termination: None,
            last_event_at: None,
        }
    }

    pub(crate) fn record(&mut self, event: ExecutionEvent) {
        let ExecutionEvent { received_at, kind } = event;
        self.last_event_at = Some(received_at);
        match kind {
            ExecutionEventKind::Log(line) => {
                if self.first_error_line.is_none()
                    && line
                        .text
                        .lines()
                        .any(|text| text.trim_start().starts_with("Error:"))
                {
                    self.first_error_line = Some(rendered_line_count(&self.log));
                }
                self.append_log(line.stream, &line.text, None);
            }
            ExecutionEventKind::Resource(resource) => {
                if let Some(message) = &resource.message {
                    self.append_log(
                        EventStream::Stdout,
                        message,
                        self.target_index(&resource.address),
                    );
                }
                self.record_resource(&resource);
            }
            ExecutionEventKind::Diagnostic(diagnostic) => {
                if diagnostic.severity == super::event::DiagnosticSeverity::Error
                    && self.first_error_line.is_none()
                {
                    self.first_error_line = Some(rendered_line_count(&self.log));
                }
                let address = diagnostic.address.clone();
                let summary =
                    super::super::copy::sanitize_text(&diagnostic.summary, &self.sensitive_values);
                let detail = diagnostic.detail.as_ref().map(|detail| {
                    super::super::copy::sanitize_text(detail, &self.sensitive_values)
                });
                let text = detail
                    .as_ref()
                    .map_or_else(|| summary.clone(), |detail| format!("{summary}\n{detail}"));
                self.append_log(
                    EventStream::Stderr,
                    &text,
                    address
                        .as_deref()
                        .and_then(|address| self.target_index(address)),
                );
                if diagnostic.severity == super::event::DiagnosticSeverity::Error
                    && let Some(target) = address
                        .as_deref()
                        .and_then(|address| self.target_index(address))
                {
                    self.targets[target].status = ExecutionTargetStatus::Failed;
                }
                self.diagnostics.push(Diagnostic {
                    summary,
                    detail,
                    ..diagnostic
                });
            }
            ExecutionEventKind::Informational {
                message: Some(message),
                ..
            } => self.append_log(EventStream::Stdout, &message, None),
            ExecutionEventKind::Summary(summary) => {
                if let Some(message) = summary.message {
                    self.append_log(EventStream::Stdout, &message, None);
                }
            }
            ExecutionEventKind::Phase(_)
            | ExecutionEventKind::Workspace(_)
            | ExecutionEventKind::Informational { message: None, .. } => {}
            ExecutionEventKind::Terminated(termination) => self.termination = Some(termination),
        }
    }

    pub(crate) fn finish(&mut self, termination: ProcessTermination) {
        self.termination = Some(termination);
        for target in &mut self.targets {
            target.status = match target.status {
                ExecutionTargetStatus::Pending => ExecutionTargetStatus::Skipped,
                ExecutionTargetStatus::Running => ExecutionTargetStatus::Incomplete,
                status => status,
            };
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
    pub(crate) fn targets(&self) -> &[ExecutionTargetState] {
        &self.targets
    }

    #[must_use]
    pub(crate) fn sensitive_values(&self) -> &[SensitiveValue] {
        &self.sensitive_values
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

    fn append_log(&mut self, stream: EventStream, text: &str, target: Option<usize>) {
        let log_id = self.log.len();
        self.log.push(ExecutionLogLine {
            stream,
            text: super::super::copy::sanitize_text(text, &self.sensitive_values),
        });
        if let Some(target) = target {
            self.targets[target].log_ids.push(log_id);
        }
    }

    fn record_resource(&mut self, resource: &ResourceEvent) {
        let Some(target) = self.target_index(&resource.address) else {
            return;
        };
        match resource.kind {
            ResourceEventKind::ApplyStart
            | ResourceEventKind::ApplyProgress
            | ResourceEventKind::ProvisionStart
            | ResourceEventKind::ProvisionProgress => {
                if self.targets[target].status == ExecutionTargetStatus::Pending
                    && resource.action.as_ref().is_some_and(|action| {
                        self.targets[target]
                            .actions()
                            .iter()
                            .any(|expected| expected == action)
                    })
                {
                    self.targets[target].status = ExecutionTargetStatus::Running;
                }
            }
            ResourceEventKind::ApplyComplete => {
                let target_state = &mut self.targets[target];
                if target_state.status == ExecutionTargetStatus::Failed {
                    return;
                }
                let Some(action) = resource.action.as_ref() else {
                    return;
                };
                if target_state.spec.actions.get(target_state.completed_stages) != Some(action) {
                    return;
                }
                target_state.status = ExecutionTargetStatus::Running;
                target_state.completed_stages = target_state
                    .completed_stages
                    .saturating_add(1)
                    .min(target_state.spec.actions.len());
                if target_state.completed_stages == target_state.spec.actions.len() {
                    target_state.status = ExecutionTargetStatus::Completed;
                }
            }
            ResourceEventKind::ApplyErrored | ResourceEventKind::ProvisionErrored => {
                self.targets[target].status = ExecutionTargetStatus::Failed;
            }
            ResourceEventKind::RefreshStart
            | ResourceEventKind::RefreshComplete
            | ResourceEventKind::ProvisionComplete
            | ResourceEventKind::ImportStart
            | ResourceEventKind::ImportComplete
            | ResourceEventKind::EphemeralStart
            | ResourceEventKind::EphemeralProgress
            | ResourceEventKind::EphemeralComplete
            | ResourceEventKind::EphemeralErrored
            | ResourceEventKind::ResourceDrift
            | ResourceEventKind::PlannedChange => {}
        }
    }

    fn target_index(&self, address: &str) -> Option<usize> {
        self.targets
            .iter()
            .position(|target| target.address() == address)
    }
}

fn rendered_line_count(log: &[ExecutionLogLine]) -> usize {
    log.iter().map(|line| line.text.lines().count()).sum()
}

#[cfg(test)]
mod tests {
    use super::super::event::{
        DiagnosticSeverity, DiagnosticSource, ExecutionSummary, ProcessExitStatus,
    };
    use super::*;

    fn event(kind: ExecutionEventKind) -> ExecutionEvent {
        ExecutionEvent {
            received_at: Instant::now(),
            kind,
        }
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
                address: None,
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
            action: None,
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

    #[test]
    fn replacement_waits_for_both_apply_stages_before_completion() {
        let mut progress = ExecutionProgress::new(
            vec![ExecutionTargetSpec {
                address: "terraform_data.api".to_owned(),
                actions: vec![PlanAction::Delete, PlanAction::Create],
            }],
            Vec::new(),
        );
        let started_at = Instant::now();

        assert_eq!(
            progress.targets()[0].actions(),
            &[PlanAction::Delete, PlanAction::Create]
        );

        progress.record(ExecutionEvent {
            received_at: started_at,
            kind: ExecutionEventKind::Resource(ResourceEvent {
                address: "terraform_data.api".to_owned(),
                kind: ResourceEventKind::ApplyStart,
                action: Some(PlanAction::Delete),
                message: Some("terraform_data.api: Destroying...".to_owned()),
            }),
        });
        progress.record(ExecutionEvent {
            received_at: started_at,
            kind: ExecutionEventKind::Resource(ResourceEvent {
                address: "terraform_data.api".to_owned(),
                kind: ResourceEventKind::ApplyComplete,
                action: Some(PlanAction::Delete),
                message: None,
            }),
        });

        assert_eq!(
            progress.targets()[0].status(),
            ExecutionTargetStatus::Running
        );
        assert_eq!(progress.targets()[0].completed_stages(), 1);

        progress.record(ExecutionEvent {
            received_at: started_at,
            kind: ExecutionEventKind::Resource(ResourceEvent {
                address: "terraform_data.api".to_owned(),
                kind: ResourceEventKind::ApplyComplete,
                action: Some(PlanAction::Create),
                message: Some("terraform_data.api: Creation complete".to_owned()),
            }),
        });

        assert_eq!(
            progress.targets()[0].status(),
            ExecutionTargetStatus::Completed
        );
        assert_eq!(progress.targets()[0].completed_stages(), 2);
        assert_eq!(progress.targets()[0].log_ids(), &[0, 1]);
    }

    #[test]
    fn explicit_diagnostic_address_is_attributed_but_unknown_events_are_overall_logs() {
        let mut progress = ExecutionProgress::new(
            vec![ExecutionTargetSpec {
                address: "terraform_data.api".to_owned(),
                actions: vec![PlanAction::Update],
            }],
            vec![SensitiveValue::Text("secret-value".to_owned())],
        );
        let received_at = Instant::now();
        progress.record(ExecutionEvent {
            received_at,
            kind: ExecutionEventKind::Diagnostic(Diagnostic {
                severity: DiagnosticSeverity::Error,
                summary: "Request secret-value failed".to_owned(),
                detail: Some("detail".to_owned()),
                address: Some("terraform_data.api".to_owned()),
                position: None,
                source: DiagnosticSource::Terraform,
            }),
        });
        progress.record(ExecutionEvent {
            received_at,
            kind: ExecutionEventKind::Diagnostic(Diagnostic {
                severity: DiagnosticSeverity::Unknown,
                summary: "Future event".to_owned(),
                detail: Some("Event type: future".to_owned()),
                address: None,
                position: None,
                source: DiagnosticSource::UnknownEvent {
                    stream: EventStream::Stdout,
                    event_type: Some("future".to_owned()),
                },
            }),
        });
        progress.finish(ProcessTermination {
            status: ProcessExitStatus::Exited(1),
            interrupted: false,
        });

        assert_eq!(
            progress.targets()[0].status(),
            ExecutionTargetStatus::Failed
        );
        assert_eq!(progress.targets()[0].log_ids(), &[0]);
        assert_eq!(
            progress.log()[0].text,
            "Request (sensitive value) failed\ndetail"
        );
        assert_eq!(progress.log()[1].text, "Future event\nEvent type: future");
        assert_eq!(
            progress.diagnostics()[0].summary,
            "Request (sensitive value) failed"
        );
        assert_eq!(progress.diagnostics()[0].detail.as_deref(), Some("detail"));
    }

    #[test]
    fn replacement_completion_requires_the_next_planned_action() {
        let mut progress = ExecutionProgress::new(
            vec![ExecutionTargetSpec {
                address: "terraform_data.api".to_owned(),
                actions: vec![PlanAction::Delete, PlanAction::Create],
            }],
            Vec::new(),
        );
        let received_at = Instant::now();

        for action in [PlanAction::Create, PlanAction::Delete, PlanAction::Delete] {
            progress.record(ExecutionEvent {
                received_at,
                kind: ExecutionEventKind::Resource(ResourceEvent {
                    address: "terraform_data.api".to_owned(),
                    kind: ResourceEventKind::ApplyComplete,
                    action: Some(action),
                    message: None,
                }),
            });
        }
        assert_eq!(progress.targets()[0].completed_stages(), 1);
        assert_eq!(
            progress.targets()[0].status(),
            ExecutionTargetStatus::Running
        );

        progress.record(ExecutionEvent {
            received_at,
            kind: ExecutionEventKind::Resource(ResourceEvent {
                address: "terraform_data.api".to_owned(),
                kind: ResourceEventKind::ApplyComplete,
                action: Some(PlanAction::Create),
                message: None,
            }),
        });

        assert_eq!(progress.targets()[0].completed_stages(), 2);
        assert_eq!(
            progress.targets()[0].status(),
            ExecutionTargetStatus::Completed
        );
    }

    #[test]
    fn finish_distinguishes_pending_and_running_targets_without_inventing_success() {
        let mut progress = ExecutionProgress::new(
            vec![
                ExecutionTargetSpec {
                    address: "terraform_data.pending".to_owned(),
                    actions: vec![PlanAction::Create],
                },
                ExecutionTargetSpec {
                    address: "terraform_data.running".to_owned(),
                    actions: vec![PlanAction::Update],
                },
            ],
            Vec::new(),
        );
        let received_at = Instant::now();
        progress.record(ExecutionEvent {
            received_at,
            kind: ExecutionEventKind::Resource(ResourceEvent {
                address: "terraform_data.running".to_owned(),
                kind: ResourceEventKind::ApplyStart,
                action: Some(PlanAction::Update),
                message: None,
            }),
        });
        progress.finish(ProcessTermination {
            status: ProcessExitStatus::Exited(0),
            interrupted: false,
        });

        assert_eq!(
            progress.targets()[0].status(),
            ExecutionTargetStatus::Skipped
        );
        assert_eq!(
            progress.targets()[1].status(),
            ExecutionTargetStatus::Incomplete
        );
    }
}
