use super::*;
use crate::ui::test_support::REPRESENTATIVE_TERMINAL_SIZE;

#[test]
fn planning_shows_resource_progress_and_waiting_time() {
    let started_at = Instant::now();
    let mut state = ExecutionState::with_context(
        started_at,
        ExecutionContext::known(
            "infra/prod",
            "default",
            "feature/network",
            "working tree vs HEAD",
        ),
    );
    state.record(resource_event(
        started_at + Duration::from_secs(1),
        "aws_vpc.main",
        ResourceEventKind::RefreshComplete,
    ));
    state.record(resource_event(
        started_at + Duration::from_secs(3),
        "aws_instance.api",
        ResourceEventKind::RefreshStart,
    ));

    insta::assert_snapshot!(buffer_text(&render_to_buffer(
        &state,
        started_at + Duration::from_secs(5),
        REPRESENTATIVE_TERMINAL_SIZE.0,
        REPRESENTATIVE_TERMINAL_SIZE.1,
    )));
}

#[test]
fn narrow_failure_wraps_diagnostic_and_preserves_quit_hint() {
    let started_at = Instant::now();
    let mut state = ExecutionState::new(started_at);
    let diagnostic_detail =
        "Run terraform init to install the providers required by this configuration.".to_owned();
    state.record(event(
        started_at + Duration::from_secs(1),
        ExecutionEventKind::Diagnostic(Diagnostic {
            severity: DiagnosticSeverity::Error,
            summary: "Terraform initialization required".to_owned(),
            detail: Some(diagnostic_detail.clone()),
            position: Some(DiagnosticPosition {
                filename: "infra/prod/main.tf".to_owned(),
                start: DiagnosticPoint {
                    line: 12,
                    column: 3,
                    byte: None,
                },
                end: DiagnosticPoint {
                    line: 12,
                    column: 9,
                    byte: None,
                },
            }),
            source: DiagnosticSource::Terraform,
            raw: None,
        }),
    ));
    state.record(event(
        started_at + Duration::from_secs(2),
        ExecutionEventKind::Terminated(ProcessTermination {
            status: ProcessExitStatus::Exited(1),
            interrupted: false,
        }),
    ));
    assert_eq!(state.progress().diagnostics().len(), 1);
    assert_eq!(
        state.progress().diagnostics()[0].summary,
        "Terraform initialization required"
    );
    assert_eq!(
        state.progress().diagnostics()[0].detail.as_deref(),
        Some(diagnostic_detail.as_str())
    );

    insta::assert_snapshot!(buffer_text(&render_to_buffer(
        &state,
        started_at + Duration::from_secs(2),
        60,
        20,
    )));
}
