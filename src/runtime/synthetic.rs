use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode};
use ratatui::{Terminal, backend::Backend, layout::Rect};

use crate::{
    app::{
        copy::CopyResult,
        environments::{
            Environment, EnvironmentAvailability, EnvironmentIdentity, EnvironmentSession,
            PlanResult,
        },
        execution::{
            ApplyStatus, EventStream, ExecutionContext, ExecutionEvent, ExecutionEventKind,
            ExecutionLogLine, ExecutionPhase, ExecutionState, ExecutionTargetSpec, ResourceAction,
            ResourceEvent, ResourceEventKind, Tool,
        },
        plan::{Plan, PlanAction, PlanValue, ResourceChange, ResourceChangeKind, ResourceMode},
        review::{PlanBlock, PlanBlockKind, PlanDocument, PlanLineKind, PlanMetadata, PlanReview},
        session::{Action, Effect, ReviewSessionState, SessionState},
    },
    ui::{
        QuitConfirmationInput,
        features::{
            environments::{EnvironmentInput, EnvironmentView},
            execution, overview, plan_review,
        },
        quit_confirmation_key_to_input,
    },
};

pub(super) fn run_synthetic() -> io::Result<()> {
    if std::env::args().any(|argument| argument == "--environments") {
        return run_synthetic_environments();
    }
    let mut state = SessionState::Review(Box::new(synthetic_review()));
    let mut view = plan_review::PlanReviewViewState::default();
    let mut confirmation_view = plan_review::ApplyConfirmationViewState::default();
    let mut complete_apply_at: Option<Instant> = None;
    let mut execution_view = execution::ExecutionViewState::default();
    let mut quit_confirmation = false;

    ratatui::run(|terminal| {
        loop {
            super::event_loop::draw_with_quit_confirmation(
                &state,
                terminal,
                execution_view,
                &view,
                &confirmation_view,
                Instant::now(),
                quit_confirmation,
            )?;

            if complete_apply_at.is_some_and(|at| Instant::now() >= at) {
                finish_synthetic_apply(&mut state, &mut execution_view, ApplyStatus::Succeeded);
                complete_apply_at = None;
                continue;
            }

            let timeout = complete_apply_at.map_or(Duration::from_millis(100), |at| {
                at.saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(100))
            });
            if !event::poll(timeout)? {
                continue;
            }
            let Some(action) = handle_synthetic_event(
                &event::read()?,
                terminal,
                &state,
                &mut view,
                &mut confirmation_view,
                &mut execution_view,
                &mut quit_confirmation,
            )?
            else {
                continue;
            };
            if apply_synthetic_action(
                &mut state,
                action,
                &mut execution_view,
                &mut complete_apply_at,
            ) {
                return Ok(());
            }
        }
    })
}

fn handle_synthetic_event<B: Backend>(
    event: &Event,
    terminal: &Terminal<B>,
    state: &SessionState,
    view: &mut plan_review::PlanReviewViewState,
    confirmation_view: &mut plan_review::ApplyConfirmationViewState,
    execution_view: &mut execution::ExecutionViewState,
    quit_confirmation: &mut bool,
) -> Result<Option<Action>, B::Error> {
    let Event::Key(key) = event else {
        if let Event::Resize(width, height) = *event {
            reconcile_synthetic_resize(
                state,
                view,
                Rect::new(0, 0, width, height),
                *quit_confirmation,
            );
        }
        return Ok(None);
    };
    if !key.is_press() {
        return Ok(None);
    }

    let key = *key;
    let mut confirmed_quit = false;
    let action = if *quit_confirmation {
        match quit_confirmation_key_to_input(key) {
            QuitConfirmationInput::Confirm => {
                *quit_confirmation = false;
                confirmed_quit = true;
                Some(Action::Quit)
            }
            QuitConfirmationInput::Cancel => {
                *quit_confirmation = false;
                None
            }
            QuitConfirmationInput::Consume => None,
            QuitConfirmationInput::Forward(key) => {
                *quit_confirmation = false;
                super::event_loop::handle_key_event(
                    terminal,
                    state,
                    execution_view,
                    view,
                    confirmation_view,
                    key,
                )?
            }
        }
    } else {
        super::event_loop::handle_key_event(
            terminal,
            state,
            execution_view,
            view,
            confirmation_view,
            key,
        )?
    };
    let Some(action) = action else {
        return Ok(None);
    };
    if matches!(action, Action::Quit) && !confirmed_quit {
        *quit_confirmation = true;
        return Ok(None);
    }
    Ok(Some(action))
}

