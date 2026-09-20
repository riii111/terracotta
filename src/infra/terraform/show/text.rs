use std::{collections::HashMap, ops::Range};

use crate::app::review::{PlanBlock, PlanBlockKind, PlanDocument};

use super::PlanParseError;

pub(super) fn parse_document(
    bytes: Vec<u8>,
    resource_addresses: &[String],
    output_names: &[String],
) -> Result<PlanDocument, PlanParseError> {
    String::from_utf8(bytes)
        .map(|text| {
            let blocks = split_blocks(&text, resource_addresses, output_names);
            PlanDocument::with_blocks(text, blocks)
        })
        .map_err(|_| PlanParseError::InvalidUtf8)
}

fn split_blocks(
    text: &str,
    resource_addresses: &[String],
    output_names: &[String],
) -> Vec<PlanBlock> {
    let lines = text.split('\n').collect::<Vec<_>>();
    let mut resource_indices = HashMap::with_capacity(resource_addresses.len());
    for (index, address) in resource_addresses.iter().enumerate() {
        resource_indices.entry(address.as_str()).or_insert(index);
    }
    let mut output_indices = HashMap::with_capacity(output_names.len());
    for (index, name) in output_names.iter().enumerate() {
        output_indices.entry(name.as_str()).or_insert(index);
    }
    let mut candidates = Vec::new();
    let mut section_boundaries = Vec::new();
    let mut heredoc_terminator: Option<String> = None;
    let mut in_output_section = false;
    for (line, text) in lines.iter().enumerate() {
        if let Some(terminator) = &heredoc_terminator {
            if heredoc_end(text, terminator) {
                heredoc_terminator = None;
            }
            continue;
        }
        if *text == "Changes to Outputs:" {
            in_output_section = true;
            section_boundaries.push(line);
        } else if text.starts_with("Plan:") {
            in_output_section = false;
            section_boundaries.push(line);
        }
        if in_output_section {
            if let Some(index) = output_header(text, &output_indices)
                && let Some(name) = output_names.get(index)
            {
                candidates.push((line, PlanBlockKind::Output(name.clone())));
            }
        } else if let Some(index) = resource_header(text, &resource_indices)
            && let Some(address) = resource_addresses.get(index)
        {
            candidates.push((line, PlanBlockKind::Resource(address.clone())));
        }
        heredoc_terminator = heredoc_start(text);
    }

    let mut blocks = Vec::new();
    let mut cursor = 0;
    for (index, (start, kind)) in candidates.iter().enumerate() {
        if *start < cursor {
            continue;
        }
        if cursor < *start {
            push_block(&mut blocks, cursor..*start, PlanBlockKind::Common);
        }
        let end = block_end(
            lines.len(),
            *start,
            kind,
            candidates.get(index + 1),
            &section_boundaries,
        );
        if *start < end {
            push_block(&mut blocks, *start..end, kind.clone());
            cursor = end;
        }
    }
    if cursor < lines.len() {
        push_block(&mut blocks, cursor..lines.len(), PlanBlockKind::Common);
    }
    if blocks.is_empty() {
        blocks.push(PlanBlock::new(0..lines.len(), PlanBlockKind::Common));
    }
    blocks
}

fn push_block(blocks: &mut Vec<PlanBlock>, lines: Range<usize>, kind: PlanBlockKind) {
    if lines.is_empty() {
        return;
    }
    if matches!(kind, PlanBlockKind::Common) && blocks.last().is_some_and(PlanBlock::is_common) {
        if let Some(previous) = blocks.last_mut() {
            previous.lines_mut().end = lines.end;
        }
        return;
    }
    blocks.push(PlanBlock::new(lines, kind));
}

