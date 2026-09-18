use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::{
    app::{
        attribution::SourceLineChange,
        attribution::{SourceRange, SourceSide},
    },
    infra::terraform::hcl::HclSourceFile,
};

use super::{
    super::command::{GitCommandError, checked_git, parse_error},
    name_status::{ChangedFile, FileChangeKind, is_direct_config_path},
};

type LineRange = (usize, usize);

pub(super) fn changed_lines(
    repository_root: &Path,
    root: &Path,
    root_spec: &Path,
) -> Result<Vec<SourceLineChange>, GitCommandError> {
    let output = checked_git(
        repository_root,
        "read Git diff hunks",
        [
            OsStr::new("-c"),
            OsStr::new("core.quotePath=false"),
            OsStr::new("diff"),
            OsStr::new("--no-ext-diff"),
            OsStr::new("--no-renames"),
            OsStr::new("--unified=0"),
            OsStr::new("--no-color"),
            OsStr::new("--src-prefix=a/"),
            OsStr::new("--dst-prefix=b/"),
            OsStr::new("HEAD"),
            OsStr::new("--"),
            root_spec.as_os_str(),
        ],
    )?;
    parse_diff_hunks(&output.stdout, repository_root, root)
}

pub(super) fn changed_lines_between(
    repository_root: &Path,
    root: &Path,
    root_spec: &Path,
    before_revision: &str,
    after_revision: &str,
) -> Result<Vec<SourceLineChange>, GitCommandError> {
    let output = checked_git(
        repository_root,
        "read Git commit diff hunks",
        [
            OsStr::new("-c"),
            OsStr::new("core.quotePath=false"),
            OsStr::new("diff"),
            OsStr::new("--no-ext-diff"),
            OsStr::new("--no-renames"),
            OsStr::new("--unified=0"),
            OsStr::new("--no-color"),
            OsStr::new("--src-prefix=a/"),
            OsStr::new("--dst-prefix=b/"),
            OsStr::new(before_revision),
            OsStr::new(after_revision),
            OsStr::new("--"),
            root_spec.as_os_str(),
        ],
    )?;
    parse_diff_hunks(&output.stdout, repository_root, root)
}

pub(super) fn add_missing_added_line_ranges(
    changes: &mut Vec<SourceLineChange>,
    after: &[HclSourceFile],
    files: &[ChangedFile],
) {
    for file in files {
        if file.kind != FileChangeKind::Added {
            continue;
        }
        let Some(source) = after
            .iter()
            .find(|source| source.path().ends_with(&file.path))
        else {
            continue;
        };
        if changes
            .iter()
            .any(|change| change.side() == SourceSide::After && change.path() == source.path())
        {
            continue;
        }
        let line_count = source.source().lines().count();
        if line_count > 0 {
            changes.push(SourceLineChange::new(
                source.path().to_owned(),
                SourceSide::After,
                SourceRange::new(1, line_count),
            ));
        }
    }
}

fn parse_diff_hunks(
    output: &[u8],
    repository_root: &Path,
    root: &Path,
) -> Result<Vec<SourceLineChange>, GitCommandError> {
    let text = String::from_utf8(output.to_owned()).map_err(|error| {
        parse_error(
            "parse Git diff hunks",
            &format!("diff is not valid UTF-8: {error}"),
        )
    })?;
    let mut old_path = None;
    let mut new_path = None;
    let mut in_file_header = false;
    let mut changes = Vec::new();

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            old_path = None;
            new_path = None;
            in_file_header = true;
            continue;
        }
        if in_file_header {
            if let Some(path) = line.strip_prefix("--- ") {
                old_path = parse_patch_path(path, b'a', repository_root)?;
                continue;
            }
            if let Some(path) = line.strip_prefix("+++ ") {
                new_path = parse_patch_path(path, b'b', repository_root)?;
                continue;
            }
        }
        let Some(header) = line.strip_prefix("@@ ") else {
            continue;
        };
        in_file_header = false;
        let (old_range, new_range) = parse_hunk_ranges(header)?;
        if let Some(path) = old_path.as_deref().or(new_path.as_deref()) {
            let absolute_path = path.to_owned();
            if !is_direct_config_path(repository_root, root, path) {
                continue;
            }
            add_line_change(
                &mut changes,
                absolute_path.clone(),
                SourceSide::Before,
                old_range,
            );
            add_line_change(&mut changes, absolute_path, SourceSide::After, new_range);
        }
    }
    Ok(changes)
}

