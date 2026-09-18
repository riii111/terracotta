use std::fmt::Write;

use crate::app::{
    attribution::{AnalysisIssue, AttributionStatus, ResourceAttribution, SourceRange, SourceSide},
    execution::{Diagnostic, DiagnosticSeverity, ExecutionContext},
    plan::{
        AttributeChangeKind, AttributeDiff, PlanAction, ReplacePathSegment, ResourceChange,
        ResourceChangeKind, ResourceMode, UnsupportedChange, UnsupportedChangeKind,
        diff_resource_attributes, format_attribute_path, format_replace_path,
    },
    review::{PlanReview, ReviewComparison},
};

pub(crate) fn resource_text(
    change: &ResourceChange,
    attribution: &ResourceAttribution,
    comparison: &ReviewComparison,
) -> String {
    let mut text = String::new();
    line(
        &mut text,
        format_args!("Resource {} {}", action_symbol(change.kind), change.address),
    );
    line(
        &mut text,
        format_args!("mode: {}", resource_mode(change.mode)),
    );
    line(
        &mut text,
        format_args!("actions: {}", format_actions(&change.actions)),
    );
    line(&mut text, format_args!("compare: {}", comparison.label()));
    append_comparison_status(&mut text, comparison);

    line(&mut text, format_args!("Git:"));
    append_attribution(&mut text, attribution);

    let diffs = diff_resource_attributes(change);
    line(&mut text, format_args!("Diff:"));
    append_attribute_diffs(
        &mut text,
        &diffs.attributes,
        diffs.changed_count,
        diffs.unchanged_count,
    );

    if let Some(paths) = diffs.replace_paths.as_deref() {
        line(
            &mut text,
            format_args!("replace paths: {}", format_replace_paths(paths)),
        );
    }
    if let Some(reason) = diffs.action_reason.as_deref() {
        line(&mut text, format_args!("replacement reason: {reason}"));
    }

    text
}

pub(crate) fn plan_text(review: &PlanReview) -> String {
    let mut text = String::new();
    line(&mut text, format_args!("Terracotta / Plan"));
    line(&mut text, format_args!("cwd: {}", review.root().display()));
    line(&mut text, format_args!("workspace: {}", review.workspace()));
    line(&mut text, format_args!("git: {}", review.git()));
    line(
        &mut text,
        format_args!("compare: {}", review.comparison().label()),
    );
    append_comparison_status(&mut text, review.comparison());

    let summary = review.plan().summary;
    line(
        &mut text,
        format_args!(
            "summary: {} create, {} update, {} replace, {} delete",
            summary.creates, summary.updates, summary.replaces, summary.deletes
        ),
    );
    line(
        &mut text,
        format_args!(
            "needs review: {} / {}",
            review.needs_review_count(),
            review.plan().changes.len()
        ),
    );

    if review.plan().changes.is_empty() {
        line(&mut text, format_args!("Resources: none"));
    } else {
        line(&mut text, format_args!("Resources:"));
        for (index, change) in review.plan().changes.iter().enumerate() {
            if let Some(attribution) = review
                .attributions()
                .get(index)
                .filter(|attribution| attribution.address() == change.address)
            {
                append_indented(
                    &mut text,
                    &resource_text(change, attribution, review.comparison()),
                    "  ",
                );
            } else {
                line(
                    &mut text,
                    format_args!(
                        "  Resource {} {}",
                        action_symbol(change.kind),
                        change.address
                    ),
                );
                line(&mut text, format_args!("    Git: attribution unavailable"));
            }
        }
    }

    append_unsupported_changes(&mut text, &review.plan().unsupported_changes);
    append_analysis_issues(&mut text, review);
    text
}

