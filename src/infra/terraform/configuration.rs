use std::{ffi::OsStr, fs, io, path::Path};

use crate::app::execution::Tool;
use hcl::Body;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionLocation {
    Local,
    HcpCandidate,
}

#[cfg(test)]
fn execution_location(root: &Path, data_dir: Option<&OsStr>) -> io::Result<ExecutionLocation> {
    execution_location_for_tool(root, Tool::Terraform, data_dir)
}

pub(crate) fn execution_location_for_tool(
    root: &Path,
    tool: Tool,
    data_dir: Option<&OsStr>,
) -> io::Result<ExecutionLocation> {
    let mut hcp = false;
    for path in configuration_files(root, tool)? {
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            return Err(invalid_configuration());
        };
        if name.starts_with('.') || name.ends_with('~') || name.starts_with('#') {
            continue;
        }
        if name.ends_with(".tf.json") || (tool == Tool::OpenTofu && name.ends_with(".tofu.json")) {
            let source = fs::read_to_string(&path)?;
            let value: Value =
                serde_json::from_str(&source).map_err(|_| invalid_configuration())?;
            hcp |= json_hcp(&value)?;
        } else if path.extension().is_some_and(|extension| {
            extension == "tf" || (tool == Tool::OpenTofu && extension == "tofu")
        }) {
            let source = fs::read_to_string(&path)?;
            let body: Body = hcl::from_str(&source).map_err(|_| invalid_configuration())?;
            hcp |= body
                .blocks()
                .filter(|block| block.identifier() == "terraform")
                .any(|block| {
                    block.body.blocks().any(|block| {
                        block.identifier() == "cloud"
                            || (block.identifier() == "backend"
                                && block
                                    .labels()
                                    .first()
                                    .is_some_and(|label| label.as_str() == "remote"))
                    })
                });
        }
    }
    let data_dir = data_dir
        .filter(|value| !value.is_empty())
        .map_or_else(|| root.join(".terraform"), |value| root.join(value));
    match fs::read(data_dir.join("terraform.tfstate")) {
        Ok(bytes) => {
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| invalid_configuration())?;
            let object = value.as_object().ok_or_else(invalid_configuration)?;
            if let Some(backend) = object.get("backend").filter(|value| !value.is_null()) {
                let kind = backend
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid_configuration)?;
                hcp |= matches!(kind, "remote" | "cloud");
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(if hcp {
        ExecutionLocation::HcpCandidate
    } else {
        ExecutionLocation::Local
    })
}

#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "Terraform and OpenTofu only recognize their lowercase configuration extensions"
)]
fn configuration_files(root: &Path, tool: Tool) -> io::Result<Vec<std::path::PathBuf>> {
    let mut paths = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    if tool == Tool::Terraform {
        return Ok(paths
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(OsStr::to_str)
                    .is_some_and(|name| name.ends_with(".tf") || name.ends_with(".tf.json"))
            })
            .collect());
    }

    let names = paths
        .iter()
        .filter_map(|path| path.file_name().and_then(OsStr::to_str))
        .map(str::to_owned)
        .collect::<std::collections::HashSet<_>>();
    Ok(paths
        .into_iter()
        .filter(|path| {
            let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                return false;
            };
            if name.ends_with(".tofu") || name.ends_with(".tofu.json") {
                return true;
            }
            if name.ends_with(".tf") {
                let tofu_name = format!("{}.tofu", name.trim_end_matches(".tf"));
                return !names.contains(&tofu_name);
            }
            if name.ends_with(".tf.json") {
                let tofu_name = format!("{}.tofu.json", name.trim_end_matches(".tf.json"));
                return !names.contains(&tofu_name);
            }
            false
        })
        .collect())
}

fn json_hcp(value: &Value) -> io::Result<bool> {
    let object = value.as_object().ok_or_else(invalid_configuration)?;
    let Some(terraform) = object.get("terraform") else {
        return Ok(false);
    };
    match terraform {
        Value::Array(blocks) => blocks
            .iter()
            .try_fold(false, |found, block| Ok(found | json_terraform_hcp(block)?)),
        _ => json_terraform_hcp(terraform),
    }
}

fn json_terraform_hcp(value: &Value) -> io::Result<bool> {
    let object = value.as_object().ok_or_else(invalid_configuration)?;
    if object.contains_key("cloud") {
        return Ok(true);
    }
    match object.get("backend") {
        None => Ok(false),
        Some(Value::Object(backends)) => Ok(backends.contains_key("remote")),
        Some(Value::Array(backends)) => backends.iter().try_fold(false, |found, backend| {
            Ok(found
                | backend
                    .as_object()
                    .ok_or_else(invalid_configuration)?
                    .contains_key("remote"))
        }),
        _ => Err(invalid_configuration()),
    }
}

