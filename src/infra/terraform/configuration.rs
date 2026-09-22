use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use crate::app::execution::Tool;
use hcl::Body;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionLocation {
    Local,
    HcpCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Configuration {
    pub(crate) has_backend: bool,
    pub(crate) execution_location: ExecutionLocation,
}

pub(crate) fn has_configuration(root: &Path, tool: Tool) -> io::Result<bool> {
    Ok(!configuration_files(root, tool)?.is_empty())
}

pub(crate) fn execution_location_for_tool(
    root: &Path,
    tool: Tool,
    data_dir: Option<&OsStr>,
) -> io::Result<ExecutionLocation> {
    Ok(read_configuration(root, tool, data_dir)?.execution_location)
}

pub(crate) fn read_configuration(
    root: &Path,
    tool: Tool,
    data_dir: Option<&OsStr>,
) -> io::Result<Configuration> {
    let mut configuration = Configuration {
        has_backend: false,
        execution_location: ExecutionLocation::Local,
    };
    for path in configuration_files(root, tool)? {
        let source = fs::read_to_string(&path)?;
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let value: Value =
                serde_json::from_str(&source).map_err(|_| invalid_configuration())?;
            read_json_configuration(&value, &mut configuration)?;
        } else {
            let body: Body = hcl::from_str(&source).map_err(|_| invalid_configuration())?;
            read_hcl_configuration(&body, &mut configuration)?;
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
                if matches!(kind, "remote" | "cloud") {
                    configuration.execution_location = ExecutionLocation::HcpCandidate;
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(configuration)
}

#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "Terraform and OpenTofu only recognize their lowercase configuration extensions"
)]
fn configuration_files(root: &Path, tool: Tool) -> io::Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| {
        !path.is_dir()
            && path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| {
                    !name.starts_with('.') && !name.starts_with('#') && !name.ends_with('~')
                })
    });
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

fn read_hcl_configuration(body: &Body, configuration: &mut Configuration) -> io::Result<()> {
    for terraform in body
        .blocks()
        .filter(|block| block.identifier() == "terraform")
    {
        if !terraform.labels().is_empty() {
            return Err(invalid_configuration());
        }
        for block in terraform.body.blocks() {
            match block.identifier() {
                "backend" => {
                    if block.labels().len() != 1 {
                        return Err(invalid_configuration());
                    }
                    configuration.has_backend = true;
                    if block.labels()[0].as_str() == "remote" {
                        configuration.execution_location = ExecutionLocation::HcpCandidate;
                    }
                }
                "cloud" => {
                    if !block.labels().is_empty() {
                        return Err(invalid_configuration());
                    }
                    configuration.has_backend = true;
                    configuration.execution_location = ExecutionLocation::HcpCandidate;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn read_json_configuration(value: &Value, configuration: &mut Configuration) -> io::Result<()> {
    let object = value.as_object().ok_or_else(invalid_configuration)?;
    if let Some(terraform) = object.get("terraform") {
        for_json_block(terraform, |block| {
            if let Some(cloud) = block.get("cloud") {
                for_json_block(cloud, |_| {
                    configuration.has_backend = true;
                    configuration.execution_location = ExecutionLocation::HcpCandidate;
                    Ok(())
                })?;
            }
            if let Some(backend) = block.get("backend") {
                for_json_block(backend, |backends| {
                    for (kind, body) in backends {
                        for_json_block(body, |_| {
                            configuration.has_backend = true;
                            if kind == "remote" {
                                configuration.execution_location = ExecutionLocation::HcpCandidate;
                            }
                            Ok(())
                        })?;
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn for_json_block(
    value: &Value,
    mut read: impl FnMut(&serde_json::Map<String, Value>) -> io::Result<()>,
) -> io::Result<()> {
    if let Some(blocks) = value.as_array() {
        for block in blocks {
            read(block.as_object().ok_or_else(invalid_configuration)?)?;
        }
        Ok(())
    } else {
        read(value.as_object().ok_or_else(invalid_configuration)?)
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
    use std::sync::atomic::{AtomicU64, Ordering};

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

    fn execution_location(root: &Path, data_dir: Option<&OsStr>) -> io::Result<ExecutionLocation> {
        super::execution_location_for_tool(root, Tool::Terraform, data_dir)
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
