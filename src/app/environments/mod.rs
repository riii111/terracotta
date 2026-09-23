use std::path::{Path, PathBuf};

use crate::app::{
    copy::{self, CopyEffect},
    execution::{Diagnostic, Tool},
    review::PlanReview,
    session::{self, Action, Effect, ReviewSessionState, SessionState},
};

pub(crate) mod comparison;
pub(crate) mod overview;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EnvironmentIdentity {
    pub(crate) directory: PathBuf,
    pub(crate) workspace: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Environment {
    pub(crate) tool: Tool,
    pub(crate) availability: EnvironmentAvailability,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EnvironmentAvailability {
    Available(EnvironmentIdentity),
    ExcludedHcp { directory: PathBuf },
    Error { directory: PathBuf, message: String },
}

#[derive(Debug)]
pub(crate) enum EnvironmentState {
    Pending,
    Running,
    Ready {
        session: Box<SessionState>,
        changed: bool,
    },
    Error,
    ExcludedHcp,
}

#[derive(Debug)]
pub(crate) struct EnvironmentPlan {
    pub(crate) tool: Tool,
    directory: PathBuf,
    identity: Option<EnvironmentIdentity>,
    state: EnvironmentState,
    diagnostics: Vec<Diagnostic>,
    failure: Option<String>,
}

pub(crate) enum PlanResult {
    Ready {
        review: Box<PlanReview>,
        changed: bool,
    },
    Error(String),
    ExcludedHcp,
}

pub(crate) struct EnvironmentSession {
    plans: Vec<EnvironmentPlan>,
    detailed_exitcode: bool,
    interrupted: bool,
    overview: overview::EnvironmentOverview,
    revision: u64,
}

impl Environment {
    pub(crate) const fn is_available(&self) -> bool {
        matches!(self.availability, EnvironmentAvailability::Available(_))
    }
}

impl EnvironmentSession {
    pub(crate) fn new(environments: Vec<Environment>, detailed_exitcode: bool) -> Self {
        let mut plans: Vec<_> = environments.into_iter().map(EnvironmentPlan::new).collect();
        plans.sort_by(|a, b| a.directory.cmp(&b.directory));
        Self {
            overview: overview::environment_overview(&plans),
            revision: 0,
            plans,
            detailed_exitcode,
            interrupted: false,
        }
    }

    pub(crate) fn plans(&self) -> &[EnvironmentPlan] {
        &self.plans
    }

    pub(crate) const fn overview(&self) -> &overview::EnvironmentOverview {
        &self.overview
    }

    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn start_next(&mut self) -> Option<usize> {
        if self.interrupted
            || self
                .plans
                .iter()
                .any(|plan| matches!(plan.state, EnvironmentState::Running))
        {
            return None;
        }
        let (index, plan) = self
            .plans
            .iter_mut()
            .enumerate()
            .find(|(_, plan)| matches!(plan.state, EnvironmentState::Pending))?;
        plan.state = EnvironmentState::Running;
        Some(index)
    }

    pub(crate) fn retry(&mut self, index: usize) -> bool {
        let Some(plan) = self.plans.get_mut(index) else {
            return false;
        };
        if self.interrupted || !matches!(plan.state, EnvironmentState::Error) {
            return false;
        }
        plan.state = EnvironmentState::Pending;
        plan.diagnostics.clear();
        plan.failure = None;
        self.refresh_overview();
        true
    }

    pub(crate) fn complete(
        &mut self,
        index: usize,
        result: PlanResult,
        diagnostics: Vec<Diagnostic>,
    ) -> bool {
        let Some(plan) = self.plans.get_mut(index) else {
            return false;
        };
        if self.interrupted || !matches!(plan.state, EnvironmentState::Running) {
            return false;
        }
        plan.diagnostics = diagnostics;
        plan.state = match result {
            PlanResult::Ready { review, changed } => {
                plan.identity = Some(EnvironmentIdentity {
                    directory: plan.directory.clone(),
                    workspace: review.workspace().to_owned(),
                });
                EnvironmentState::Ready {
                    session: Box::new(SessionState::Review(Box::new(ReviewSessionState::new(
                        review.with_diagnostics(std::mem::take(&mut plan.diagnostics)),
                    )))),
                    changed,
                }
            }
            PlanResult::Error(message) => {
                plan.failure = Some(message);
                EnvironmentState::Error
            }
            PlanResult::ExcludedHcp => EnvironmentState::ExcludedHcp,
        };
        self.refresh_overview();
        true
    }

    pub(crate) fn update_review(
        &mut self,
        index: usize,
        action: Action,
        now: std::time::Instant,
    ) -> Option<Effect> {
        if !matches!(
            action,
            Action::ReviewSearchChanged(_) | Action::Copy(_) | Action::CopyCompleted { .. }
        ) {
            return None;
        }
        let EnvironmentState::Ready { session, .. } = &mut self.plans.get_mut(index)?.state else {
            return None;
        };
        session::update(session, action, now)
    }

    pub(crate) fn acquiring(&self) -> bool {
        self.plans.iter().any(|plan| {
            matches!(
                plan.state,
                EnvironmentState::Pending | EnvironmentState::Running
            )
        })
    }

    pub(crate) const fn interrupt(&mut self) {
        self.interrupted = true;
    }

    pub(crate) fn exit_code(&self) -> u8 {
        if self.interrupted {
            return 130;
        }
        if self.plans.is_empty()
            || self
                .plans
                .iter()
                .any(|plan| !matches!(plan.state, EnvironmentState::Ready { .. }))
        {
            return 1;
        }
        if self.detailed_exitcode
            && self
                .plans
                .iter()
                .any(|plan| matches!(plan.state, EnvironmentState::Ready { changed: true, .. }))
        {
            2
        } else {
            0
        }
    }
    fn refresh_overview(&mut self) {
        self.overview = overview::environment_overview(&self.plans);
        self.revision += 1;
    }
}

impl EnvironmentPlan {
    fn new(environment: Environment) -> Self {
        let (directory, identity, state, failure) = match environment.availability {
            EnvironmentAvailability::Available(identity) => (
                identity.directory.clone(),
                Some(identity),
                EnvironmentState::Pending,
                None,
            ),
            EnvironmentAvailability::ExcludedHcp { directory } => {
                (directory, None, EnvironmentState::ExcludedHcp, None)
            }
            EnvironmentAvailability::Error { directory, message } => {
                (directory, None, EnvironmentState::Error, Some(message))
            }
        };
        Self {
            tool: environment.tool,
            directory,
            identity,
            state,
            diagnostics: Vec::new(),
            failure,
        }
    }

    pub(crate) const fn state(&self) -> &EnvironmentState {
        &self.state
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn workspace(&self) -> Option<&str> {
        self.identity
            .as_ref()
            .map(|identity| identity.workspace.as_str())
    }

    pub(crate) fn review(&self) -> Option<&ReviewSessionState> {
        match &self.state {
            EnvironmentState::Ready { session, .. } => session.review(),
            _ => None,
        }
    }

    pub(crate) fn diagnostic(&self) -> CopyEffect {
        let sensitive = self.review().map_or(&[][..], |review| {
            review.review().metadata().sensitive_values()
        });
        let diagnostics = self.review().map_or(self.diagnostics.as_slice(), |review| {
            review.review().diagnostics()
        });
        let effect = copy::diagnostic_effect(diagnostics, self.failure.as_deref(), sensitive);
        if !diagnostics.is_empty()
            && let Some(failure) = self.failure.as_deref()
        {
            return CopyEffect::new(
                effect.target(),
                format!(
                    "{}\n\n{}",
                    copy::sanitize_text(failure, sensitive),
                    effect.text()
                ),
            );
        }
        effect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::review::{PlanMetadata, test_support::plan_document};

    fn available(name: &str) -> Environment {
        Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from(name),
                workspace: "default".to_owned(),
            }),
        }
    }

    fn ready(changed: bool) -> PlanResult {
        PlanResult::Ready {
            review: Box::new(PlanReview::new(
                PathBuf::from("/test"),
                "chosen".to_owned(),
                plan_document("No changes.\n".to_owned()),
                PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, changed),
                Vec::new(),
            )),
            changed,
        }
    }