fn reconcile_synthetic_resize(
    state: &SessionState,
    view: &mut plan_review::PlanReviewViewState,
    area: Rect,
    quit_confirmation: bool,
) {
    if let Some(review) = state.review() {
        let layout = plan_review::layout_with_quit_confirmation(
            area,
            view.searching(),
            review,
            quit_confirmation,
        );
        view.reconcile(
            layout.body(),
            layout.max_vertical(),
            layout.max_horizontal(),
            layout.matches(),
        );
    }
    if let Some(overview_state) = state.overview() {
        overview::reconcile_view(area, overview_state, view.overview_mut());
    }
}

// Stands in for the runtime effects: the synthetic session never starts Terraform,
// writes the clipboard, or saves apply history. Returns whether the session finished.
fn apply_synthetic_action(
    state: &mut SessionState,
    action: Action,
    execution_view: &mut execution::ExecutionViewState,
    complete_apply_at: &mut Option<Instant>,
) -> bool {
    match super::event_loop::update_session(state, action, execution_view, Instant::now()) {
        Some(Effect::StartApply) => {
            record_synthetic_apply_events(state, execution_view);
            *complete_apply_at = Some(Instant::now() + Duration::from_millis(250));
            false
        }
        Some(Effect::CancelExecution) => {
            finish_synthetic_apply(state, execution_view, ApplyStatus::Interrupted);
            *complete_apply_at = None;
            false
        }
        Some(Effect::WriteClipboard(effect)) => apply_synthetic_action(
            state,
            Action::CopyCompleted {
                target: effect.target(),
                result: CopyResult::Written,
            },
            execution_view,
            complete_apply_at,
        ),
        Some(Effect::Finish(_)) => true,
        Some(Effect::PersistHistory(_)) | None => false,
    }
}

fn synthetic_review() -> ReviewSessionState {
    let plan = PlanReview::new(
        PathBuf::from("/workspace/infra/prod"),
        "default".to_owned(),
        PlanDocument::with_blocks_and_line_kinds(
            "Terraform will perform the following actions:\n\n  # terraform_data.example will be updated in-place\n  ~ resource \"terraform_data.example\" {\n      ~ input = \"before\" -> \"after\"\n      note = \"searchable synthetic value\"\n    }\n\n  # terraform_data.cache will be created\n  + resource \"terraform_data\" \"cache\" {\n      input = \"cache\"\n    }\n\n  # terraform_data.old will be destroyed\n  - resource \"terraform_data\" \"old\" {}\n\nPlan: 1 to add, 1 to change, 1 to destroy.\n"
                .to_owned(),
            vec![
                PlanBlock::new(0..2, PlanBlockKind::Common),
                PlanBlock::with_addresses(
                    2..7,
                    PlanBlockKind::Resource,
                    vec!["terraform_data.example".to_owned()],
                ),
                PlanBlock::new(7..8, PlanBlockKind::Common),
                PlanBlock::with_addresses(
                    8..12,
                    PlanBlockKind::Resource,
                    vec!["terraform_data.cache".to_owned()],
                ),
                PlanBlock::new(12..13, PlanBlockKind::Common),
                PlanBlock::with_addresses(
                    13..15,
                    PlanBlockKind::Resource,
                    vec!["terraform_data.old".to_owned()],
                ),
                PlanBlock::new(15..17, PlanBlockKind::Common),
            ],
            vec![
                PlanLineKind::Intro,
                PlanLineKind::Intro,
                PlanLineKind::Note,
                PlanLineKind::Body,
                PlanLineKind::Body,
                PlanLineKind::Body,
                PlanLineKind::Body,
                PlanLineKind::Intro,
                PlanLineKind::Note,
                PlanLineKind::Body,
                PlanLineKind::Body,
                PlanLineKind::Body,
                PlanLineKind::Intro,
                PlanLineKind::Note,
                PlanLineKind::Body,
                PlanLineKind::Intro,
                PlanLineKind::Summary,
            ],
        ),
        Plan {
            value_addresses: BTreeSet::new(),
            resource_changes: vec![
                synthetic_change(
                    "terraform_data.example",
                    ResourceChangeKind::Update,
                    vec![PlanAction::Update],
                    "before",
                    "after",
                ),
                synthetic_change(
                    "terraform_data.cache",
                    ResourceChangeKind::Create,
                    vec![PlanAction::Create],
                    "",
                    "cache",
                ),
                synthetic_change(
                    "terraform_data.old",
                    ResourceChangeKind::Delete,
                    vec![PlanAction::Delete],
                    "old",
                    "",
                ),
            ],
            unsupported_changes: Vec::new(),
            output_changes: Vec::new(),
        },
        PlanMetadata::new(true),
        Vec::new(),
    )
    .with_context(
        ExecutionContext::loading("/workspace/infra/prod")
            .with_launch_root("/workspace")
            .with_workspace("default")
            .with_tool_version(Tool::Terraform, "1.9.0"),
    );
    ReviewSessionState::new(plan)
}

