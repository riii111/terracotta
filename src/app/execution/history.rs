use std::{
    fmt::Write as _,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use sha2::{Digest, Sha256};

use crate::app::plan::PlanAction;

use super::{ExecutionContext, ExecutionContextValue, ExecutionTargetSpec, Tool};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryKey {
    directory: PathBuf,
    workspace: String,
    address: String,
    actions: Vec<PlanAction>,
    tool: Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SuccessfulTarget {
    pub(crate) key: HistoryKey,
    pub(crate) duration: Duration,
}

impl HistoryKey {
    #[must_use]
    pub(crate) fn for_target(
        context: &ExecutionContext,
        target: &ExecutionTargetSpec,
    ) -> Option<Self> {
        let ExecutionContextValue::Known(workspace) = context.workspace() else {
            return None;
        };
        Some(Self {
            directory: normalize_directory(context.cwd_path()),
            workspace: workspace.clone(),
            address: target.address.clone(),
            actions: target.actions.clone(),
            tool: context.tool(),
        })
    }

    #[must_use]
    pub(crate) fn file_stem(&self) -> String {
        let mut hasher = Sha256::new();
        append_text(&mut hasher, &self.directory.display().to_string());
        append_text(&mut hasher, &self.workspace);
        append_text(&mut hasher, &self.address);
        hasher.update((self.actions.len() as u64).to_le_bytes());
        for action in &self.actions {
            append_action(&mut hasher, action);
        }
        append_text(&mut hasher, self.tool.display_name());

        let mut file_stem = String::with_capacity(64);
        for byte in hasher.finalize() {
            write!(file_stem, "{byte:02x}").expect("writing to a String cannot fail");
        }
        file_stem
    }
}

fn append_text(hasher: &mut Sha256, text: &str) {
    hasher.update((text.len() as u64).to_le_bytes());
    hasher.update(text.as_bytes());
}

fn append_action(hasher: &mut Sha256, action: &PlanAction) {
    match action {
        PlanAction::Create => append_text(hasher, "create"),
        PlanAction::Read => append_text(hasher, "read"),
        PlanAction::Update => append_text(hasher, "update"),
        PlanAction::Delete => append_text(hasher, "delete"),
        PlanAction::NoOp => append_text(hasher, "no-op"),
        PlanAction::Unknown(value) => {
            append_text(hasher, "unknown");
            append_text(hasher, value);
        }
    }
}

fn normalize_directory(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(_) | Component::Prefix(_) | Component::RootDir => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(
        directory: &str,
        workspace: &str,
        address: &str,
        actions: Vec<PlanAction>,
    ) -> HistoryKey {
        HistoryKey {
            directory: normalize_directory(Path::new(directory)),
            workspace: workspace.to_owned(),
            address: address.to_owned(),
            actions,
            tool: Tool::Terraform,
        }
    }

    #[test]
    fn key_components_are_part_of_the_digest() {
        let update_key =
            |workspace, address| key("/repo/infra", workspace, address, vec![PlanAction::Update]);
        let cases = [
            (
                "normalized_directory",
                key(
                    "/repo/infra/../infra",
                    "default",
                    "aws_vpc.main",
                    vec![PlanAction::Update],
                ),
                update_key("default", "aws_vpc.main"),
                true,
            ),
            (
                "workspace",
                update_key("staging", "aws_vpc.main"),
                update_key("production", "aws_vpc.main"),
                false,
            ),
            (
                "address",
                update_key("default", "aws_vpc.main"),
                update_key("default", "aws_vpc.worker"),
                false,
            ),
            (
                "action_sequence",
                key(
                    "/repo/infra",
                    "default",
                    "aws_vpc.main",
                    vec![PlanAction::Delete, PlanAction::Create],
                ),
                key(
                    "/repo/infra",
                    "default",
                    "aws_vpc.main",
                    vec![PlanAction::Create, PlanAction::Delete],
                ),
                false,
            ),
            (
                "tool",
                HistoryKey {
                    tool: Tool::Terraform,
                    ..update_key("default", "aws_vpc.main")
                },
                HistoryKey {
                    tool: Tool::OpenTofu,
                    ..update_key("default", "aws_vpc.main")
                },
                false,
            ),
        ];

        for (name, first, second, same_digest) in cases {
            assert_eq!(
                first.file_stem() == second.file_stem(),
                same_digest,
                "case: {name}"
            );
        }
    }

    #[test]
    fn target_key_uses_the_canonical_context_and_full_action_sequence() {
        let context = ExecutionContext::loading("/repo/infra/../infra")
            .with_workspace("default")
            .with_tool(Tool::OpenTofu);
        let target = ExecutionTargetSpec {
            address: "module.network.aws_vpc.main[\"blue\"]".to_owned(),
            actions: vec![PlanAction::Create, PlanAction::Delete],
        };

        let key = HistoryKey::for_target(&context, &target).expect("workspace is known");

        assert_eq!(key.directory, Path::new("/repo/infra"));
        assert_eq!(key.workspace, "default");
        assert_eq!(key.address, target.address);
        assert_eq!(key.actions, target.actions);
        assert_eq!(key.tool, Tool::OpenTofu);
    }
}
