use std::time::Instant;

use serde_json::{Map, Value};

use crate::app::execution::{
    Diagnostic, DiagnosticPoint, DiagnosticPosition, DiagnosticSeverity, DiagnosticSource,
    EventStream, ExecutionEvent, ExecutionEventKind, ExecutionSummary, ResourceEvent,
    ResourceEventKind,
};

#[derive(Default)]
pub(crate) struct TerraformEventParser {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl TerraformEventParser {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(
        &mut self,
        stream: EventStream,
        bytes: &[u8],
        received_at: Instant,
    ) -> Vec<ExecutionEvent> {
        let buffer = self.buffer_mut(stream);
        buffer.extend_from_slice(bytes);
        self.take_complete_lines(stream, received_at)
    }

    pub(crate) fn finish(
        &mut self,
        stream: EventStream,
        received_at: Instant,
    ) -> Vec<ExecutionEvent> {
        let buffer = self.buffer_mut(stream);
        if buffer.is_empty() {
            return Vec::new();
        }
        let line = std::mem::take(buffer);
        Self::parse_line(stream, &line, received_at)
            .into_iter()
            .collect()
    }

    const fn buffer_mut(&mut self, stream: EventStream) -> &mut Vec<u8> {
        match stream {
            EventStream::Stdout => &mut self.stdout,
            EventStream::Stderr => &mut self.stderr,
        }
    }

    fn take_complete_lines(
        &mut self,
        stream: EventStream,
        received_at: Instant,
    ) -> Vec<ExecutionEvent> {
        let mut lines = Vec::new();
        {
            let buffer = self.buffer_mut(stream);
            while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                let mut line = buffer.drain(..=newline).collect::<Vec<_>>();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                lines.push(line);
            }
        }
        lines
            .into_iter()
            .filter_map(|line| Self::parse_line(stream, &line, received_at))
            .collect()
    }

    fn parse_line(
        stream: EventStream,
        line: &[u8],
        received_at: Instant,
    ) -> Option<ExecutionEvent> {
        if line.iter().all(u8::is_ascii_whitespace) {
            return None;
        }

        let text = String::from_utf8_lossy(line);
        let kind = serde_json::from_str::<Value>(&text).map_or_else(
            |_| ExecutionEventKind::Diagnostic(non_json_diagnostic(stream, text.into_owned())),
            |value| parse_json_event(stream, &value),
        );
        Some(ExecutionEvent { received_at, kind })
    }
}

fn parse_json_event(stream: EventStream, value: &Value) -> ExecutionEventKind {
    let Some(object) = value.as_object() else {
        return ExecutionEventKind::Diagnostic(unknown_event_diagnostic(stream, None, None));
    };
    let event_type = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let message = object
        .get("@message")
        .and_then(Value::as_str)
        .map(str::to_owned);

    match event_type.as_deref() {
        Some("diagnostic") => parse_diagnostic(object).map_or_else(
            || {
                ExecutionEventKind::Diagnostic(unknown_event_diagnostic(
                    stream, event_type, message,
                ))
            },
            ExecutionEventKind::Diagnostic,
        ),
        Some("change_summary" | "summary") => parse_summary(object).map_or_else(
            || {
                ExecutionEventKind::Diagnostic(unknown_event_diagnostic(
                    stream, event_type, message,
                ))
            },
            ExecutionEventKind::Summary,
        ),
        Some(
            "version"
            | "log"
            | "initializing_modules_message"
            | "outputs"
            | "test_abstract"
            | "test_file"
            | "test_run"
            | "test_cleanup"
            | "test_summary"
            | "test_plan"
            | "test_state"
            | "test_interrupt"
            | "planned_action_invocation",
        ) => ExecutionEventKind::Informational {
            event_type: event_type.expect("matched event type should be present"),
            message,
        },
        Some(event_type) => resource_event_kind(event_type)
            .and_then(|kind| resource_address(object).map(|address| (kind, address)))
            .map_or_else(
                || {
                    ExecutionEventKind::Diagnostic(unknown_event_diagnostic(
                        stream,
                        Some(event_type.to_owned()),
                        message,
                    ))
                },
                |(kind, address)| ExecutionEventKind::Resource(ResourceEvent { address, kind }),
            ),
        None => ExecutionEventKind::Diagnostic(unknown_event_diagnostic(stream, None, message)),
    }
}