fn invalid_configuration() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "cannot determine Terraform execution location",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new(name: &str, source: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "terracotta-config-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).expect("create fixture");
            fs::write(root.join(name), source).expect("write configuration");
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("clean fixture");
        }
    }

    #[rstest]
    #[case::cloud("main.tf", "terraform {\n cloud {}\n}")]
    #[case::remote("main.tf", "terraform {\n backend \"remote\" {}\n}")]
    #[case::json_cloud("main.tf.json", r#"{"terraform":{"cloud":{}}}"#)]
    #[case::json_remote("main.tf.json", r#"{"terraform":[{"backend":{"remote":{}}}]}"#)]
    fn hcp_configuration_is_a_candidate(#[case] name: &str, #[case] source: &str) {
        let fixture = Fixture::new(name, source);
        assert_eq!(
            execution_location(&fixture.0, None).unwrap(),
            ExecutionLocation::HcpCandidate
        );
    }

    #[rstest]
    #[case::s3("main.tf", "terraform {\n backend \"s3\" {}\n}")]
    #[case::gcs("main.tf.json", r#"{"terraform":{"backend":{"gcs":{}}}}"#)]
    #[case::comments(
        "main.tf",
        "# terraform { cloud {} }\n/* backend \"remote\" {} */\nlocals { text = \"cloud\" }"
    )]
    #[case::strings("main.tf.json", r#"{"locals":{"text":"terraform {\n cloud {}\n}"}}"#)]
    fn local_configuration_does_not_detect_cloud_words(#[case] name: &str, #[case] source: &str) {
        let fixture = Fixture::new(name, source);
        assert_eq!(
            execution_location(&fixture.0, None).unwrap(),
            ExecutionLocation::Local
        );
    }

    #[rstest]
    #[case::hcl("main.tf", "terraform {")]
    #[case::json("main.tf.json", "{")]
    #[case::json_shape("main.tf.json", r#"{"terraform":true}"#)]
    fn broken_configuration_is_indeterminate(#[case] name: &str, #[case] source: &str) {
        let fixture = Fixture::new(name, source);
        assert!(execution_location(&fixture.0, None).is_err());
    }

    #[test]
    fn initialized_backend_uses_selected_data_directory_and_rejects_corruption() {
        let fixture = Fixture::new("main.tf", "");
        let data = fixture.0.join("custom data");
        fs::create_dir(&data).unwrap();
        let state = data.join("terraform.tfstate");
        for (kind, expected) in [
            ("remote", ExecutionLocation::HcpCandidate),
            ("cloud", ExecutionLocation::HcpCandidate),
            ("s3", ExecutionLocation::Local),
            ("gcs", ExecutionLocation::Local),
        ] {
            fs::write(&state, format!(r#"{{"backend":{{"type":"{kind}"}}}}"#)).unwrap();
            assert_eq!(
                execution_location(&fixture.0, Some(OsStr::new("custom data"))).unwrap(),
                expected
            );
            assert_eq!(
                execution_location(&fixture.0, Some(data.as_os_str())).unwrap(),
                expected
            );
        }
        fs::write(&state, "broken").unwrap();
        assert!(execution_location(&fixture.0, Some(data.as_os_str())).is_err());
    }

    #[test]
    fn opentofu_prefers_tofu_configuration_over_same_named_terraform_file() {
        let fixture = Fixture::new("main.tf", "terraform {\n  backend \"s3\" {}\n}\n");
        fs::write(fixture.0.join("main.tofu"), "terraform {\n  cloud {}\n}\n").unwrap();

        assert_eq!(
            execution_location_for_tool(&fixture.0, Tool::OpenTofu, None).unwrap(),
            ExecutionLocation::HcpCandidate
        );
    }

    #[test]
    fn opentofu_prefers_tofu_json_over_same_named_terraform_json_file() {
        let fixture = Fixture::new("main.tf.json", r#"{"terraform":{"backend":{"s3":{}}}}"#);
        fs::write(
            fixture.0.join("main.tofu.json"),
            r#"{"terraform":{"cloud":{}}}"#,
        )
        .unwrap();

        assert_eq!(
            execution_location_for_tool(&fixture.0, Tool::OpenTofu, None).unwrap(),
            ExecutionLocation::HcpCandidate
        );
    }
}