pub(crate) fn diagnostic_text(diagnostic: &Diagnostic) -> String {
    let mut text = String::new();
    line(
        &mut text,
        format_args!(
            "{}: {}",
            diagnostic_severity(diagnostic.severity),
            diagnostic.summary
        ),
    );
    if let Some(detail) = diagnostic.detail.as_deref() {
        for detail_line in detail.lines() {
            line(&mut text, format_args!("  {detail_line}"));
        }
    }
    if let Some(position) = &diagnostic.position {
        line(
            &mut text,
            format_args!(
                "  at {}:{}:{}",
                position.filename, position.start.line, position.start.column
            ),
        );
    }
    text
}

pub(crate) fn failed_diagnostic_text(message: Option<&str>, diagnostics: &[Diagnostic]) -> String {
    let mut text = String::new();
    if diagnostics.is_empty() {
        if let Some(message) = message {
            line(&mut text, format_args!("Diagnostic:"));
            append_indented(&mut text, message, "  ");
        } else {
            line(&mut text, format_args!("Diagnostic unavailable."));
        }
    } else {
        line(
            &mut text,
            format_args!("Diagnostics ({}):", diagnostics.len()),
        );
        for diagnostic in diagnostics {
            append_indented(&mut text, &diagnostic_text(diagnostic), "  ");
        }
        if let Some(message) = message
            && !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.summary == message)
        {
            line(&mut text, format_args!("Additional failure:"));
            append_indented(&mut text, message, "  ");
        }
    }
    text
}

pub(crate) fn failed_text(
    context: &ExecutionContext,
    message: Option<&str>,
    diagnostics: &[Diagnostic],
) -> String {
    let mut text = String::new();
    line(&mut text, format_args!("Terracotta / Failed"));
    line(
        &mut text,
        format_args!(
            "cwd {}   workspace {}",
            context.cwd().as_str(),
            context.workspace().as_str()
        ),
    );
    line(
        &mut text,
        format_args!(
            "git {}   compare {}",
            context.git().as_str(),
            context.comparison().as_str()
        ),
    );
    line(&mut text, format_args!("Terraform plan failed."));

    if diagnostics.is_empty() {
        if let Some(message) = message {
            line(&mut text, format_args!("Diagnostic:"));
            append_indented(&mut text, message, "  ");
        } else {
            line(&mut text, format_args!("Diagnostic unavailable."));
        }
    } else {
        line(
            &mut text,
            format_args!("Diagnostics ({}):", diagnostics.len()),
        );
        for diagnostic in diagnostics {
            append_indented(&mut text, &diagnostic_text(diagnostic), "  ");
        }
        if let Some(message) = message
            && !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.summary == message)
        {
            line(&mut text, format_args!("Additional failure:"));
            append_indented(&mut text, message, "  ");
        }
    }

    line(&mut text, format_args!("Review result is unavailable."));
    text
}

fn append_comparison_status(text: &mut String, comparison: &ReviewComparison) {
    if let Some(message) = comparison.status().message() {
        line(text, format_args!("analysis: incomplete: {message}"));
    }
}

fn append_attribution(text: &mut String, attribution: &ResourceAttribution) {
    match attribution.status() {
        AttributionStatus::Direct => line(text, format_args!("  direct")),
        AttributionStatus::NoMatch => {
            line(text, format_args!("  no match"));
            line(text, format_args!("  No direct match in analyzed sources."));
        }
    }

    for evidence in attribution.evidence() {
        line(
            text,
            format_args!(
                "  evidence: {} {}",
                source_side(evidence.side()),
                format_source_location(evidence.path(), evidence.range())
            ),
        );
    }

    if !attribution.analysis().is_complete() {
        line(text, format_args!("  analysis: incomplete"));
        for issue in attribution.analysis().issues() {
            line(
                text,
                format_args!("  reason: {}", format_analysis_issue(issue)),
            );
        }
    }
}