fn synthetic_change(
    address: &str,
    kind: ResourceChangeKind,
    actions: Vec<PlanAction>,
    before: &str,
    after: &str,
) -> ResourceChange {
    ResourceChange {
        address: address.to_owned(),
        provider: None,
        resource_type: Some("terraform_data".to_owned()),
        resource_name: Some(address.rsplit('.').next().unwrap_or(address).to_owned()),
        mode: ResourceMode::Managed,
        actions,
        kind,
        before: Some(PlanValue::Object(BTreeMap::from([(
            "input".to_owned(),
            PlanValue::String(before.to_owned()),
        )]))),
        after: Some(PlanValue::Object(BTreeMap::from([(
            "input".to_owned(),
            PlanValue::String(after.to_owned()),
        )]))),
        before_sensitive: None,
        after_sensitive: None,
        after_unknown: None,
        replace_paths: None,
        action_reason: None,
        previous_address: None,
        importing: None,
    }
}

fn finish_synthetic_apply(
    state: &mut SessionState,
    execution_view: &mut execution::ExecutionViewState,
    status: ApplyStatus,
) {
    let summary_line = matches!(status, ApplyStatus::Succeeded)
        .then(|| "Apply complete! Resources: 1 added, 1 changed, 1 destroyed.".to_owned());
    let _ = super::event_loop::update_session(
        state,
        Action::ApplyCompleted {
            status,
            summary_line,
        },
        execution_view,
        Instant::now(),
    );
}

fn record_synthetic_apply_events(
    state: &mut SessionState,
    execution_view: &mut execution::ExecutionViewState,
) {
    let now = Instant::now();
    for kind in synthetic_apply_events() {
        let _ = super::event_loop::update_session(
            state,
            Action::ApplyWorkerEvent(ExecutionEvent {
                received_at: now,
                kind,
            }),
            execution_view,
            now,
        );
    }
}

fn resource_event(
    address: &str,
    kind: ResourceEventKind,
    action: ResourceAction,
    message: &str,
) -> ExecutionEventKind {
    ExecutionEventKind::Resource(ResourceEvent {
        address: address.to_owned(),
        kind,
        action: Some(action),
        message: Some(message.to_owned()),
    })
}