fn block_end(
    line_count: usize,
    start: usize,
    kind: &PlanBlockKind,
    next_candidate: Option<&(usize, PlanBlockKind)>,
    section_boundaries: &[usize],
) -> usize {
    let next_same_kind = next_candidate
        .filter(|(_, candidate_kind)| {
            matches!(
                (kind, candidate_kind),
                (PlanBlockKind::Resource(_), PlanBlockKind::Resource(_))
                    | (PlanBlockKind::Output(_), PlanBlockKind::Output(_))
            )
        })
        .map(|(line, _)| *line);
    let section_boundary = if matches!(kind, PlanBlockKind::Resource(_) | PlanBlockKind::Output(_))
    {
        section_boundaries
            .get(section_boundaries.partition_point(|line| *line <= start))
            .copied()
    } else {
        None
    };
    next_same_kind
        .into_iter()
        .chain(section_boundary)
        .min()
        .unwrap_or(line_count)
}

fn resource_header(line: &str, indices: &HashMap<&str, usize>) -> Option<usize> {
    let rest = line.strip_prefix("  # ")?;
    let is_action = rest.contains(" will be ")
        || rest.contains(" must be ")
        || rest.contains(" has moved to ")
        || rest.contains(" will no longer be managed ");
    if !is_action {
        return None;
    }
    rest.char_indices()
        .filter(|&(_, character)| matches!(character, ' ' | ','))
        .map(|(index, _)| &rest[..index])
        .filter_map(|prefix| indices.get(prefix).copied())
        .chain(
            rest.split_whitespace()
                .filter_map(|word| indices.get(word.trim_matches(',')).copied()),
        )
        .min()
}

fn output_header(line: &str, indices: &HashMap<&str, usize>) -> Option<usize> {
    let rest = line.strip_prefix("  ")?;
    let rest = rest
        .strip_prefix('+')
        .or_else(|| rest.strip_prefix('-'))
        .or_else(|| rest.strip_prefix('~'))?;
    let (candidate, _) = rest.trim_start().split_once('=')?;
    indices.get(candidate.trim()).copied()
}

fn heredoc_start(line: &str) -> Option<String> {
    let mut quoted = false;
    let mut escaped = false;
    let marker = line.char_indices().find_map(|(index, character)| {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            return None;
        }
        if character == '"' {
            quoted = true;
            return None;
        }
        (character == '<'
            && line[index..].starts_with("<<")
            && line[..index].trim_end().ends_with('='))
        .then_some(index)
    })?;
    let mut value = line[marker + 2..].trim_start();
    value = value.strip_prefix('-').unwrap_or(value).trim_start();
    let terminator = value.split_whitespace().next()?;
    (!terminator.is_empty()
        && terminator
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-')))
    .then(|| terminator.to_owned())
}