    #[test]
    fn starts_in_path_order_and_keeps_only_one_running() {
        let mut state = EnvironmentSession::new(vec![available("z"), available("a")], false);
        assert_eq!(state.plans().len(), 2);
        assert!(
            state
                .plans()
                .iter()
                .all(|plan| matches!(plan.state(), EnvironmentState::Pending))
        );

        let first_index = state.start_next().unwrap();
        assert_eq!(state.plans()[first_index].directory(), Path::new("a"));
        assert!(state.start_next().is_none());
        assert!(state.complete(first_index, ready(true), Vec::new()));
        let next_index = state.start_next().unwrap();

        assert_eq!(state.plans()[next_index].directory(), Path::new("z"));
        assert!(state.plans()[0].review().is_some());
        assert!(matches!(
            state.plans()[1].state(),
            EnvironmentState::Running
        ));
    }

    #[test]
    fn retries_only_errors_and_rejects_duplicate_completions() {
        let mut state = EnvironmentSession::new(
            vec![
                available("a"),
                available("b"),
                Environment {
                    tool: Tool::Terraform,
                    availability: EnvironmentAvailability::ExcludedHcp {
                        directory: PathBuf::from("c"),
                    },
                },
            ],
            true,
        );
        let index = state.start_next().unwrap();
        assert!(!state.retry(0));
        assert!(state.complete(index, PlanResult::Error("failed".to_owned()), Vec::new()));
        assert!(state.retry(0));
        assert!(!state.retry(0));
        assert!(!state.retry(1));
        assert!(!state.retry(2));
        assert!(!state.retry(99));
        let retry_index = state.start_next().unwrap();

        assert!(state.complete(retry_index, ready(false), Vec::new()));
        assert!(!state.complete(
            retry_index,
            PlanResult::Error("duplicate".to_owned()),
            Vec::new()
        ));
        assert!(!state.retry(0));
        assert!(!state.complete(99, ready(true), Vec::new()));
        assert_eq!(
            state.plans()[0].identity.as_ref().unwrap().workspace,
            "chosen"
        );
        assert!(state.plans()[0].failure.is_none());
    }