fn run_synthetic_environments() -> io::Result<()> {
    let names = if std::env::args().any(|argument| argument == "--many-environments") {
        (0..12)
            .map(|index| format!("env-{index:02}"))
            .collect::<Vec<_>>()
    } else {
        ["dev", "prod", "stg"].map(str::to_owned).to_vec()
    };
    let mut state = EnvironmentSession::new(
        names
            .into_iter()
            .map(|name| Environment {
                tool: Tool::Terraform,
                availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                    directory: PathBuf::from(format!("/example/{name}")),
                    workspace: "default".to_owned(),
                }),
            })
            .collect(),
        true,
    );
    let mut view = EnvironmentView::default();
    let mut next = Instant::now() + Duration::from_millis(500);
    let mut failed_once = false;
    let mut running = state.start_next();
    ratatui::run(|terminal| {
        loop {
            if Instant::now() >= next {
                if let Some(index) = running.take() {
                    let result = if index == 1 && !failed_once {
                        failed_once = true;
                        PlanResult::Error(
                            "Synthetic missing variable. Press r to retry this environment."
                                .to_owned(),
                        )
                    } else {
                        PlanResult::Ready {
                            review: Box::new(synthetic_environment_review(
                                state.plans()[index].directory(),
                                if index == 1 { 200 } else { 20 },
                            )),
                            changed: true,
                        }
                    };
                    state.complete(index, result, Vec::new());
                }
                running = state.start_next();
                next = Instant::now() + Duration::from_millis(750);
            }
            terminal.draw(|frame| view.render(frame, &state))?;
            if !event::poll(Duration::from_millis(100))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            match view.handle_key(key, terminal.size()?, &state) {
                Some(EnvironmentInput::Quit | EnvironmentInput::Interrupt) => break,
                Some(EnvironmentInput::Retry(index)) => {
                    state.retry(index);
                }
                Some(EnvironmentInput::Review(index, action)) => {
                    state.update_review(index, *action, Instant::now());
                }
                None => {}
            }
        }
        Ok(())
    })
}

fn synthetic_environment_review(directory: &Path, count: usize) -> PlanReview {
    let changes: Vec<_> = (0..count)
        .map(|index| {
            synthetic_change(
                &format!("terraform_data.server[{index}]"),
                ResourceChangeKind::Update,
                vec![PlanAction::Update],
                "before",
                "after",
            )
        })
        .collect();
    let mut lines = vec![
        "Terraform will perform the following actions:".to_owned(),
        String::new(),
    ];
    let mut blocks = vec![PlanBlock::new(0..2, PlanBlockKind::Common)];
    for change in &changes {
        let start = lines.len();
        lines.extend([
            format!("# {} will be updated in-place", change.address),
            "~ input = before -> after".to_owned(),
            String::new(),
        ]);
        blocks.push(PlanBlock::with_addresses(
            start..lines.len(),
            PlanBlockKind::Resource,
            vec![change.address.clone()],
        ));
    }
    PlanReview::new(
        directory.to_owned(),
        "default".to_owned(),
        PlanDocument::with_blocks_and_line_kinds(lines.join("\n"), blocks, Vec::new()),
        Plan {
            resource_changes: changes,
            ..Plan::empty()
        },
        PlanMetadata::new(true),
        Vec::new(),
    )
    .with_apply_allowed(false)
    .with_apply_entry(false)
    .with_context(
        ExecutionContext::loading(directory)
            .with_workspace("default")
            .with_tool_version(Tool::Terraform, "1.9.0"),
    )
}

