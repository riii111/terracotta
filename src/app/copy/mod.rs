use std::fmt::{Debug, Formatter};
use std::time::Duration;

use super::{
    execution::{Diagnostic, ExecutionStage, ExecutionState, SensitiveValue},
    review::PlanReview,
};

const REDACTION_TEXT: &str = "(sensitive value)";
const PROTECTED_REDACTION: &str = "\u{0}terracotta-redacted\u{0}";

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
            Self::Failed => "Copy failed.",
        }
    }

    #[must_use]
    pub(crate) const fn duration(self) -> Duration {
        match self {
            Self::Copied { .. } => Duration::from_secs(3),
            Self::Failed => Duration::from_secs(5),
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
    let mut text = diagnostic_text(review.diagnostics(), review.metadata().sensitive_values());
    if !text.is_empty() && !review.document().text().is_empty() {
        text.push('\n');
    }
    text.push_str(review.document().text());
    CopyEffect::new(CopyTarget::Plan, text)
}

#[must_use]
pub(crate) fn diagnostic_effect(
    diagnostics: &[Diagnostic],
    fallback: Option<&str>,
    sensitive_values: &[SensitiveValue],
) -> CopyEffect {
    let text = if diagnostics.is_empty() {
        sanitize_text(
            fallback.unwrap_or("Diagnostic unavailable."),
            sensitive_values,
        )
    } else {
        diagnostic_text(diagnostics, sensitive_values)
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
        _ => sections.push(format!("{} failed.", state.context().tool_name())),
    }
    if let Some(result) = state.result() {
        let log = state.progress().log();
        if let Some(summary) = result.summary_line()
            && !log
                .iter()
                .any(|line| line.text.lines().any(|text| text == summary))
        {
            sections.push(sanitize_text(summary, state.progress().sensitive_values()));
        }
        sections.extend(log.iter().map(|line| line.text.clone()));
    }
    CopyEffect::new(CopyTarget::Execution, sections.join("\n"))
}

pub(crate) fn sanitize_text(text: &str, sensitive_values: &[SensitiveValue]) -> String {
    let mut values = sensitive_values
        .iter()
        .filter(|value| match value {
            SensitiveValue::Text(value) | SensitiveValue::Number(value) => !value.is_empty(),
            SensitiveValue::Bool(_) => true,
        })
        .collect::<Vec<_>>();
    values.sort_by_key(|value| std::cmp::Reverse(sensitive_value_text(value).len()));
    values.dedup();

    let sanitized = values.into_iter().fold(
        text.replace(REDACTION_TEXT, PROTECTED_REDACTION),
        |text, value| match value {
            SensitiveValue::Text(value) if value.len() < 4 && !value.is_empty() => {
                transform_unmasked(&text, |text| redact_lines_containing(text, value))
            }
            SensitiveValue::Text(value) => {
                transform_unmasked(&text, |text| text.replace(value, PROTECTED_REDACTION))
            }
            SensitiveValue::Number(value) => {
                transform_unmasked(&text, |text| replace_scalar_tokens(text, value, true))
            }
            SensitiveValue::Bool(value) => transform_unmasked(&text, |text| {
                replace_scalar_tokens(text, if *value { "true" } else { "false" }, false)
            }),
        },
    );
    sanitized.replace(PROTECTED_REDACTION, REDACTION_TEXT)
}

fn transform_unmasked(text: &str, transform: impl Fn(&str) -> String) -> String {
    text.split(PROTECTED_REDACTION)
        .map(transform)
        .collect::<Vec<_>>()
        .join(PROTECTED_REDACTION)
}

fn redact_lines_containing(text: &str, value: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            if line.contains(value) {
                if line.ends_with('\n') {
                    format!("{PROTECTED_REDACTION}\n")
                } else {
                    PROTECTED_REDACTION.to_owned()
                }
            } else {
                line.to_owned()
            }
        })
        .collect()
}

fn sensitive_value_text(value: &SensitiveValue) -> &str {
    match value {
        SensitiveValue::Text(value) | SensitiveValue::Number(value) => value,
        SensitiveValue::Bool(value) if *value => "true",
        SensitiveValue::Bool(_) => "false",
    }
}

