use std::fmt::{Debug, Formatter};

use super::{execution::Diagnostic, review::PlanReview};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyTarget {
    Diagnostic,
    Plan,
    Result,
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
        execution::{DiagnosticSeverity, DiagnosticSource},
        review::{PlanDocument, PlanMetadata},
    };

    use super::*;

    #[test]
    fn plan_copy_contains_diagnostics_then_exact_standard_text() {
        let review = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            PlanDocument::new("Terraform plan body\n".to_owned()),
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
}