fn resource_event_kind(event_type: &str) -> Option<ResourceEventKind> {
    Some(match event_type {
        "refresh_start" => ResourceEventKind::RefreshStart,
        "refresh_complete" => ResourceEventKind::RefreshComplete,
        "apply_start" => ResourceEventKind::ApplyStart,
        "apply_progress" => ResourceEventKind::ApplyProgress,
        "apply_complete" => ResourceEventKind::ApplyComplete,
        "apply_errored" => ResourceEventKind::ApplyErrored,
        "provision_start" => ResourceEventKind::ProvisionStart,
        "provision_progress" => ResourceEventKind::ProvisionProgress,
        "provision_complete" => ResourceEventKind::ProvisionComplete,
        "provision_errored" => ResourceEventKind::ProvisionErrored,
        "import_start" => ResourceEventKind::ImportStart,
        "import_complete" => ResourceEventKind::ImportComplete,
        "ephemeral_op_start" => ResourceEventKind::EphemeralStart,
        "ephemeral_op_progress" => ResourceEventKind::EphemeralProgress,
        "ephemeral_op_complete" => ResourceEventKind::EphemeralComplete,
        "ephemeral_op_errored" => ResourceEventKind::EphemeralErrored,
        "resource_drift" => ResourceEventKind::ResourceDrift,
        "planned_change" => ResourceEventKind::PlannedChange,
        _ => return None,
    })
}

fn resource_address(object: &Map<String, Value>) -> Option<String> {
    ["hook", "change", "resource"]
        .iter()
        .find_map(|field| object.get(*field))
        .and_then(|value| resource_address_from_value(value, 0))
}

fn resource_address_from_value(value: &Value, depth: usize) -> Option<String> {
    if depth > 2 {
        return None;
    }
    let object = value.as_object()?;
    if let Some(address) = object.get("addr").and_then(Value::as_str) {
        return Some(address.to_owned());
    }
    ["resource", "hook", "change"]
        .iter()
        .find_map(|field| object.get(*field))
        .and_then(|nested| resource_address_from_value(nested, depth + 1))
}

fn parse_summary(object: &Map<String, Value>) -> Option<ExecutionSummary> {
    let changes = object
        .get("changes")
        .or_else(|| object.get("summary"))?
        .as_object()?;
    Some(ExecutionSummary {
        adds: optional_count(changes, "add"),
        changes: optional_count(changes, "change"),
        removes: optional_count(changes, "remove"),
        operation: changes
            .get("operation")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn optional_count(object: &Map<String, Value>, field: &str) -> Option<usize> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
}

fn parse_diagnostic(object: &Map<String, Value>) -> Option<Diagnostic> {
    let diagnostic = object.get("diagnostic")?.as_object()?;
    let summary = diagnostic
        .get("summary")
        .and_then(Value::as_str)
        .or_else(|| object.get("@message").and_then(Value::as_str))
        .unwrap_or("Terraform diagnostic")
        .to_owned();
    let detail = diagnostic
        .get("detail")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(Diagnostic {
        severity: diagnostic
            .get("severity")
            .and_then(Value::as_str)
            .or_else(|| object.get("@level").and_then(Value::as_str))
            .map_or(DiagnosticSeverity::Unknown, diagnostic_severity),
        summary,
        detail,
        position: diagnostic.get("range").and_then(parse_position),
        source: DiagnosticSource::Terraform,
    })
}

fn diagnostic_severity(severity: &str) -> DiagnosticSeverity {
    match severity {
        "error" => DiagnosticSeverity::Error,
        "warning" | "warn" => DiagnosticSeverity::Warning,
        "info" => DiagnosticSeverity::Info,
        _ => DiagnosticSeverity::Unknown,
    }
}

fn parse_position(value: &Value) -> Option<DiagnosticPosition> {
    let range = value.as_object()?;
    let filename = range.get("filename").and_then(Value::as_str)?.to_owned();
    Some(DiagnosticPosition {
        filename,
        start: parse_point(range.get("start")?)?,
        end: parse_point(range.get("end")?)?,
    })
}

fn parse_point(value: &Value) -> Option<DiagnosticPoint> {
    let point = value.as_object()?;
    Some(DiagnosticPoint {
        line: point.get("line").and_then(Value::as_u64)?,
        column: point.get("column").and_then(Value::as_u64)?,
        byte: point.get("byte").and_then(Value::as_u64),
    })
}

const fn non_json_diagnostic(stream: EventStream, text: String) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        summary: text,
        detail: None,
        position: None,
        source: DiagnosticSource::NonJson { stream },
    }
}

