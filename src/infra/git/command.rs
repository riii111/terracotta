use std::{
    ffi::OsStr,
    io,
    path::Path,
    process::{Command, Output},
};

#[derive(Debug)]
pub(super) struct GitCommandError {
    pub(super) operation: String,
    pub(super) message: String,
}

impl GitCommandError {
    fn from_spawn(operation: &str, error: &io::Error) -> Self {
        Self {
            operation: operation.to_owned(),
            message: error.to_string(),
        }
    }

    pub(super) fn from_output(operation: &str, output: &Output) -> Self {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Self {
            operation: operation.to_owned(),
            message: if message.is_empty() {
                format!("git exited with status {}", output.status)
            } else {
                message
            },
        }
    }
}

pub(super) fn checked_git<I, S>(
    directory: &Path,
    operation: &str,
    args: I,
) -> Result<Output, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git(directory, operation, args)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitCommandError::from_output(operation, &output))
    }
}

pub(super) fn run_git<I, S>(
    directory: &Path,
    operation: &str,
    args: I,
) -> Result<Output, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_git_with_env(directory, operation, args, &[])
}

pub(super) fn run_git_with_env<I, S>(
    directory: &Path,
    operation: &str,
    args: I,
    environment: &[(&str, &str)],
) -> Result<Output, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(args);
    for (key, value) in environment {
        command.env(key, value);
    }
    command
        .output()
        .map_err(|error| GitCommandError::from_spawn(operation, &error))
}

pub(super) fn parse_error(operation: &str, message: &str) -> GitCommandError {
    GitCommandError {
        operation: operation.to_owned(),
        message: message.to_owned(),
    }
}

pub(super) fn nul_fields(output: &[u8], operation: &str) -> Result<Vec<String>, GitCommandError> {
    output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| {
            String::from_utf8(field.to_owned()).map_err(|error| {
                parse_error(operation, &format!("Git path is not valid UTF-8: {error}"))
            })
        })
        .collect()
}
