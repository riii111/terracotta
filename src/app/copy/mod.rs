use std::fmt::{Debug, Formatter};

use super::{
    execution::{Diagnostic, ExecutionStage, ExecutionState},
    review::PlanReview,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyTarget {
    Diagnostic,
    Plan,
    Execution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyResult {
    Written,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyNotice {
    Copied { target: CopyTarget },
    Failed,
}

impl CopyNotice {
    #[must_use]
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Copied { .. } => "Copied.",
            Self::Failed => "Copy failed: clipboard unavailable.",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CopyEffect {
    target: CopyTarget,
    text: String,
}

impl CopyEffect {
    #[must_use]
    pub(crate) const fn new(target: CopyTarget, text: String) -> Self {
        Self { target, text }
    }

    #[must_use]
    pub(crate) const fn target(&self) -> CopyTarget {
        self.target
    }

    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

impl Debug for CopyEffect {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CopyEffect")
            .field("target", &self.target)
            .field("text", &"<redacted>")
            .finish()
    }
}

#[must_use]
pub(crate) fn plan_effect(review: &PlanReview) -> CopyEffect {
    let mut text = diagnostic_text(review.diagnostics());
    if !text.is_empty() && !review.document().text().is_empty() {
        text.push('\n');
    }
    text.push_str(review.document().text());
    CopyEffect::new(CopyTarget::Plan, text)
}

#[must_use]
pub(crate) fn diagnostic_effect(diagnostics: &[Diagnostic], fallback: Option<&str>) -> CopyEffect {
    let text = if diagnostics.is_empty() {
        fallback.unwrap_or("Diagnostic unavailable.").to_owned()
    } else {
        diagnostic_text(diagnostics)
    };
    CopyEffect::new(CopyTarget::Diagnostic, text)
}

#[must_use]
pub(crate) fn execution_effect(state: &ExecutionState) -> CopyEffect {
    let mut sections = Vec::new();
    match state.stage() {
        ExecutionStage::ApplySucceeded => sections.push("Apply complete.".to_owned()),
        ExecutionStage::ApplyInterrupted => {
            sections.push("Apply interrupted.".to_owned());
            sections.push("Changes may already be applied.".to_owned());
        }
        ExecutionStage::ApplyFailed => {
            sections.push("Apply failed.".to_owned());
            sections.push("Changes may already be applied.".to_owned());
        }
        _ => sections.push("Terraform failed.".to_owned()),
    }
    if let Some(result) = state.result() {
        let log = state.progress().log();
        if let Some(summary) = result.summary_line()
            && !log
                .iter()
                .any(|line| line.text.lines().any(|text| text == summary))
        {
            sections.push(summary.to_owned());
        }
        sections.extend(log.iter().map(|line| line.text.clone()));
    }
    CopyEffect::new(CopyTarget::Execution, sections.join("\n"))
}

fn diagnostic_text(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| {
            diagnostic.detail.as_ref().map_or_else(
                || diagnostic.summary.clone(),
                |detail| format!("{}\n{detail}", diagnostic.summary),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::app::{
        execution::{
            ApplyStatus, DiagnosticSeverity, DiagnosticSource, EventStream, ExecutionContext,
            ExecutionEvent, ExecutionEventKind, ExecutionLogLine,
        },
        review::{PlanMetadata, test_support::plan_document},
    };

    use super::*;

    #[test]
    fn plan_copy_contains_diagnostics_then_exact_standard_text() {
        let review = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document("Terraform plan body\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 1, 0, true),
            vec![Diagnostic {
                severity: DiagnosticSeverity::Warning,
                summary: "Provider warning".to_owned(),
                detail: None,
                position: None,
                source: DiagnosticSource::Terraform,
            }],
        );

        let effect = plan_effect(&review);

        assert_eq!(effect.text(), "Provider warning\nTerraform plan body\n");
        assert!(!format!("{effect:?}").contains("Terraform plan body"));
    }

    #[test]
    fn apply_copy_keeps_human_output_without_repeating_the_summary() {
        let now = std::time::Instant::now();
        let mut state = ExecutionState::applying(now, ExecutionContext::loading("/project"));
        let summary = "Apply complete! Resources: 1 added, 0 changed, 0 destroyed.";
        state.record(ExecutionEvent {
            received_at: now,
            kind: ExecutionEventKind::Log(ExecutionLogLine {
                stream: EventStream::Stdout,
                text: format!("Applying saved plan...\n{summary}"),
            }),
        });
        state.finish_apply(ApplyStatus::Succeeded, Some(summary.to_owned()), None, now);

        let effect = state
            .copy_effect(CopyTarget::Execution)
            .expect("apply result copy should be available");
        assert!(effect.text().contains("Applying saved plan..."));
        assert_eq!(effect.text().matches(summary).count(), 1);
    }
}
