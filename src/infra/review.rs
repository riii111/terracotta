use std::{ffi::OsStr, path::Path};

use crate::app::{
    attribution::{AnalysisIssue, SourceFileAnalysis, attribute_changes, mark_analysis_incomplete},
    execution::{ExecutionEvent, ExecutionEventKind, ExecutionPhase},
    review::{PlanReview, ReviewComparison, ReviewComparisonBasis, ReviewComparisonStatus},
};

use super::{
    git::{self, ComparisonBasis, ConfigurationComparison, ConfigurationSnapshot, GitDiff},
    terraform::{self, CancellationToken, TerraformExecutionError, hcl},
};

pub(crate) fn run_review(
    root: &Path,
    compare_ref: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<PlanReview, TerraformExecutionError> {
    let mut ignore_event = |_| {};
    run_review_with_events(root, compare_ref, cancellation, &mut ignore_event)
}

pub(crate) fn run_review_with_events(
    root: &Path,
    compare_ref: Option<&str>,
    cancellation: &CancellationToken,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) -> Result<PlanReview, TerraformExecutionError> {
    let mut ignore_phase = |_| {};
    run_review_with_events_and_phases(
        root,
        compare_ref,
        cancellation,
        event_sink,
        &mut ignore_phase,
    )
}

pub(crate) fn run_review_with_events_and_phases(
    root: &Path,
    compare_ref: Option<&str>,
    cancellation: &CancellationToken,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    phase_sink: &mut dyn FnMut(ExecutionPhase),
) -> Result<PlanReview, TerraformExecutionError> {
    run_review_with_events_with_runner_and_phases(
        root,
        compare_ref,
        cancellation,
        &terraform::SystemProcessRunner,
        event_sink,
        phase_sink,
    )
}

pub(crate) fn run_review_with_events_with_runner(
    root: &Path,
    compare_ref: Option<&str>,
    cancellation: &CancellationToken,
    runner: &dyn terraform::ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
) -> Result<PlanReview, TerraformExecutionError> {
    let mut ignore_phase = |_| {};
    run_review_with_events_with_runner_and_phases(
        root,
        compare_ref,
        cancellation,
        runner,
        event_sink,
        &mut ignore_phase,
    )
}

pub(crate) fn run_review_with_events_with_runner_and_phases(
    root: &Path,
    compare_ref: Option<&str>,
    cancellation: &CancellationToken,
    runner: &dyn terraform::ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    phase_sink: &mut dyn FnMut(ExecutionPhase),
) -> Result<PlanReview, TerraformExecutionError> {
    let mut no_op = || {};
    run_review_with_events_with_runner_and_hook_and_phases(
        root,
        compare_ref,
        cancellation,
        runner,
        event_sink,
        &mut no_op,
        phase_sink,
    )
}

fn run_review_with_events_with_runner_and_hook(
    root: &Path,
    compare_ref: Option<&str>,
    cancellation: &CancellationToken,
    runner: &dyn terraform::ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    after_git_diff: &mut dyn FnMut(),
) -> Result<PlanReview, TerraformExecutionError> {
    run_review_with_events_with_runner_and_hook_and_phases(
        root,
        compare_ref,
        cancellation,
        runner,
        event_sink,
        after_git_diff,
        &mut |_| {},
    )
}

fn run_review_with_events_with_runner_and_hook_and_phases(
    root: &Path,
    compare_ref: Option<&str>,
    cancellation: &CancellationToken,
    runner: &dyn terraform::ProcessRunner,
    event_sink: &mut dyn FnMut(ExecutionEvent),
    after_git_diff: &mut dyn FnMut(),
    phase_sink: &mut dyn FnMut(ExecutionPhase),
) -> Result<PlanReview, TerraformExecutionError> {
    let git_diff = collect_git_diff(root, compare_ref);
    after_git_diff();
    let execution_root = git_diff.root().to_owned();
    let configuration_before = git::capture_working_tree_configuration(&execution_root);
    let git_branch = git::current_branch(&execution_root);
    event_sink(ExecutionEvent {
        received_at: std::time::Instant::now(),
        kind: ExecutionEventKind::Git(git_branch.clone()),
    });

    let workspace = terraform::read_workspace_with_runner(&execution_root, cancellation, runner)?;
    event_sink(ExecutionEvent {
        received_at: std::time::Instant::now(),
        kind: ExecutionEventKind::Workspace(workspace.clone()),
    });
    let execution = terraform::run_plan_with_events_with_runner_and_phase(
        &execution_root,
        cancellation,
        runner,
        event_sink,
        phase_sink,
    )?;
    phase_sink(ExecutionPhase::Matching);
    let configuration_after = git::capture_working_tree_configuration(&execution_root);

    let source_files = parse_git_sources(&git_diff);
    let configuration_comparison = git::compare_configuration(&git_diff, &configuration_before);
    let mut analysis_issues = git_analysis_issues(&git_diff);
    let current_native_paths = configuration_comparison
        .changed_paths()
        .iter()
        .filter(|path| is_supported_native_configuration_path(&execution_root, path))
        .cloned()
        .collect::<Vec<_>>();
    for path in configuration_before.differing_source_paths(
        git_diff.before(),
        git_diff.after(),
        &current_native_paths,
    ) {
        push_unique(
            &mut analysis_issues,
            AnalysisIssue::configuration_changed(&path),
        );
    }
    add_configuration_issues(
        &mut analysis_issues,
        &configuration_before,
        &configuration_after,
        &configuration_comparison,
        git_diff.basis() == ComparisonBasis::HeadVsMergeBase,
    );
    if git_diff.basis() == ComparisonBasis::HeadVsMergeBase {
        add_unsupported_comparison_changes(
            &mut analysis_issues,
            &git_diff,
            &git::compare_commit_configurations(&git_diff),
        );
    } else {
        add_unsupported_comparison_changes(
            &mut analysis_issues,
            &git_diff,
            &configuration_comparison,
        );
    }

    let mut attributions = attribute_changes(
        &execution.plan().changes,
        &source_files,
        git_diff.changed_lines(),
    );
    mark_analysis_incomplete(&mut attributions, &analysis_issues);

    Ok(PlanReview::new(
        execution_root,
        workspace,
        execution.plan().clone(),
        source_files,
        attributions,
        review_comparison(&git_diff),
        analysis_issues,
    )
    .with_git(git_branch.unwrap_or_else(|| "unavailable".to_owned())))
}

fn collect_git_diff(root: &Path, compare_ref: Option<&str>) -> GitDiff {
    compare_ref.map_or_else(
        || git::collect_diff(root),
        |compare_ref| git::collect_diff_against_ref(root, compare_ref),
    )
}

fn parse_git_sources(diff: &GitDiff) -> Vec<SourceFileAnalysis> {
    let inputs = diff
        .before()
        .iter()
        .chain(diff.after())
        .cloned()
        .collect::<Vec<_>>();
    hcl::parse_files(inputs).files().to_vec()
}

fn git_analysis_issues(diff: &GitDiff) -> Vec<AnalysisIssue> {
    diff.status()
        .message()
        .map_or_else(Vec::new, |message| vec![AnalysisIssue::git(message)])
}

fn add_configuration_issues(
    issues: &mut Vec<AnalysisIssue>,
    before: &ConfigurationSnapshot,
    after: &ConfigurationSnapshot,
    comparison: &ConfigurationComparison,
    execution_must_match_head: bool,
) {
    for path in before.changed_paths(after) {
        push_unique(issues, AnalysisIssue::configuration_changed(&path));
    }
    for message in before.issues().iter().chain(after.issues()) {
        push_unique(
            issues,
            AnalysisIssue::configuration_unavailable(None, message.clone()),
        );
    }
    for message in comparison.issues() {
        push_unique(
            issues,
            AnalysisIssue::configuration_unavailable(None, message.clone()),
        );
    }
    if execution_must_match_head {
        for path in comparison.changed_paths() {
            push_unique(issues, AnalysisIssue::configuration_differs_from_head(path));
        }
    }
}

fn add_unsupported_comparison_changes(
    issues: &mut Vec<AnalysisIssue>,
    diff: &GitDiff,
    comparison: &ConfigurationComparison,
) {
    for path in comparison.changed_paths() {
        if !is_supported_native_configuration_path(diff.root(), path) {
            push_unique(issues, AnalysisIssue::unsupported_configuration(path));
        }
    }
}

fn is_supported_native_configuration_path(root: &Path, path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == OsStr::new("tf"))
        && path.parent() == Some(root)
}