fn unknown_event_diagnostic(
    stream: EventStream,
    event_type: Option<String>,
    message: Option<String>,
) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        summary: message.unwrap_or_else(|| "Unknown Terraform event".to_owned()),
        detail: event_type
            .as_deref()
            .map(|event_type| format!("Event type: {event_type}")),
        position: None,
        source: DiagnosticSource::UnknownEvent { stream, event_type },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_split_refresh_events_by_resource_address() {
        let mut parser = TerraformEventParser::new();
        let first = br#"{"type":"refresh_start","hook":{"resource":{"addr":"aws_vpc.main"}}}
"#;
        let second = br#"{"type":"refresh_complete","hook":{"resource":{"addr":"aws_vpc.main"}}}
"#;

        let split = first.len() / 2;
        assert!(
            parser
                .push(EventStream::Stdout, &first[..split], Instant::now())
                .is_empty()
        );
        let first_events = parser.push(EventStream::Stdout, &first[split..], Instant::now());
        assert_eq!(first_events.len(), 1);
        assert_eq!(
            first_events[0].kind,
            ExecutionEventKind::Resource(ResourceEvent {
                address: "aws_vpc.main".to_owned(),
                kind: ResourceEventKind::RefreshStart,
            })
        );
        let events = parser.push(EventStream::Stdout, second, Instant::now());
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].kind,
            ExecutionEventKind::Resource(ResourceEvent {
                address: "aws_vpc.main".to_owned(),
                kind: ResourceEventKind::RefreshComplete,
            })
        );
    }

    #[test]
    fn preserves_interleaved_streams_and_waits_for_a_complete_line() {
        let mut parser = TerraformEventParser::new();
        let stdout = br#"{"type":"refresh_start","hook":{"resource":{"addr":"aws_vpc.main"}}}
"#;
        let stderr = b"provider warning";
        let timestamp = Instant::now();

        assert!(
            parser
                .push(EventStream::Stdout, &stdout[..20], timestamp)
                .is_empty()
        );
        let stdout_events = parser.push(EventStream::Stdout, &stdout[20..], timestamp);
        assert!(
            parser
                .push(EventStream::Stderr, stderr, timestamp)
                .is_empty()
        );
        let stderr_events = parser.finish(EventStream::Stderr, timestamp);
        assert!(matches!(
            stdout_events[0].kind,
            ExecutionEventKind::Resource(ResourceEvent {
                kind: ResourceEventKind::RefreshStart,
                ..
            })
        ));
        assert!(matches!(
            stderr_events[0].kind,
            ExecutionEventKind::Diagnostic(Diagnostic {
                source: DiagnosticSource::NonJson {
                    stream: EventStream::Stderr
                },
                ..
            })
        ));
        assert_eq!(stdout_events[0].received_at, timestamp);
    }

    #[test]
    fn parses_summary_and_diagnostic_position_without_inventing_progress_total() {
        let mut parser = TerraformEventParser::new();
        let summary = json!({
            "type": "change_summary",
            "changes": {"add": 2, "change": 2, "remove": 2, "operation": "plan"}
        })
        .to_string();
        let diagnostic = json!({
            "type": "diagnostic",
            "@level": "error",
            "diagnostic": {
                "severity": "error",
                "summary": "Invalid value",
                "detail": "The value is not valid.",
                "range": {
                    "filename": "main.tf",
                    "start": {"line": 4, "column": 2, "byte": 20},
                    "end": {"line": 4, "column": 8, "byte": 26}
                }
            }
        })
        .to_string();

        let summary_event = parser.push(
            EventStream::Stdout,
            format!("{summary}\n").as_bytes(),
            Instant::now(),
        );
        let diagnostic_event = parser.push(
            EventStream::Stdout,
            format!("{diagnostic}\n").as_bytes(),
            Instant::now(),
        );
        assert_eq!(
            summary_event[0].kind,
            ExecutionEventKind::Summary(ExecutionSummary {
                adds: Some(2),
                changes: Some(2),
                removes: Some(2),
                operation: Some("plan".to_owned()),
            })
        );
        let ExecutionEventKind::Diagnostic(diagnostic) = &diagnostic_event[0].kind else {
            panic!("expected a diagnostic event");
        };
        assert_eq!(diagnostic.summary, "Invalid value");
        assert_eq!(
            diagnostic.detail.as_deref(),
            Some("The value is not valid.")
        );
        assert_eq!(
            diagnostic
                .position
                .as_ref()
                .map(|position| position.filename.as_str()),
            Some("main.tf")
        );
    }

    #[test]
    fn preserves_long_non_json_diagnostic_text() {
        let mut parser = TerraformEventParser::new();
        let long_text = "x".repeat(100_000);
        let stderr_events = parser.push(
            EventStream::Stderr,
            format!("{long_text}\n").as_bytes(),
            Instant::now(),
        );

        let ExecutionEventKind::Diagnostic(diagnostic) = &stderr_events[0].kind else {
            panic!("expected a stderr diagnostic");
        };
        assert_eq!(diagnostic.summary.len(), long_text.len());
        assert!(diagnostic.detail.is_none());
        assert!(
            Instant::now().duration_since(stderr_events[0].received_at) < Duration::from_secs(1)
        );
    }

    #[rstest]
    #[case::unknown_type(
        r#"{"@message":"Future event occurred","type":"future_event"}"#,
        "Future event occurred",
        Some("Event type: future_event"),
        Some("future_event")
    )]
    #[case::missing_type(
        r#"{"@message":"Event type is missing"}"#,
        "Event type is missing",
        None,
        None
    )]
    #[case::malformed_diagnostic(
        r#"{"@message":"Malformed diagnostic","type":"diagnostic"}"#,
        "Malformed diagnostic",
        Some("Event type: diagnostic"),
        Some("diagnostic")
    )]
    fn classifies_unusable_json_events_as_diagnostics(
        #[case] input: &str,
        #[case] expected_summary: &str,
        #[case] expected_detail: Option<&str>,
        #[case] expected_event_type: Option<&str>,
    ) {
        let mut parser = TerraformEventParser::new();
        let events = parser.push(
            EventStream::Stdout,
            format!("{input}\n").as_bytes(),
            Instant::now(),
        );

        let ExecutionEventKind::Diagnostic(diagnostic) = &events[0].kind else {
            panic!("expected an unusable JSON event diagnostic");
        };
        assert_eq!(diagnostic.summary, expected_summary);
        assert_eq!(diagnostic.detail.as_deref(), expected_detail);
        assert_eq!(
            diagnostic.source,
            DiagnosticSource::UnknownEvent {
                stream: EventStream::Stdout,
                event_type: expected_event_type.map(str::to_owned),
            }
        );
    }

    #[test]
    fn treats_known_non_resource_messages_as_informational() {
        let mut parser = TerraformEventParser::new();
        let events = parser.push(
            EventStream::Stdout,
            br#"{"@message":"Terraform 1.9.0","type":"version"}
"#,
            Instant::now(),
        );

        let ExecutionEventKind::Informational {
            event_type,
            message: Some(message),
        } = &events[0].kind
        else {
            panic!("expected a known informational event");
        };
        assert_eq!(event_type, "version");
        assert_eq!(message, "Terraform 1.9.0");

        let events = parser.push(
            EventStream::Stdout,
            br#"{"@level":"info","@message":"provider output","type":"log"}
{"@message":"action invoked","type":"planned_action_invocation","invocation":{"action_addr":{"addr":"action.example.main"}}}
"#,
            Instant::now(),
        );
        assert!(matches!(
            &events[0].kind,
            ExecutionEventKind::Informational { event_type, .. }
                if event_type == "log"
        ));
        assert!(matches!(
            &events[1].kind,
            ExecutionEventKind::Informational { event_type, .. }
                if event_type == "planned_action_invocation"
        ));
    }
}