fn append_attribute_diffs(
    text: &mut String,
    attributes: &[AttributeDiff],
    changed_count: usize,
    unchanged_count: usize,
) {
    if changed_count == 0 {
        line(text, format_args!("  no changed attributes"));
    } else {
        for attribute in attributes
            .iter()
            .filter(|attribute| attribute.kind == AttributeChangeKind::Changed)
        {
            line(
                text,
                format_args!("  {}:", format_attribute_path(&attribute.path)),
            );
            line(
                text,
                format_args!("    before: {}", attribute.before.display()),
            );
            line(
                text,
                format_args!("    after: {}", attribute.after.display()),
            );
        }
    }
    if unchanged_count > 0 {
        line(
            text,
            format_args!("  {unchanged_count} unchanged attributes omitted"),
        );
    }
}

fn append_unsupported_changes(text: &mut String, changes: &[UnsupportedChange]) {
    if changes.is_empty() {
        return;
    }

    line(text, format_args!("Unsupported changes:"));
    for change in changes {
        let mut detail = format!(
            "{} {}",
            unsupported_change_kind(change.kind),
            change.address
        );
        if !change.actions.is_empty() {
            write!(detail, " [{}]", format_actions(&change.actions))
                .expect("writing unsupported change details should not fail");
        }
        if let Some(reason) = change.reason.as_deref() {
            write!(detail, "; reason: {reason}")
                .expect("writing unsupported change details should not fail");
        }
        if let Some(action_type) = change.action_type.as_deref() {
            write!(detail, "; type: {action_type}")
                .expect("writing unsupported change details should not fail");
        }
        line(text, format_args!("  {detail}"));
    }
}

fn append_analysis_issues(text: &mut String, review: &PlanReview) {
    let mut issues = Vec::new();
    if let Some(message) = review.comparison().status().message() {
        issues.push(message.to_owned());
    }
    for issue in review.analysis_issues() {
        let formatted = format_analysis_issue(issue);
        if !issues.iter().any(|known| known == &formatted) {
            issues.push(formatted);
        }
    }
    for file in review.source_files() {
        for issue in file.issues() {
            let formatted = format!(
                "{} {}: {}",
                source_side(file.side()),
                file.path().display(),
                issue.message()
            );
            if !issues.iter().any(|known| known == &formatted) {
                issues.push(formatted);
            }
        }
    }

    if issues.is_empty() {
        return;
    }

    line(text, format_args!("Analysis incomplete:"));
    for issue in issues {
        line(text, format_args!("  {issue}"));
    }
}

fn append_indented(text: &mut String, value: &str, prefix: &str) {
    for line_value in value.lines() {
        line(text, format_args!("{prefix}{line_value}"));
    }
}

fn format_actions(actions: &[PlanAction]) -> String {
    actions
        .iter()
        .map(|action| match action {
            PlanAction::Create => "create".to_owned(),
            PlanAction::Read => "read".to_owned(),
            PlanAction::Update => "update".to_owned(),
            PlanAction::Delete => "delete".to_owned(),
            PlanAction::NoOp => "no-op".to_owned(),
            PlanAction::Unknown(action) => action.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_replace_paths(paths: &[Vec<ReplacePathSegment>]) -> String {
    paths
        .iter()
        .map(|path| format_replace_path(path))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_source_location(path: &std::path::Path, range: SourceRange) -> String {
    if range.start_line() == range.end_line() {
        format!("{}:{}", path.display(), range.start_line())
    } else {
        format!(
            "{}:{}-{}",
            path.display(),
            range.start_line(),
            range.end_line()
        )
    }
}

fn format_analysis_issue(issue: &AnalysisIssue) -> String {
    match (issue.side(), issue.path()) {
        (Some(side), Some(path)) => format!(
            "{} {}: {}",
            source_side(side),
            path.display(),
            issue.message()
        ),
        (None, Some(path)) => format!("{}: {}", path.display(), issue.message()),
        (Some(side), None) => format!("{}: {}", source_side(side), issue.message()),
        (None, None) => issue.message().to_owned(),
    }
}

const fn action_symbol(kind: ResourceChangeKind) -> &'static str {
    match kind {
        ResourceChangeKind::Create => "+",
        ResourceChangeKind::Update => "~",
        ResourceChangeKind::Replace => "R",
        ResourceChangeKind::Delete => "-",
    }
}

const fn resource_mode(mode: ResourceMode) -> &'static str {
    match mode {
        ResourceMode::Managed => "managed",
        ResourceMode::Data => "data",
    }
}

const fn source_side(side: SourceSide) -> &'static str {
    match side {
        SourceSide::Before => "before",
        SourceSide::After => "after",
    }
}

const fn unsupported_change_kind(kind: UnsupportedChangeKind) -> &'static str {
    match kind {
        UnsupportedChangeKind::Output => "output",
        UnsupportedChangeKind::Drift => "drift",
        UnsupportedChangeKind::Read => "read",
        UnsupportedChangeKind::Move => "move",
        UnsupportedChangeKind::Import => "import",
        UnsupportedChangeKind::UnknownAction => "unknown action",
        UnsupportedChangeKind::UnsupportedActions => "unsupported actions",
        UnsupportedChangeKind::Deferred => "deferred",
        UnsupportedChangeKind::ActionInvocation => "action invocation",
        UnsupportedChangeKind::DeferredActionInvocation => "deferred action invocation",
    }
}