fn push_unique(issues: &mut Vec<AnalysisIssue>, issue: AnalysisIssue) {
    if !issues.contains(&issue) {
        issues.push(issue);
    }
}

fn review_comparison(diff: &GitDiff) -> ReviewComparison {
    let basis = match diff.basis() {
        ComparisonBasis::WorkingTreeVsHead => ReviewComparisonBasis::WorkingTreeVsHead,
        ComparisonBasis::HeadVsMergeBase => ReviewComparisonBasis::HeadVsMergeBase,
    };
    let status = diff
        .status()
        .message()
        .map_or(ReviewComparisonStatus::Complete, |message| {
            ReviewComparisonStatus::Incomplete(message.to_owned())
        });
    ReviewComparison::new(
        basis,
        diff.compare_ref().map(str::to_owned),
        diff.resolved_commit().map(str::to_owned),
        diff.head_commit().map(str::to_owned),
        diff.merge_base().map(str::to_owned),
        status,
    )
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::VecDeque,
        fs, io,
        path::PathBuf,
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    use crate::{
        app::attribution::{AnalysisIssueKind, AttributionStatus},
        infra::terraform::ProcessOutput,
    };
    use serde_json::json;

    use super::*;

    static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);

    struct TestRepository {
        path: PathBuf,
    }

    impl TestRepository {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "terracotta-review-{}-{}",
                std::process::id(),
                NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("test repository should be created");
            git(&path, &["init", "--quiet", "--initial-branch=main"]);
            git(&path, &["config", "user.email", "test@example.com"]);
            git(&path, &["config", "user.name", "Terracotta Test"]);
            Self { path }
        }

        fn write(&self, relative: &str, source: &str) {
            fs::write(self.path.join(relative), source).expect("test source should be written");
        }

        fn commit(&self, message: &str) {
            git(&self.path, &["add", "."]);
            git(&self.path, &["commit", "--quiet", "-m", message]);
        }
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).expect("test repository should be removed");
        }
    }

    fn git(repository: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .expect("git should start");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    struct FakeRunner {
        outputs: RefCell<VecDeque<ProcessOutput>>,
        mutate_on_plan: Option<(PathBuf, String)>,
    }

    impl FakeRunner {
        fn new(plan: ProcessOutput, mutate_on_plan: Option<(PathBuf, String)>) -> Self {
            Self {
                outputs: RefCell::new(VecDeque::from([
                    ProcessOutput::new(b"default\n".to_vec(), Vec::new()),
                    plan.clone(),
                    plan,
                ])),
                mutate_on_plan,
            }
        }
    }

    struct FakeProcess {
        output: Option<ProcessOutput>,
    }

    impl terraform::RunningProcess for FakeProcess {
        fn try_wait(&mut self) -> io::Result<Option<terraform::ProcessStatus>> {
            Ok(Some(terraform::ProcessStatus::Exited(0)))
        }

        fn kill(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn wait(&mut self) -> io::Result<terraform::ProcessStatus> {
            Ok(terraform::ProcessStatus::Exited(0))
        }

        fn collect_output(mut self: Box<Self>) -> io::Result<terraform::ProcessOutput> {
            self.output
                .take()
                .ok_or_else(|| io::Error::other("fake process output was already collected"))
        }
    }

    impl terraform::ProcessRunner for FakeRunner {
        fn start(
            &self,
            _root: &Path,
            arguments: &[std::ffi::OsString],
        ) -> io::Result<Box<dyn terraform::RunningProcess>> {
            if arguments.first().is_some_and(|argument| argument == "plan")
                && let Some((path, source)) = &self.mutate_on_plan
            {
                fs::write(path, source).expect("fake Terraform should mutate configuration");
            }
            let output = self
                .outputs
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| io::Error::other("fake process response was exhausted"))?;
            Ok(Box::new(FakeProcess {
                output: Some(output),
            }))
        }
    }

    fn plan_output(address: Option<&str>) -> ProcessOutput {
        let resource_changes = address.map_or_else(Vec::new, |address| {
            vec![json!({
                "address": address,
                "mode": "managed",
                "change": {"actions": ["update"]},
            })]
        });
        ProcessOutput::new(
            json!({
                "format_version": "1.0",
                "resource_changes": resource_changes,
            })
            .to_string()
            .into_bytes(),
            Vec::new(),
        )
    }

    fn run_fake_review(
        root: &Path,
        compare_ref: Option<&str>,
        plan: ProcessOutput,
        mutate_on_plan: Option<(PathBuf, String)>,
    ) -> PlanReview {
        let runner = FakeRunner::new(plan, mutate_on_plan);
        run_review_with_events_with_runner(
            root,
            compare_ref,
            &CancellationToken::new(),
            &runner,
            &mut |_| {},
        )
        .expect("fake Terraform review should succeed")
    }

    fn run_fake_review_after_git_diff(
        root: &Path,
        plan: ProcessOutput,
        mutate_after_git_diff: impl FnOnce(),
    ) -> PlanReview {
        let runner = FakeRunner::new(plan, None);
        let mut mutate_after_git_diff = Some(mutate_after_git_diff);
        run_review_with_events_with_runner_and_hook(
            root,
            None,
            &CancellationToken::new(),
            &runner,
            &mut |_| {},
            &mut || {
                mutate_after_git_diff
                    .take()
                    .expect("Git diff hook should run once")();
            },
        )
        .expect("fake Terraform review should succeed")
    }

    #[test]
    #[ignore = "requires Terraform CLI"]
    fn reviews_the_basic_scenario_with_four_direct_matches_and_one_no_match() {
        let setup = Command::new("python3")
            .args([
                concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/basic/scenario.py"),
                "setup",
            ])
            .output()
            .expect("scenario setup should start");
        assert!(
            setup.status.success(),
            "scenario setup failed: {}",
            String::from_utf8_lossy(&setup.stderr)
        );
        let directory = PathBuf::from(
            String::from_utf8(setup.stdout)
                .expect("scenario path should be UTF-8")
                .trim(),
        );

        let result = run_review(&directory, None, &CancellationToken::new());
        let cleanup = Command::new("python3")
            .args([
                concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/basic/scenario.py"),
                "clean",
            ])
            .arg(&directory)
            .output()
            .expect("scenario cleanup should start");
        assert!(
            cleanup.status.success(),
            "scenario cleanup failed: {}",
            String::from_utf8_lossy(&cleanup.stderr)
        );

        let review = result.expect("review should be returned");
        assert_eq!(review.root(), directory);
        assert_eq!(review.workspace(), "default");
        assert_eq!(
            review.comparison().basis(),
            ReviewComparisonBasis::WorkingTreeVsHead
        );
        assert!(review.comparison().head_commit().is_some());
        assert!(review.comparison().status().is_complete());
        assert_eq!(review.attributions().len(), 5);
        assert_eq!(
            review
                .attributions()
                .iter()
                .filter(|attribution| attribution.status() == AttributionStatus::Direct)
                .count(),
            4
        );
        assert_eq!(review.needs_review_count(), 1);
    }

    #[test]
    fn keeps_plan_data_when_git_is_outside_a_repository() {
        let root = std::env::temp_dir().join(format!(
            "terracotta-review-outside-{}",
            NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("outside root should be created");

        let review = run_fake_review(&root, None, plan_output(None), None);

        assert_eq!(review.plan().summary.total(), 0);
        assert!(!review.comparison().status().is_complete());
        assert!(
            review
                .analysis_issues()
                .iter()
                .any(|issue| { issue.kind() == AnalysisIssueKind::Git })
        );
        fs::remove_dir(&root).expect("outside root should be removed");
    }

    #[test]
    fn marks_module_resource_addresses_as_incomplete_without_root_matching() {
        let repository = TestRepository::new();
        repository.write("main.tf", "module \"child\" {\n  source = \"./child\"\n}\n");
        repository.commit("initial");
        repository.write(
            "main.tf",
            "module \"child\" {\n  source = \"./changed-child\"\n}\n",
        );

        let review = run_fake_review(
            &repository.path,
            None,
            plan_output(Some("module.child.terraform_data.value")),
            None,
        );

        assert_eq!(review.attributions().len(), 1);
        assert_eq!(review.needs_review_count(), 1);
        assert!(
            review.attributions()[0]
                .analysis()
                .issues()
                .iter()
                .any(|issue| { issue.kind() == AnalysisIssueKind::UnsupportedAddress })
        );
    }

    #[test]
    fn marks_configuration_changed_during_plan_as_incomplete() {
        let repository = TestRepository::new();
        repository.write(
            "main.tf",
            "resource \"terraform_data\" \"value\" {\n  input = \"before\"\n}\n",
        );
        repository.commit("initial");

        let review = run_fake_review(
            &repository.path,
            None,
            plan_output(Some("terraform_data.value")),
            Some((
                repository.path.join("main.tf"),
                "resource \"terraform_data\" \"value\" {\n  input = \"during\"\n}\n".to_owned(),
            )),
        );

        assert!(review.analysis_issues().iter().any(|issue| {
            issue.kind() == AnalysisIssueKind::ConfigurationChanged
                && issue.path().is_some_and(|path| path.ends_with("main.tf"))
        }));
        assert!(review.attributions()[0].needs_review());
    }

    #[test]
    fn marks_a_native_configuration_changed_after_git_collection_as_incomplete() {
        let repository = TestRepository::new();
        repository.write(
            "main.tf",
            "resource \"terraform_data\" \"value\" {\n  input = \"head\"\n}\n",
        );
        repository.write(
            "second.tf",
            "resource \"terraform_data\" \"second\" {\n  input = \"head\"\n}\n",
        );
        repository.commit("initial");
        repository.write(
            "main.tf",
            "resource \"terraform_data\" \"value\" {\n  input = \"intended\"\n}\n",
        );

        let review = run_fake_review_after_git_diff(
            &repository.path,
            plan_output(Some("terraform_data.value")),
            || {
                repository.write(
                    "second.tf",
                    "resource \"terraform_data\" \"second\" {\n  input = \"unintended\"\n}\n",
                );
            },
        );

        assert!(review.analysis_issues().iter().any(|issue| {
            issue.kind() == AnalysisIssueKind::ConfigurationChanged
                && issue.path().is_some_and(|path| path.ends_with("second.tf"))
        }));
        assert!(review.attributions()[0].needs_review());
    }

    #[test]
    fn marks_a_native_configuration_deleted_after_git_collection_as_incomplete() {
        let repository = TestRepository::new();
        repository.write(
            "main.tf",
            "resource \"terraform_data\" \"value\" {\n  input = \"head\"\n}\n",
        );
        repository.write(
            "second.tf",
            "resource \"terraform_data\" \"second\" {\n  input = \"head\"\n}\n",
        );
        repository.commit("initial");
        repository.write(
            "main.tf",
            "resource \"terraform_data\" \"value\" {\n  input = \"intended\"\n}\n",
        );

        let review = run_fake_review_after_git_diff(
            &repository.path,
            plan_output(Some("terraform_data.value")),
            || {
                fs::remove_file(repository.path.join("second.tf"))
                    .expect("late configuration deletion should succeed");
            },
        );

        assert!(review.analysis_issues().iter().any(|issue| {
            issue.kind() == AnalysisIssueKind::ConfigurationChanged
                && issue.path().is_some_and(|path| path.ends_with("second.tf"))
        }));
        assert!(review.attributions()[0].needs_review());
    }

    #[test]
    fn checks_json_variables_and_lockfile_changes_for_incomplete_analysis() {
        let repository = TestRepository::new();
        repository.write("main.tf", "resource \"terraform_data\" \"value\" {}\n");
        repository.commit("initial");
        for path in [
            "config.tf.json",
            "values.tfvars",
            "values.tfvars.json",
            ".terraform.lock.hcl",
        ] {
            repository.write(path, "changed\n");
        }

        let review = run_fake_review(&repository.path, None, plan_output(None), None);

        for path in [
            "config.tf.json",
            "values.tfvars",
            "values.tfvars.json",
            ".terraform.lock.hcl",
        ] {
            assert!(
                review.analysis_issues().iter().any(|issue| {
                    issue.kind() == AnalysisIssueKind::ConfigurationChanged
                        && issue
                            .path()
                            .is_some_and(|issue_path| issue_path.ends_with(path))
                }),
                "configuration change should be reported for {path}"
            );
        }
    }

    #[test]
    fn marks_dirty_execution_settings_in_head_comparison_as_incomplete() {
        let repository = TestRepository::new();
        repository.write(
            "main.tf",
            "resource \"terraform_data\" \"value\" {\n  input = \"head\"\n}\n",
        );
        repository.commit("initial");
        repository.write(
            "main.tf",
            "resource \"terraform_data\" \"value\" {\n  input = \"dirty\"\n}\n",
        );

        let review = run_fake_review(
            &repository.path,
            Some("main"),
            plan_output(Some("terraform_data.value")),
            None,
        );

        assert_eq!(
            review.comparison().basis(),
            ReviewComparisonBasis::HeadVsMergeBase
        );
        assert!(review.analysis_issues().iter().any(|issue| {
            issue.kind() == AnalysisIssueKind::ConfigurationChanged
                && issue.path().is_some_and(|path| path.ends_with("main.tf"))
        }));
        assert!(review.attributions()[0].needs_review());
    }
}
