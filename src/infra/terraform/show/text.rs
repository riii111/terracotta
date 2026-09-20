use std::ops::Range;

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
    let mut candidates = Vec::new();
    let mut heredoc_terminator = None;
    let mut in_output_section = false;
    for (line, text) in lines.iter().enumerate() {
        if let Some(terminator) = &heredoc_terminator {
            if text.trim() == terminator {
                heredoc_terminator = None;
            }
            continue;
        }
        if *text == "Changes to Outputs:" {
            in_output_section = true;
        } else if text.starts_with("Plan:") {
            in_output_section = false;
        }
        if in_output_section {
            if let Some(name) = output_names.iter().find(|name| output_header(text, name)) {
                candidates.push((line, PlanBlockKind::Output(name.clone())));
            }
        } else if let Some(address) = resource_addresses
            .iter()
            .find(|address| resource_header(text, address))
        {
            candidates.push((line, PlanBlockKind::Resource(address.clone())));
        }
        heredoc_terminator = heredoc_start(text);
    }
    candidates.sort_by_key(|(line, _)| *line);

    let mut blocks = Vec::new();
    let mut cursor = 0;
    for (index, (start, kind)) in candidates.iter().enumerate() {
        if *start < cursor {
            continue;
        }
        if cursor < *start {
            push_block(&mut blocks, cursor..*start, PlanBlockKind::Common);
        }
        let end = block_end(&lines, *start, kind, candidates.get(index + 1));
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
    lines: &[&str],
    start: usize,
    kind: &PlanBlockKind,
    next_candidate: Option<&(usize, PlanBlockKind)>,
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
    let section_boundary = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(line, text)| match kind {
            PlanBlockKind::Resource(_) if *text == "Changes to Outputs:" => Some(line),
            PlanBlockKind::Output(_) if text.starts_with("Plan:") => Some(line),
            _ => None,
        });
    next_same_kind
        .into_iter()
        .chain(section_boundary)
        .min()
        .unwrap_or(lines.len())
}

fn resource_header(line: &str, address: &str) -> bool {
    let Some(rest) = line.strip_prefix("  # ") else {
        return false;
    };
    let Some(suffix) = rest.strip_prefix(address) else {
        return false;
    };
    suffix.starts_with(' ') && (suffix.contains(" will be ") || suffix.contains(" must be "))
}

fn output_header(line: &str, name: &str) -> bool {
    let Some(rest) = line.strip_prefix("  ") else {
        return false;
    };
    let Some(rest) = rest
        .strip_prefix('+')
        .or_else(|| rest.strip_prefix('-'))
        .or_else(|| rest.strip_prefix('~'))
    else {
        return false;
    };
    let Some((candidate, _)) = rest.trim_start().split_once('=') else {
        return false;
    };
    candidate.trim() == name
}

fn heredoc_start(line: &str) -> Option<String> {
    let marker = line.find("<<")?;
    let mut value = line[marker + 2..].trim_start();
    value = value.strip_prefix('-').unwrap_or(value).trim_start();
    let terminator = value.split_whitespace().next()?.trim_matches(['"', '\'']);
    (!terminator.is_empty()).then(|| terminator.to_owned())
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
        let document = parse_document(
            source.as_bytes().to_vec(),
            &[
                "terraform_data.api".to_owned(),
                "terraform_data.worker".to_owned(),
            ],
            &["endpoint".to_owned()],
        )
        .expect("text should parse");

        assert_eq!(document.blocks().len(), 5);
        assert_eq!(document.blocks()[1].lines(), &(2..9));
        assert_eq!(document.blocks()[2].lines(), &(9..10));
        assert_eq!(document.blocks()[3].lines(), &(10..12));
    }
}