pub(super) fn run_synthetic_execution() -> io::Result<()> {
    let started = Instant::now();
    let mut execution = ExecutionState::applying_with_targets(
        started,
        ExecutionContext::loading("infra/prod").with_workspace("default"),
        synthetic_apply_targets(),
        Vec::new(),
    );
    execution.record(ExecutionEvent {
        received_at: started,
        kind: ExecutionEventKind::Phase(ExecutionPhase::Planning),
    });
    for kind in synthetic_apply_events() {
        execution.record(ExecutionEvent {
            received_at: started,
            kind,
        });
    }
    let mut execution_view = execution::ExecutionViewState::default();
    execution_view.initialize_target_selection(&execution.progress().display_target_indices(false));
    let mut state = SessionState::Apply(Box::new(execution));
    let mut review_view = plan_review::PlanReviewViewState::default();
    let mut confirmation_view = plan_review::ApplyConfirmationViewState::default();
    let mut complete_apply_at = Some(started + Duration::from_millis(750));
    ratatui::run(|terminal| {
        loop {
            super::event_loop::draw_with_quit_confirmation(
                &state,
                terminal,
                execution_view,
                &review_view,
                &confirmation_view,
                Instant::now(),
                false,
            )?;
            if complete_apply_at.is_some_and(|at| Instant::now() >= at) {
                finish_synthetic_apply(&mut state, &mut execution_view, ApplyStatus::Succeeded);
                complete_apply_at = None;
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if !key.is_press() {
                continue;
            }
            if key.code == KeyCode::Esc {
                return Ok(());
            }
            let Some(action) = super::event_loop::handle_key_event(
                terminal,
                &state,
                &mut execution_view,
                &mut review_view,
                &mut confirmation_view,
                key,
            )?
            else {
                continue;
            };
            if apply_synthetic_action(
                &mut state,
                action,
                &mut execution_view,
                &mut complete_apply_at,
            ) {
                return Ok(());
            }
        }
    })
}

fn synthetic_apply_targets() -> Vec<ExecutionTargetSpec> {
    vec![
        ExecutionTargetSpec {
            address: "terraform_data.example".to_owned(),
            actions: vec![PlanAction::Update],
        },
        ExecutionTargetSpec {
            address: "terraform_data.cache".to_owned(),
            actions: vec![PlanAction::Create],
        },
        ExecutionTargetSpec {
            address: "terraform_data.old".to_owned(),
            actions: vec![PlanAction::Delete],
        },
    ]
}

fn synthetic_apply_events() -> Vec<ExecutionEventKind> {
    vec![
        ExecutionEventKind::Log(ExecutionLogLine {
            stream: EventStream::Stdout,
            text: "Applying saved plan...".to_owned(),
        }),
        resource_event(
            "terraform_data.example",
            ResourceEventKind::ApplyStart,
            ResourceAction::Update,
            "terraform_data.example: Modifying...",
        ),
        resource_event(
            "terraform_data.example",
            ResourceEventKind::ApplyComplete,
            ResourceAction::Update,
            "terraform_data.example: Modifications complete",
        ),
        resource_event(
            "terraform_data.cache",
            ResourceEventKind::ApplyStart,
            ResourceAction::Create,
            "terraform_data.cache: Creating...",
        ),
        resource_event(
            "terraform_data.cache",
            ResourceEventKind::ApplyComplete,
            ResourceAction::Create,
            "terraform_data.cache: Creation complete",
        ),
        resource_event(
            "terraform_data.old",
            ResourceEventKind::ApplyStart,
            ResourceAction::Delete,
            "terraform_data.old: Destroying...",
        ),
        resource_event(
            "terraform_data.old",
            ResourceEventKind::ApplyComplete,
            ResourceAction::Delete,
            "terraform_data.old: Destruction complete",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::{app::execution::ExecutionStage, runtime::event_loop::test_support::terminal_text};

    struct SyntheticSession {
        state: SessionState,
        view: plan_review::PlanReviewViewState,
        confirmation_view: plan_review::ApplyConfirmationViewState,
        execution_view: execution::ExecutionViewState,
        quit_confirmation: bool,
        complete_apply_at: Option<Instant>,
    }

    impl SyntheticSession {
        fn review() -> Self {
            Self {
                state: SessionState::Review(Box::new(synthetic_review())),
                view: plan_review::PlanReviewViewState::default(),
                confirmation_view: plan_review::ApplyConfirmationViewState::default(),
                execution_view: execution::ExecutionViewState::default(),
                quit_confirmation: false,
                complete_apply_at: None,
            }
        }

        fn send(&mut self, terminal: &Terminal<TestBackend>, key: KeyEvent) -> bool {
            let action = handle_synthetic_event(
                &Event::Key(key),
                terminal,
                &self.state,
                &mut self.view,
                &mut self.confirmation_view,
                &mut self.execution_view,
                &mut self.quit_confirmation,
            )
            .expect("synthetic input should be handled");
            action.is_some_and(|action| {
                apply_synthetic_action(
                    &mut self.state,
                    action,
                    &mut self.execution_view,
                    &mut self.complete_apply_at,
                )
            })
        }

        fn press(&mut self, terminal: &Terminal<TestBackend>, code: KeyCode) -> bool {
            self.send(terminal, KeyEvent::new(code, KeyModifiers::NONE))
        }

        fn type_confirmation(&mut self, terminal: &Terminal<TestBackend>) {
            let expected = self
                .state
                .apply_confirmation()
                .expect("apply confirmation should be open")
                .review()
                .confirmation_input();
            for character in expected.chars() {
                assert!(!self.press(terminal, KeyCode::Char(character)));
            }
        }

        fn draw(&self, terminal: &mut Terminal<TestBackend>) -> String {
            super::super::event_loop::draw_with_quit_confirmation(
                &self.state,
                terminal,
                self.execution_view,
                &self.view,
                &self.confirmation_view,
                Instant::now(),
                self.quit_confirmation,
            )
            .expect("synthetic screen should render");
            terminal_text(terminal)
        }
    }

    fn terminal(width: u16, height: u16) -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(width, height)).expect("test terminal")
    }

    #[test]
    fn raw_copy_shows_the_synthetic_copy_notice() {
        let mut terminal = terminal(100, 30);
        let mut session = SyntheticSession::review();

        assert!(!session.press(&terminal, KeyCode::Char('y')));

        let text = session.draw(&mut terminal);
        assert!(text.contains("Copied."), "{text}");
    }

    #[test]
    fn overview_quit_is_confirmed_before_finishing() {
        let mut terminal = terminal(100, 30);
        let mut session = SyntheticSession::review();
        assert!(!session.press(&terminal, KeyCode::Char('s')));
        assert!(session.state.overview().is_some());

        assert!(!session.press(&terminal, KeyCode::Char('q')));

        let text = session.draw(&mut terminal);
        assert!(text.contains("Quit Terracotta?"), "{text}");
        assert!(session.press(&terminal, KeyCode::Enter));
    }

    #[test]
    fn narrow_confirmation_does_not_start_the_synthetic_apply() {
        let wide = terminal(100, 30);
        let narrow = terminal(20, 5);
        let mut session = SyntheticSession::review();
        assert!(!session.press(&wide, KeyCode::Char('a')));

        session.type_confirmation(&narrow);
        assert!(!session.press(&narrow, KeyCode::Enter));

        assert!(session.state.apply_confirmation().is_some());
        assert_eq!(session.confirmation_view.input(), "");
        assert_eq!(session.complete_apply_at, None);
    }

    #[test]
    fn confirmed_apply_records_synthetic_events_until_the_timer() {
        let terminal = terminal(100, 30);
        let mut session = SyntheticSession::review();
        assert!(!session.press(&terminal, KeyCode::Char('a')));
        session.type_confirmation(&terminal);

        assert!(!session.press(&terminal, KeyCode::Enter));

        let apply = session.state.apply().expect("synthetic apply should start");
        assert_eq!(apply.stage(), ExecutionStage::Applying);
        assert_eq!(apply.progress().targets().len(), 3);
        assert!(session.complete_apply_at.is_some());
    }

    #[test]
    fn cancelling_the_synthetic_apply_interrupts_it() {
        let terminal = terminal(100, 30);
        let mut session = SyntheticSession::review();
        assert!(!session.press(&terminal, KeyCode::Char('a')));
        session.type_confirmation(&terminal);
        assert!(!session.press(&terminal, KeyCode::Enter));

        assert!(!session.send(
            &terminal,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ));

        let apply = session
            .state
            .apply()
            .expect("synthetic apply should remain");
        assert_eq!(apply.stage(), ExecutionStage::ApplyInterrupted);
        assert_eq!(session.complete_apply_at, None);
    }
}