    #[test]
    fn ready_environment_rejects_apply_even_if_given_an_applyable_review() {
        let mut state = EnvironmentSession::new(vec![available("a")], false);
        let run = state.start_next().unwrap();
        state.complete(run, ready(true), Vec::new());

        assert!(
            state
                .update_review(0, Action::OpenApplyConfirmation, std::time::Instant::now())
                .is_none()
        );
        assert!(state.plans()[0].review().is_some());
    }

    #[test]
    fn exit_code_uses_current_results_and_interruption_precedes_incomplete_results() {
        for (detailed, changed, expected) in [
            (false, false, 0),
            (false, true, 0),
            (true, false, 0),
            (true, true, 2),
        ] {
            let mut state = EnvironmentSession::new(vec![available("a")], detailed);
            assert_eq!(state.exit_code(), 1);
            let first = state.start_next().unwrap();
            state.complete(first, PlanResult::Error("failed".to_owned()), Vec::new());
            assert_eq!(state.exit_code(), 1);
            state.retry(0);
            let second = state.start_next().unwrap();
            state.complete(second, ready(changed), Vec::new());
            assert_eq!(
                state.exit_code(),
                expected,
                "detailed={detailed}, changed={changed}"
            );
            state.interrupt();
            assert_eq!(state.exit_code(), 130);
            assert!(!state.complete(second, ready(false), Vec::new()));
            assert!(state.start_next().is_none());
        }
        let state = EnvironmentSession::new(
            vec![Environment {
                tool: Tool::Terraform,
                availability: EnvironmentAvailability::ExcludedHcp {
                    directory: PathBuf::from("hcp"),
                },
            }],
            true,
        );
        assert_eq!(state.exit_code(), 1);
    }
}