fn heredoc_end(line: &str, terminator: &str) -> bool {
    let trimmed = line.trim();
    if trimmed == terminator {
        return true;
    }
    trimmed
        .strip_prefix(terminator)
        .is_some_and(|suffix| suffix.trim_start().starts_with("->"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_text_order_newlines_and_sensitive_markers() {
        let source = "first\n  password = (sensitive value)\nlast\n";

        let document = parse_document(
            source.as_bytes().to_vec(),
            &["terraform_data.api".to_owned()],
            &[],
        )
        .expect("text should parse");

        assert_eq!(document.text(), source);
        assert!(!format!("{document:?}").contains("password"));
    }

    #[test]
    fn rejects_invalid_utf8_without_exposing_bytes() {
        assert_eq!(
            parse_document(vec![0xff], &[], &[]),
            Err(PlanParseError::InvalidUtf8)
        );
    }

    #[test]
    fn keeps_nested_heading_like_text_inside_the_resource_block() {
        let source = "Terraform will perform the following actions:\n\n  # terraform_data.api will be updated in-place\n  ~ resource \"terraform_data\" \"api\" {\n      value = <<EOF\n  # terraform_data.worker will be created\nEOF\n    }\n\nChanges to Outputs:\n  ~ endpoint = \"new\"\n\nPlan: 0 to add, 1 to change, 0 to destroy.\n";
        let blocks = split_blocks(
            source,
            &[
                "terraform_data.api".to_owned(),
                "terraform_data.worker".to_owned(),
            ],
            &["endpoint".to_owned()],
        );

        assert_eq!(blocks.len(), 5);
        assert_eq!(blocks[1].lines(), &(2..9));
        assert_eq!(blocks[2].lines(), &(9..10));
        assert_eq!(blocks[3].lines(), &(10..12));
    }

    #[test]
    fn keeps_plan_summary_outside_the_last_resource_block() {
        let source = "  # terraform_data.api will be updated in-place\n  ~ resource \"terraform_data\" \"api\" {\n      input = \"after\"\n    }\n\nPlan: 0 to add, 1 to change, 0 to destroy.\n";
        let blocks = split_blocks(source, &["terraform_data.api".to_owned()], &[]);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].lines(), &(0..5));
        assert_eq!(blocks[1].lines(), &(5..7));
    }

    #[test]
    fn ignores_shift_markers_inside_quoted_values() {
        let source = "  # terraform_data.api will be updated in-place\n  ~ resource \"terraform_data\" \"api\" {\n      input = \"a << b\"\n    }\n\n  # terraform_data.worker will be created\n  + resource \"terraform_data\" \"worker\" {\n      input = \"worker\"\n    }\n";
        let blocks = split_blocks(
            source,
            &[
                "terraform_data.api".to_owned(),
                "terraform_data.worker".to_owned(),
            ],
            &[],
        );

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].lines(), &(0..5));
        assert_eq!(blocks[1].lines(), &(5..10));
    }

    #[test]
    fn recognizes_moved_and_removed_resource_headers() {
        let source = "  # terraform_data.old has moved to terraform_data.new\n  ~ resource \"terraform_data\" \"new\" {\n      input = \"new\"\n    }\n\n  # terraform_data.removed will no longer be managed by Terraform, but will not be destroyed\n  - resource \"terraform_data\" \"removed\" {\n      input = \"removed\"\n    }\n";
        let blocks = split_blocks(
            source,
            &[
                "terraform_data.new".to_owned(),
                "terraform_data.removed".to_owned(),
            ],
            &[],
        );

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].lines(), &(0..5));
        assert_eq!(blocks[1].lines(), &(5..10));
    }

    #[test]
    fn indexes_resource_headers_without_prefix_collisions_or_unknown_matches() {
        let resource_addresses = [
            "terraform_data.api".to_owned(),
            "terraform_data.api_extra".to_owned(),
            "module.service[\"a, b\"]".to_owned(),
            "module.service[\"will be here\"]".to_owned(),
            "terraform_data.moved_new".to_owned(),
            "terraform_data.moved_old".to_owned(),
        ];
        let mut resource_indices = HashMap::new();
        for (index, address) in resource_addresses.iter().enumerate() {
            resource_indices.entry(address.as_str()).or_insert(index);
        }

        for (case_name, line, expected) in [
            (
                "prefix_collision",
                "  # terraform_data.api_extra will be created",
                Some(1),
            ),
            (
                "quoted_comma",
                "  # module.service[\"a, b\"] will be created",
                Some(2),
            ),
            (
                "quoted_action_phrase",
                "  # module.service[\"will be here\"] will be created",
                Some(3),
            ),
            (
                "moved_uses_metadata_order",
                "  # terraform_data.moved_old has moved to terraform_data.moved_new",
                Some(4),
            ),
            (
                "unknown_address",
                "  # terraform_data.unknown will be created",
                None,
            ),
            (
                "non_action_heading",
                "  # terraform_data.api is unchanged",
                None,
            ),
        ] {
            assert_eq!(
                resource_header(line, &resource_indices),
                expected,
                "case: {case_name}; line: {line}"
            );
        }

        let duplicate_addresses = ["terraform_data.duplicate", "terraform_data.duplicate"];
        let mut duplicate_indices = HashMap::new();
        for (index, address) in duplicate_addresses.iter().enumerate() {
            duplicate_indices.entry(*address).or_insert(index);
        }
        assert_eq!(
            resource_header(
                "  # terraform_data.duplicate will be created",
                &duplicate_indices
            ),
            Some(0)
        );
    }

    #[test]
    fn indexes_output_headers_with_the_existing_action_and_assignment_rules() {
        let output_names = ["endpoint".to_owned(), "endpoint_extra".to_owned()];
        let mut output_indices = HashMap::new();
        for (index, name) in output_names.iter().enumerate() {
            output_indices.entry(name.as_str()).or_insert(index);
        }

        for (case_name, line, expected) in [
            (
                "prefix_collision",
                "  + endpoint_extra = (known after apply)",
                Some(1),
            ),
            ("known_output", "  ~ endpoint = \"new\"", Some(0)),
            ("missing_assignment", "  + endpoint_extra", None),
            ("nested_name", "  + endpoint_extra.value = \"new\"", None),
            ("missing_action_marker", "  endpoint = \"new\"", None),
        ] {
            assert_eq!(
                output_header(line, &output_indices),
                expected,
                "case: {case_name}; line: {line}"
            );
        }

        let duplicate_names = ["duplicate", "duplicate"];
        let mut duplicate_indices = HashMap::new();
        for (index, name) in duplicate_names.iter().enumerate() {
            duplicate_indices.entry(*name).or_insert(index);
        }
        assert_eq!(
            output_header("  + duplicate = (known after apply)", &duplicate_indices),
            Some(0)
        );
    }

    #[test]
    fn preserves_indexed_resource_output_and_unknown_heading_block_ranges() {
        let source = "preamble\n  # terraform_data.moved_old has moved to terraform_data.moved_new\n  ~ resource \"terraform_data\" \"new\" {\n      value = \"new\"\n    }\n  # terraform_data.api_extra will be created\n  + resource \"terraform_data\" \"api_extra\" {\n      value = \"extra\"\n    }\n  # terraform_data.unknown will be created\nChanges to Outputs:\n  + endpoint_extra = (known after apply)\n  ~ endpoint = \"new\"\nPlan: 0 to add, 2 to change, 0 to destroy.\n";
        let blocks = split_blocks(
            source,
            &[
                "terraform_data.moved_new".to_owned(),
                "terraform_data.moved_old".to_owned(),
                "terraform_data.api_extra".to_owned(),
            ],
            &["endpoint".to_owned(), "endpoint_extra".to_owned()],
        );

        assert_eq!(blocks.len(), 7);
        assert_eq!(blocks[0].lines(), &(0..1));
        assert_eq!(blocks[1].lines(), &(1..5));
        assert_eq!(blocks[2].lines(), &(5..10));
        assert_eq!(blocks[3].lines(), &(10..11));
        assert_eq!(blocks[4].lines(), &(11..12));
        assert_eq!(blocks[5].lines(), &(12..13));
        assert_eq!(blocks[6].lines(), &(13..15));
    }

    #[test]
    fn recognizes_heredoc_termination_with_deleted_value() {
        let source = "  # terraform_data.api will be updated in-place\n  ~ resource \"terraform_data\" \"api\" {\n      value = <<-EOT\n      first\n      second\n      EOT -> null\n    }\n\n  # terraform_data.worker will be updated in-place\n  ~ resource \"terraform_data\" \"worker\" {\n      input = \"new\"\n    }\n\nPlan: 0 to add, 2 to change, 0 to destroy.\n";
        let blocks = split_blocks(
            source,
            &[
                "terraform_data.api".to_owned(),
                "terraform_data.worker".to_owned(),
            ],
            &[],
        );
        let document = parse_document(
            source.as_bytes().to_vec(),
            &[
                "terraform_data.api".to_owned(),
                "terraform_data.worker".to_owned(),
            ],
            &[],
        )
        .expect("text should parse");

        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].lines(), &(0..8));
        assert_eq!(blocks[1].lines(), &(8..13));
        assert_eq!(blocks[2].lines(), &(13..15));
        assert_eq!(
            document.filter("worker").lines(),
            vec![
                "  # terraform_data.worker will be updated in-place",
                "  ~ resource \"terraform_data\" \"worker\" {",
                "      input = \"new\"",
                "    }",
                "",
                "Plan: 0 to add, 2 to change, 0 to destroy.",
                "",
            ]
        );
    }
}