fn parse_patch_path(
    path: &str,
    prefix: u8,
    repository_root: &Path,
) -> Result<Option<PathBuf>, GitCommandError> {
    let decoded = decode_git_path(path)?;
    if decoded == "/dev/null" {
        return Ok(None);
    }
    let prefix = format!("{}/", char::from(prefix));
    let Some(relative) = decoded.strip_prefix(&prefix) else {
        return Err(parse_error(
            "parse Git diff hunks",
            &format!("unexpected diff path: {decoded}"),
        ));
    };
    Ok(Some(repository_root.join(relative)))
}

fn decode_git_path(path: &str) -> Result<String, GitCommandError> {
    let path = path.split_once('\t').map_or(path, |(path, _)| path);
    let path = path.strip_suffix('\r').unwrap_or(path);
    if !path.starts_with('"') {
        return Ok(path.to_owned());
    }
    let bytes = path.as_bytes();
    if bytes.len() < 2 || bytes[bytes.len() - 1] != b'"' {
        return Err(parse_error(
            "parse Git diff hunks",
            "unterminated quoted path",
        ));
    }
    let mut decoded = Vec::new();
    let mut index = 1;
    while index < bytes.len() - 1 {
        if bytes[index] != b'\\' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        index += 1;
        let Some(&escaped) = bytes.get(index) else {
            return Err(parse_error(
                "parse Git diff hunks",
                "unterminated path escape",
            ));
        };
        match escaped {
            b'a' => decoded.push(7),
            b'b' => decoded.push(8),
            b't' => decoded.push(b'\t'),
            b'n' => decoded.push(b'\n'),
            b'v' => decoded.push(11),
            b'f' => decoded.push(12),
            b'r' => decoded.push(b'\r'),
            b'\\' | b'"' => decoded.push(escaped),
            b'0'..=b'7' => {
                let mut value = escaped - b'0';
                let mut digits = 1;
                while digits < 3
                    && bytes
                        .get(index + 1)
                        .is_some_and(|byte| (b'0'..=b'7').contains(byte))
                {
                    index += 1;
                    value = value * 8 + bytes[index] - b'0';
                    digits += 1;
                }
                decoded.push(value);
            }
            _ => {
                return Err(parse_error("parse Git diff hunks", "unknown path escape"));
            }
        }
        index += 1;
    }
    String::from_utf8(decoded).map_err(|error| {
        parse_error(
            "parse Git diff hunks",
            &format!("path is not valid UTF-8: {error}"),
        )
    })
}

fn parse_hunk_ranges(header: &str) -> Result<(LineRange, LineRange), GitCommandError> {
    let ranges = header
        .split_once(" @@")
        .map_or(header, |(ranges, _)| ranges);
    let mut fields = ranges.split_whitespace();
    let old = fields.next().and_then(|field| field.strip_prefix('-'));
    let new = fields.next().and_then(|field| field.strip_prefix('+'));
    match (
        old.and_then(parse_line_range),
        new.and_then(parse_line_range),
    ) {
        (Some(old), Some(new)) => Ok((old, new)),
        _ => Err(parse_error(
            "parse Git diff hunks",
            &format!("invalid hunk header: @@ {header}"),
        )),
    }
}

fn parse_line_range(range: &str) -> Option<(usize, usize)> {
    let (start, count) = range
        .split_once(',')
        .map_or((range, "1"), |(start, count)| (start, count));
    Some((start.parse().ok()?, count.parse().ok()?))
}

fn add_line_change(
    changes: &mut Vec<SourceLineChange>,
    path: PathBuf,
    side: SourceSide,
    (start, count): (usize, usize),
) {
    if count == 0 {
        return;
    }
    let Some(end) = start.checked_add(count - 1) else {
        return;
    };
    changes.push(SourceLineChange::new(
        path,
        side,
        SourceRange::new(start, end),
    ));
}