const fn diagnostic_severity(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Unknown => "diagnostic",
    }
}

fn line(text: &mut String, arguments: std::fmt::Arguments<'_>) {
    text.write_fmt(arguments)
        .expect("writing copy text to a String should not fail");
    text.push('\n');
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::app::{
        attribution::{
            SourceFileAnalysis, SourceIssue, SourceIssueKind, attribute_changes,
            mark_analysis_incomplete,
        },
        execution::{DiagnosticPoint, DiagnosticPosition, DiagnosticSource},
        plan::{Plan, PlanSummary, PlanValue, UnsupportedChangeScope},
        review::{ReviewComparisonBasis, ReviewComparisonStatus},
    };

    fn plan_value(value: serde_json::Value) -> PlanValue {
        match value {
            serde_json::Value::Null => PlanValue::Null,
            serde_json::Value::Bool(value) => PlanValue::Bool(value),
            serde_json::Value::Number(value) => PlanValue::Number(value.to_string()),
            serde_json::Value::String(value) => PlanValue::String(value),
            serde_json::Value::Array(values) => {
                PlanValue::Array(values.into_iter().map(plan_value).collect())
            }
            serde_json::Value::Object(values) => PlanValue::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, plan_value(value)))
                    .collect(),
            ),
        }
    }

    fn change(
        address: &str,
        kind: ResourceChangeKind,
        before: serde_json::Value,
        after: serde_json::Value,
    ) -> ResourceChange {
        ResourceChange {
            address: address.to_owned(),
            mode: ResourceMode::Managed,
            actions: match kind {
                ResourceChangeKind::Create => vec![PlanAction::Create],
                ResourceChangeKind::Update => vec![PlanAction::Update],
                ResourceChangeKind::Replace => vec![PlanAction::Delete, PlanAction::Create],
                ResourceChangeKind::Delete => vec![PlanAction::Delete],
            },
            kind,
            before: Some(plan_value(before)),
            after: Some(plan_value(after)),
            before_sensitive: Some(PlanValue::Bool(false)),
            after_sensitive: Some(PlanValue::Bool(false)),
            after_unknown: Some(PlanValue::Bool(false)),
            replace_paths: Some(vec![vec![ReplacePathSegment::Attribute("name".to_owned())]]),
            action_reason: Some("replace_because_cannot_update".to_owned()),
        }
    }

    fn attribution(change: &ResourceChange) -> ResourceAttribution {
        attribute_changes(std::slice::from_ref(change), &[], &[])
            .into_iter()
            .next()
            .expect("one attribution should be created")
    }

    fn comparison(status: ReviewComparisonStatus) -> ReviewComparison {
        ReviewComparison::new(ReviewComparisonBasis::WorkingTreeVsHead, None, status)
    }

    #[test]
    fn resource_text_lists_all_change_kinds_and_review_evidence() {
        let changes = [
            change(
                "aws_vpc.create",
                ResourceChangeKind::Create,
                serde_json::json!(null),
                serde_json::json!({"id": "create"}),
            ),
            change(
                "aws_vpc.update",
                ResourceChangeKind::Update,
                serde_json::json!({"name": "old"}),
                serde_json::json!({"name": "new"}),
            ),
            change(
                "aws_vpc.replace",
                ResourceChangeKind::Replace,
                serde_json::json!({"name": "old"}),
                serde_json::json!({"name": "new"}),
            ),
            change(
                "aws_vpc.delete",
                ResourceChangeKind::Delete,
                serde_json::json!({"name": "old"}),
                serde_json::json!(null),
            ),
        ];

        let output = changes
            .iter()
            .map(|change| {
                resource_text(
                    change,
                    &attribution(change),
                    &comparison(ReviewComparisonStatus::Complete),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(output.contains("Resource + aws_vpc.create"));
        assert!(output.contains("Resource ~ aws_vpc.update"));
        assert!(output.contains("Resource R aws_vpc.replace"));
        assert!(output.contains("Resource - aws_vpc.delete"));
        assert!(output.contains("before: \"old\""));
        assert!(output.contains("replacement reason: replace_because_cannot_update"));
    }

    #[test]
    fn resource_text_masks_nested_sensitive_values_without_reveal_input() {
        let mut change = change(
            "aws_instance.api",
            ResourceChangeKind::Update,
            serde_json::json!({"credentials": {"token": "old-secret", "user": "alice"}}),
            serde_json::json!({"credentials": {"token": "new-secret", "user": "bob"}}),
        );
        change.before_sensitive = Some(plan_value(
            serde_json::json!({"credentials": {"token": true}}),
        ));
        change.after_sensitive = Some(plan_value(
            serde_json::json!({"credentials": {"token": true}}),
        ));

        let output = resource_text(
            &change,
            &attribution(&change),
            &comparison(ReviewComparisonStatus::Complete),
        );

        assert!(!output.contains("old-secret"));
        assert!(!output.contains("new-secret"));
        assert!(output.contains("<sensitive>"));
        assert!(output.contains("\"alice\""));
        assert!(output.contains("\"bob\""));
    }

    #[test]
    fn resource_text_keeps_special_attribute_keys_unambiguous() {
        let mut api_change = change(
            "aws_instance.api",
            ResourceChangeKind::Update,
            serde_json::json!({
                "tags": {
                    "service.name": "old-secret",
                    "service": {"name": "nested-old"}
                }
            }),
            serde_json::json!({
                "tags": {
                    "service.name": "new-secret",
                    "service": {"name": "nested-new"}
                }
            }),
        );

        api_change.before_sensitive = Some(plan_value(
            serde_json::json!({"tags": {"service.name": true}}),
        ));
        api_change.after_sensitive = Some(plan_value(
            serde_json::json!({"tags": {"service.name": true}}),
        ));

        let output = resource_text(
            &api_change,
            &attribution(&api_change),
            &comparison(ReviewComparisonStatus::Complete),
        );

        assert!(output.contains("tags[\"service.name\"]"));
        assert!(output.contains("tags.service.name"));
        assert!(!output.contains("old-secret"));
        assert!(!output.contains("new-secret"));
        assert!(output.contains("<sensitive>"));
    }

    #[test]
    fn resource_text_keeps_no_match_and_incomplete_reasons_distinct() {
        let change = change(
            "aws_instance.api",
            ResourceChangeKind::Update,
            serde_json::json!({"name": "old"}),
            serde_json::json!({"name": "new"}),
        );
        let mut attributions = [attribution(&change)];
        mark_analysis_incomplete(&mut attributions, &[AnalysisIssue::git("Git unavailable")]);

        let output = resource_text(
            &change,
            &attributions[0],
            &comparison(ReviewComparisonStatus::Incomplete(
                "comparison unavailable".to_owned(),
            )),
        );

        assert!(output.contains("no match"));
        assert!(output.contains("No direct match in analyzed sources."));
        assert!(output.contains("analysis: incomplete"));
        assert!(output.contains("Git unavailable"));
        assert!(output.contains("comparison unavailable"));
    }

    #[test]
    fn plan_text_includes_empty_plan_and_unsupported_changes() {
        let review = PlanReview::new(
            PathBuf::from("infra/prod"),
            "default".to_owned(),
            Plan {
                changes: Vec::new(),
                summary: PlanSummary::default(),
                unsupported_changes: vec![UnsupportedChange {
                    scope: UnsupportedChangeScope::Output,
                    address: "module.app.output".to_owned(),
                    actions: vec![PlanAction::Update],
                    kind: UnsupportedChangeKind::Output,
                    reason: None,
                    action_type: None,
                }],
            },
            vec![SourceFileAnalysis::new(
                PathBuf::from("broken.tf"),
                SourceSide::After,
                Vec::new(),
                vec![SourceIssue::new(
                    SourceIssueKind::SyntaxError,
                    "syntax error in broken.tf",
                )],
            )],
            Vec::new(),
            comparison(ReviewComparisonStatus::Complete),
            Vec::new(),
        );

        let output = plan_text(&review);

        assert!(output.contains("summary: 0 create, 0 update, 0 replace, 0 delete"));
        assert!(output.contains("Resources: none"));
        assert!(output.contains("Unsupported changes:"));
        assert!(output.contains("output module.app.output [update]"));
        assert!(output.contains("syntax error in broken.tf"));
        assert!(output.contains("after broken.tf: syntax error in broken.tf"));
    }

    #[test]
    fn plan_text_does_not_pair_mismatched_attribution_with_a_resource() {
        let api_change = change(
            "aws_instance.api",
            ResourceChangeKind::Update,
            serde_json::json!({"name": "old"}),
            serde_json::json!({"name": "new"}),
        );
        let wrong_change = change(
            "aws_instance.other",
            ResourceChangeKind::Update,
            serde_json::json!({"name": "wrong-old"}),
            serde_json::json!({"name": "wrong-new"}),
        );
        let review = PlanReview::new(
            PathBuf::from("infra/prod"),
            "default".to_owned(),
            Plan {
                changes: vec![api_change],
                summary: PlanSummary {
                    updates: 1,
                    ..PlanSummary::default()
                },
                unsupported_changes: Vec::new(),
            },
            Vec::new(),
            vec![attribution(&wrong_change)],
            comparison(ReviewComparisonStatus::Complete),
            Vec::new(),
        );

        let output = plan_text(&review);

        assert!(output.contains("Resource ~ aws_instance.api"));
        assert!(output.contains("attribution unavailable"));
        assert!(!output.contains("wrong-old"));
    }

    #[test]
    fn failed_text_formats_structured_diagnostic_fields() {
        let context = ExecutionContext::known(
            "infra/prod",
            "default",
            "feature/test",
            "working tree vs HEAD",
        );
        let diagnostic = Diagnostic {
            severity: DiagnosticSeverity::Error,
            summary: "Terraform initialization required.".to_owned(),
            detail: Some("Run terraform init, then try again.".to_owned()),
            position: Some(DiagnosticPosition {
                filename: "main.tf".to_owned(),
                start: DiagnosticPoint {
                    line: 3,
                    column: 4,
                    byte: None,
                },
                end: DiagnosticPoint {
                    line: 3,
                    column: 8,
                    byte: None,
                },
            }),
            source: DiagnosticSource::Terraform,
        };

        let output = failed_text(
            &context,
            Some("Terraform initialization required."),
            &[diagnostic],
        );

        assert!(output.contains("Terracotta / Failed"));
        assert!(output.contains("error: Terraform initialization required."));
        assert!(output.contains("Run terraform init, then try again."));
        assert!(output.contains("at main.tf:3:4"));
        assert!(output.contains("Review result is unavailable."));
    }
}