fn replace_scalar_tokens(text: &str, value: &str, numeric: bool) -> String {
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, _) in text.match_indices(value) {
        let end = start + value.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        let is_boundary = |character: Option<char>| {
            !character.is_some_and(|character| {
                character.is_ascii_alphanumeric()
                    || character == '_'
                    || (numeric && matches!(character, '.' | '-'))
            })
        };
        if !is_boundary(before) || !is_boundary(after) {
            continue;
        }
        result.push_str(&text[cursor..start]);
        result.push_str(PROTECTED_REDACTION);
        cursor = end;
    }
    result.push_str(&text[cursor..]);
    result
}

fn diagnostic_text(diagnostics: &[Diagnostic], sensitive_values: &[SensitiveValue]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let text = diagnostic.detail.as_ref().map_or_else(
                || diagnostic.summary.clone(),
                |detail| format!("{}\n{detail}", diagnostic.summary),
            );
            sanitize_text(&text, sensitive_values)
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
        review::{
            PlanBlock, PlanBlockKind, PlanMetadata,
            test_support::{plan_document, plan_document_with_blocks},
        },
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
                address: None,
                position: None,
                source: DiagnosticSource::Terraform,
            }],
        );

        let effect = plan_effect(&review);

        assert_eq!(effect.text(), "Provider warning\nTerraform plan body\n");
        assert!(!format!("{effect:?}").contains("Terraform plan body"));
    }

    #[test]
    fn plan_copy_preserves_the_complete_sanitized_show_text() {
        let source = "Terraform used the selected providers to generate the following execution\n"
            .to_owned()
            + "plan. Resource actions are indicated with the following symbols:\n\n"
            + "  # terraform_data.api will be created\n"
            + "  + resource \"terraform_data\" \"api\" {\n"
            + "      value = (sensitive value)\n"
            + "    }\n\n"
            + "Changes to Outputs:\n"
            + "  + endpoint = (known after apply)\n\n"
            + "Plan: 1 to add, 0 to change, 0 to destroy.\n";
        let end = source.split('\n').count();
        let review = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document_with_blocks(
                source.clone(),
                vec![PlanBlock::new(0..end, PlanBlockKind::Common)],
            ),
            PlanMetadata::new(
                vec!["terraform_data.api".to_owned()],
                vec!["endpoint".to_owned()],
                1,
                0,
                0,
                true,
            ),
            Vec::new(),
        );

        assert_eq!(plan_effect(&review).text(), source);
    }

    #[test]
    fn plan_copy_prefixes_diagnostics_without_rewriting_the_plan_text() {
        let source = "Plan: 0 to add, 0 to change, 0 to destroy.\n";
        let review = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document(source.to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
            vec![Diagnostic {
                severity: DiagnosticSeverity::Warning,
                summary: "Provider warning".to_owned(),
                detail: Some("warning detail".to_owned()),
                address: None,
                position: None,
                source: DiagnosticSource::Terraform,
            }],
        );

        assert_eq!(
            plan_effect(&review).text(),
            "Provider warning\nwarning detail\nPlan: 0 to add, 0 to change, 0 to destroy.\n"
        );
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

    #[test]
    fn scalar_sensitive_values_are_replaced_only_at_token_boundaries() {
        let text = "true feature=true id=1 total=10 version1";
        let sensitive = [
            SensitiveValue::Bool(true),
            SensitiveValue::Number("1".to_owned()),
        ];

        assert_eq!(
            sanitize_text(text, &sensitive),
            "(sensitive value) feature=(sensitive value) id=(sensitive value) total=10 version1"
        );
    }

    #[test]
    fn short_text_sensitive_values_redact_the_whole_affected_line() {
        let sensitive = [SensitiveValue::Text("abc".to_owned())];

        assert_eq!(
            sanitize_text("terraform_data.api\nrequest xabcx failed\nsafe", &sensitive),
            "terraform_data.api\n(sensitive value)\nsafe"
        );
    }

    #[test]
    fn sanitizing_already_redacted_text_is_idempotent() {
        let sensitive = [SensitiveValue::Text("value".to_owned())];
        let text = sanitize_text("value", &sensitive);

        assert_eq!(text, "(sensitive value)");
        assert_eq!(sanitize_text(&text, &sensitive), text);
    }
}
