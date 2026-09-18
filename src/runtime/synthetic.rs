use std::{
    io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::app::{
    attribution::{
        ResourceAddress, ResourceSourceLocation, SourceFileAnalysis, SourceLineChange, SourceRange,
        SourceSide, attribute_changes,
    },
    execution::{
        Diagnostic, DiagnosticSeverity, DiagnosticSource, ExecutionContext, ExecutionEvent,
        ExecutionEventKind, ExecutionState, ResourceEvent, ResourceEventKind,
    },
    plan::{
        Plan, PlanAction, PlanSummary, PlanValue, ResourceChange, ResourceChangeKind, ResourceMode,
        UnsupportedChange, UnsupportedChangeKind, UnsupportedChangeScope,
    },
    review::{
        PlanReview, PlanReviewMessage, ReviewComparison, ReviewComparisonBasis,
        ReviewComparisonStatus,
    },
};
use crate::infra::{CancellationToken, ClipboardExecutor};

use super::{event_loop, run_terminal};

pub(crate) fn run_synthetic() -> io::Result<()> {
    let (sender, receiver) = mpsc::channel();
    sender.send(synthetic_diagnostic()).map_err(|error| {
        io::Error::other(format!("failed to queue synthetic diagnostic: {error}"))
    })?;
    sender
        .send(PlanReviewMessage::Completed(synthetic_review()))
        .map_err(|error| io::Error::other(format!("failed to queue synthetic review: {error}")))?;
    let cancellation = CancellationToken::new();
    let mut clipboard = ClipboardExecutor::new();
    let context = ExecutionContext::known(
        "infra/prod",
        "default",
        "feature/synthetic-review",
        "working tree vs HEAD",
    );
    run_terminal(|terminal| {
        event_loop::run_connected(
            terminal,
            ExecutionState::with_context(Instant::now(), context.clone()),
            &receiver,
            &cancellation,
            &mut clipboard,
        )
        .map(|_| ())
    })
}

fn synthetic_diagnostic() -> PlanReviewMessage {
    PlanReviewMessage::Event(ExecutionEvent {
        received_at: Instant::now(),
        kind: ExecutionEventKind::Diagnostic(Diagnostic {
            severity: DiagnosticSeverity::Warning,
            summary: "Synthetic warning: review this plan".to_owned(),
            detail: Some("The synthetic plan completed successfully.".to_owned()),
            position: None,
            source: DiagnosticSource::Terraform,
            raw: None,
        }),
    })
}

pub(crate) fn run_synthetic_execution() -> io::Result<()> {
    let started_at = Instant::now();
    let (sender, receiver) = mpsc::channel();
    sender
        .send(synthetic_resource_event(
            started_at,
            "aws_vpc.main",
            ResourceEventKind::RefreshComplete,
        ))
        .map_err(|error| io::Error::other(format!("failed to queue synthetic event: {error}")))?;
    sender
        .send(synthetic_resource_event(
            started_at + Duration::from_millis(400),
            "aws_instance.api",
            ResourceEventKind::RefreshStart,
        ))
        .map_err(|error| io::Error::other(format!("failed to queue synthetic event: {error}")))?;

    let cancellation = CancellationToken::new();
    let worker_sender = sender;
    let worker_cancellation = cancellation.clone();
    let worker = thread::spawn(move || {
        while !worker_cancellation.is_cancelled() {
            thread::sleep(Duration::from_millis(25));
        }
        let _ = worker_sender.send(PlanReviewMessage::Failed {
            message: "synthetic execution cancelled".to_owned(),
            interrupted: true,
        });
    });

    let mut clipboard = ClipboardExecutor::new();
    let context = ExecutionContext::known(
        "infra/prod",
        "default",
        "feature/execution-ui",
        "working tree vs HEAD",
    );
    let result = run_terminal(|terminal| {
        event_loop::run_connected(
            terminal,
            ExecutionState::with_context(started_at, context.clone()),
            &receiver,
            &cancellation,
            &mut clipboard,
        )
        .map(|_| ())
    });
    if result.is_err() {
        cancellation.cancel();
    }
    let _ = worker.join();
    result
}

fn synthetic_resource_event(
    received_at: Instant,
    address: &str,
    kind: ResourceEventKind,
) -> PlanReviewMessage {
    PlanReviewMessage::Event(ExecutionEvent {
        received_at,
        kind: ExecutionEventKind::Resource(ResourceEvent {
            address: address.to_owned(),
            kind,
        }),
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the synthetic review fixture keeps its product-shaped data together"
)]
fn synthetic_review() -> PlanReview {
    let mut changes = vec![
        synthetic_change(
            "aws_instance.api",
            ResourceChangeKind::Update,
            PlanAction::Update,
        ),
        synthetic_change(
            "aws_s3_bucket.logs_with_a_very_long_resource_address_that_needs_truncation_for_narrow_terminal",
            ResourceChangeKind::Create,
            PlanAction::Create,
        ),
        synthetic_change(
            "aws_instance.worker",
            ResourceChangeKind::Replace,
            PlanAction::Delete,
        ),
        synthetic_change(
            "aws_security_group.old",
            ResourceChangeKind::Delete,
            PlanAction::Delete,
        ),
    ];
    let source_files = vec![
        SourceFileAnalysis::new(
            "main.tf".into(),
            SourceSide::After,
            vec![ResourceSourceLocation::new(
                ResourceAddress::new("aws_instance", "api"),
                "main.tf".into(),
                SourceSide::After,
                SourceRange::new(42, 46),
            )],
            Vec::new(),
        ),
        SourceFileAnalysis::new(
            "storage.tf".into(),
            SourceSide::After,
            vec![ResourceSourceLocation::new(
                ResourceAddress::new(
                    "aws_s3_bucket",
                    "logs_with_a_very_long_resource_address_that_needs_truncation_for_narrow_terminal",
                ),
                "storage.tf".into(),
                SourceSide::After,
                SourceRange::new(8, 10),
            )],
            Vec::new(),
        ),
        SourceFileAnalysis::new(
            "worker.tf".into(),
            SourceSide::After,
            vec![ResourceSourceLocation::new(
                ResourceAddress::new("aws_instance", "worker"),
                "worker.tf".into(),
                SourceSide::After,
                SourceRange::new(12, 18),
            )],
            Vec::new(),
        ),
        SourceFileAnalysis::new(
            "old.tf".into(),
            SourceSide::Before,
            vec![ResourceSourceLocation::new(
                ResourceAddress::new("aws_security_group", "old"),
                "old.tf".into(),
                SourceSide::Before,
                SourceRange::new(20, 24),
            )],
            Vec::new(),
        ),
    ];
    changes[2].mode = ResourceMode::Data;
    let changed_lines = vec![
        SourceLineChange::new("main.tf", SourceSide::After, SourceRange::new(42, 43)),
        SourceLineChange::new("storage.tf", SourceSide::After, SourceRange::new(8, 8)),
        SourceLineChange::new("worker.tf", SourceSide::After, SourceRange::new(14, 14)),
    ];
    let attributions = attribute_changes(&changes, &source_files, &changed_lines);
    PlanReview::new(
        "infra/prod".into(),
        "default".to_owned(),
        Plan {
            changes,
            summary: PlanSummary {
                creates: 1,
                updates: 1,
                replaces: 1,
                deletes: 1,
            },
            unsupported_changes: vec![UnsupportedChange {
                scope: UnsupportedChangeScope::Output,
                address: "output.synthetic".to_owned(),
                actions: vec![PlanAction::Update],
                kind: UnsupportedChangeKind::Output,
                reason: None,
                action_type: None,
            }],
        },
        source_files,
        attributions,
        ReviewComparison::new(
            ReviewComparisonBasis::WorkingTreeVsHead,
            None,
            None,
            None,
            None,
            ReviewComparisonStatus::Complete,
        ),
        Vec::new(),
    )
}

fn synthetic_change(address: &str, kind: ResourceChangeKind, action: PlanAction) -> ResourceChange {
    ResourceChange {
        address: address.to_owned(),
        mode: ResourceMode::Managed,
        actions: vec![action],
        kind,
        before: Some(PlanValue::Null),
        after: Some(PlanValue::Null),
        before_sensitive: None,
        after_sensitive: None,
        after_unknown: None,
        replace_paths: None,
        action_reason: None,
    }
}
