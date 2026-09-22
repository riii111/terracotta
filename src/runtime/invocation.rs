use std::{
    env,
    ffi::{OsStr, OsString},
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::ExitCode,
};

use crate::infra::terraform::{
    self,
    configuration::{self, ExecutionLocation},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Subcommand {
    Plan,
    Apply,
}

struct Invocation {
    subcommand: Subcommand,
    directory: PathBuf,
    global_arguments: Vec<OsString>,
    effective_arguments: Vec<OsString>,
    json: bool,
    auto_approve: bool,
    input: bool,
    saved_plan: Option<OsString>,
}

pub(crate) fn run(arguments: &[OsString]) -> ExitCode {
    match execute(arguments) {
        Ok(exit) => exit,
        Err(error) => {
            super::report_error(&error.to_string());
            ExitCode::from(1)
        }
    }
}

fn execute(arguments: &[OsString]) -> io::Result<ExitCode> {
    let executable = terraform::resolve_executable()?;
    let terminals = [
        io::stdin().is_terminal(),
        io::stdout().is_terminal(),
        io::stderr().is_terminal(),
    ];
    if interactive(terminals, env::var_os("CI").as_deref(), env::var_os("TF_IN_AUTOMATION").as_deref())
        && let Ok(root) = env::current_dir()
        && let Some(invocation) = parse(arguments, &root, |name| env::var_os(name))
        && invocation.review_candidate()
        // SBI01-02 will connect effective options and apply to the managed execution path.
        && invocation.subcommand == Subcommand::Plan
        && invocation.global_arguments.is_empty()
        && invocation.effective_arguments.is_empty()
        && configuration::execution_location(&invocation.directory, env::var_os("TF_DATA_DIR").as_deref())
            .is_ok_and(|location| location == ExecutionLocation::Local)
    {
        return Ok(super::run_plan(&invocation.directory, None));
    }
    terraform::delegate(&executable, arguments)
}

fn interactive(terminals: [bool; 3], ci: Option<&OsStr>, automation: Option<&OsStr>) -> bool {
    terminals.into_iter().all(|terminal| terminal)
        && !ci.is_some_and(|value| {
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        })
        && automation.is_none_or(OsStr::is_empty)
}

impl Invocation {
    const fn review_candidate(&self) -> bool {
        !self.json
            && !self.auto_approve
            && self.saved_plan.is_none()
            && (matches!(self.subcommand, Subcommand::Plan) || self.input)
    }
}

fn parse(
    arguments: &[OsString],
    root: &Path,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> Option<Invocation> {
    let mut offset = 0;
    let mut directory = root.to_path_buf();
    if let Some(value) = arguments.first()?.to_str()?.strip_prefix("-chdir=") {
        if !value.is_empty() {
            directory = root.join(value);
        }
        offset = 1;
    }
    let command = arguments.get(offset)?.to_str()?;
    let subcommand = match command {
        "plan" => Subcommand::Plan,
        "apply" => Subcommand::Apply,
        _ => return None,
    };
    let mut effective_arguments = Vec::new();
    for name in ["TF_CLI_ARGS".to_owned(), format!("TF_CLI_ARGS_{command}")] {
        if let Some(value) = lookup(&name) {
            effective_arguments.extend(
                split_arguments(value.to_str()?)?
                    .into_iter()
                    .map(OsString::from),
            );
        }
    }
    effective_arguments.extend_from_slice(&arguments[offset + 1..]);
    let mut invocation = Invocation {
        subcommand,
        directory,
        global_arguments: arguments[..offset].to_vec(),
        effective_arguments,
        json: false,
        auto_approve: false,
        input: !lookup("TF_INPUT")
            .is_some_and(|value| value == "0" || value.eq_ignore_ascii_case("false")),
        saved_plan: None,
    };
    classify_options(&mut invocation)?;
    Some(invocation)
}

fn classify_options(invocation: &mut Invocation) -> Option<()> {
    let mut arguments = invocation.effective_arguments.iter();
    while let Some(argument) = arguments.next() {
        let argument = argument.to_str()?;
        if !argument.starts_with('-') {
            if invocation.subcommand != Subcommand::Apply || invocation.saved_plan.is_some() {
                return None;
            }
            invocation.saved_plan = Some(OsString::from(argument));
            // Go flag parsing stops at the first positional argument.
            if arguments.next().is_some() {
                return None;
            }
            break;
        }
        let option = argument
            .strip_prefix("--")
            .unwrap_or_else(|| &argument[1..]);
        let (name, value) = option
            .split_once('=')
            .map_or((option, None), |(name, value)| (name, Some(value)));
        match name {
            "json" => invocation.json = flag_bool(value)?,
            "auto-approve" if invocation.subcommand == Subcommand::Apply => {
                invocation.auto_approve = flag_bool(value)?;
            }
            "input" => invocation.input = flag_bool(value)?,
            "destroy" | "refresh-only" | "refresh" | "lock" | "compact-warnings" | "no-color" => {
                flag_bool(value)?;
            }
            "detailed-exitcode" if invocation.subcommand == Subcommand::Plan => {
                flag_bool(value)?;
            }
            "var" | "var-file" | "target" | "replace" | "parallelism" | "lock-timeout" => {
                if value.is_none() {
                    arguments.next()?.to_str()?;
                }
            }
            "out" | "generate-config-out" if invocation.subcommand == Subcommand::Plan => {
                if value.is_none() {
                    arguments.next()?.to_str()?;
                }
            }
            _ => return None,
        }
    }
    Some(())
}

fn flag_bool(value: Option<&str>) -> Option<bool> {
    match value {
        None | Some("1" | "t" | "T" | "TRUE" | "true" | "True") => Some(true),
        Some("0" | "f" | "F" | "FALSE" | "false" | "False") => Some(false),
        _ => None,
    }
}

fn split_arguments(value: &str) -> Option<Vec<String>> {
    let mut arguments = Vec::new();
    let mut argument = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some('"'), '"') | (Some('\''), '\'') => quote = None,
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None | Some('"'), '\\') => {
                argument.push(chars.next()?);
                started = true;
            }
            (None, ch) if ch.is_whitespace() => {
                if started {
                    arguments.push(std::mem::take(&mut argument));
                    started = false;
                }
            }
            _ => {
                argument.push(ch);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        arguments.push(argument);
    }
    Some(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn invocation(args: &[&str], environment: &[(&str, &str)]) -> Option<Invocation> {
        parse(
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            Path::new("/root"),
            |name| {
                environment
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            },
        )
    }

    #[test]
    fn environment_options_precede_explicit_options_without_deduplicating_values() {
        let parsed = invocation(
            &["plan", "-var", "name=last value", "-json=false"],
            &[
                ("TF_CLI_ARGS", "-var 'name=first value' -json"),
                ("TF_CLI_ARGS_plan", "-var-file=second.tfvars"),
                ("TF_CLI_ARGS_apply", "-json"),
            ],
        )
        .expect("known options");

        assert!(parsed.review_candidate());
        assert_eq!(
            parsed.effective_arguments,
            [
                "-var",
                "name=first value",
                "-json",
                "-var-file=second.tfvars",
                "-var",
                "name=last value",
                "-json=false"
            ]
            .map(OsString::from)
        );
    }

    #[rstest]
    #[case::json(&["plan", "-json"], &[])]
    #[case::environment_json(&["plan"], &[("TF_CLI_ARGS_plan", "-json")])]
    #[case::approve(&["apply"], &[("TF_CLI_ARGS_apply", "-auto-approve")])]
    #[case::input_disabled(&["apply", "-input=false"], &[])]
    #[case::environment_input(&["apply"], &[("TF_INPUT", "0")])]
    #[case::saved_plan(&["apply", "saved.tfplan"], &[])]
    fn noninteractive_options_delegate(
        #[case] args: &[&str],
        #[case] environment: &[(&str, &str)],
    ) {
        assert!(
            !invocation(args, environment)
                .expect("recognized arguments")
                .review_candidate()
        );
    }

    #[test]
    fn boolean_precedence_uses_last_value_in_original_subcommand() {
        let parsed = invocation(
            &["apply", "-auto-approve=false", "-input=true"],
            &[
                ("TF_CLI_ARGS", "-json -auto-approve=false"),
                ("TF_CLI_ARGS_apply", "-json=false -auto-approve"),
                ("TF_CLI_ARGS_plan", "-json"),
                ("TF_INPUT", "0"),
            ],
        )
        .expect("valid flags");
        assert!(parsed.review_candidate());
    }

    #[rstest]
    #[case::unknown(&["plan", "-future"], &[])]
    #[case::help(&["plan", "-help"], &[])]
    #[case::version(&["-version"], &[])]
    #[case::missing_value(&["plan", "-var"], &[])]
    #[case::invalid_boolean(&["plan", "-json=maybe"], &[])]
    #[case::bad_quotes(&["plan"], &[("TF_CLI_ARGS", "-var 'unterminated")])]
    #[case::global_in_environment(&["plan"], &[("TF_CLI_ARGS", "-chdir=other")])]
    fn unclassified_arguments_delegate(
        #[case] args: &[&str],
        #[case] environment: &[(&str, &str)],
    ) {
        assert!(invocation(args, environment).is_none());
    }

    #[test]
    fn chdir_is_resolved_against_original_directory() {
        let parsed = invocation(&["-chdir=directory with spaces", "plan"], &[]).expect("chdir");
        assert_eq!(parsed.directory, Path::new("/root/directory with spaces"));
        assert_eq!(
            parsed.global_arguments,
            [OsString::from("-chdir=directory with spaces")]
        );
    }

    #[test]
    fn environment_splitting_preserves_literal_expansions_and_empty_values() {
        assert_eq!(
            split_arguments(r#"-var 'x=$HOME $(touch marker)' -var "x=`id`" ''"#),
            Some(vec![
                "-var".into(),
                "x=$HOME $(touch marker)".into(),
                "-var".into(),
                "x=`id`".into(),
                String::new()
            ])
        );
    }

    #[test]
    fn terminal_and_automation_conditions_control_interactivity() {
        for (name, terminals, ci, automation, expected) in [
            ("interactive", [true; 3], None, None, true),
            ("stdin", [false, true, true], None, None, false),
            ("stdout", [true, false, true], None, None, false),
            ("stderr", [true, true, false], None, None, false),
            ("ci", [true; 3], Some("true"), None, false),
            ("ci_false", [true; 3], Some("false"), None, true),
            ("ci_zero", [true; 3], Some("0"), None, true),
            (
                "automation_false_is_active",
                [true; 3],
                None,
                Some("false"),
                false,
            ),
            ("empty_automation", [true; 3], None, Some(""), true),
        ] {
            assert_eq!(
                interactive(terminals, ci.map(OsStr::new), automation.map(OsStr::new)),
                expected,
                "{name}"
            );
        }
    }
}
