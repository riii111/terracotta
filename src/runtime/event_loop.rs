use std::{
    io,
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyEvent};
use ratatui::{DefaultTerminal, Terminal, backend::Backend, layout::Rect};

use crate::{
    app::{
        copy::{CopyEffect, CopyResult, CopyTarget},
        execution::{ExecutionStage, ExecutionState},
        review::PlanReviewMessage,
        session::{self, Action, Effect, SessionOutcome, SessionState},
    },
    infra::{CancellationToken, ClipboardExecutor, terraform::SavedPlan},
    ui::features::{execution, plan_review},
};

#[allow(
    clippy::too_many_arguments,
    reason = "the event loop receives the explicit runtime resources it coordinates"
)]
pub(crate) fn run_connected(
    terminal: &mut DefaultTerminal,
    root: &Path,
    execution: ExecutionState,
    messages: &Receiver<PlanReviewMessage>,
    sender: &std::sync::mpsc::Sender<PlanReviewMessage>,
    saved_plan_slot: &Arc<Mutex<Option<SavedPlan>>>,
    cancellation: &CancellationToken,
    clipboard: &mut ClipboardExecutor,
    apply_worker: &mut Option<JoinHandle<()>>,
) -> io::Result<SessionOutcome> {
    let mut execution_view = execution::ExecutionViewState::default();
    let mut review_view = plan_review::PlanReviewViewState::default();
    let mut confirmation_view = plan_review::ApplyConfirmationViewState::default();
    let mut effects = RuntimeEffects {
        root,
        sender,
        saved_plan_slot,
        cancellation,
        clipboard,
        apply_worker,
    };
    let mut state = SessionState::new(execution);
    let mut worker_disconnected = false;
    let mut dirty = true;

    loop {
        let (outcome, received) = receive_messages(
            messages,
            &mut state,
            &mut execution_view,
            &mut worker_disconnected,
            &mut effects,
        );
        dirty |= received;
        if let Some(outcome) = outcome {
            return Ok(outcome);
        }

        let now = Instant::now();
        draw_if_needed(
            &mut state,
            terminal,
            execution_view,
            &review_view,
            &confirmation_view,
            &mut dirty,
            now,
        )?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Resize(_, _) => dirty = true,
                Event::Key(key) if key.is_press() => {
                    dirty = true;
                    if let Some(action) = handle_key_event(
                        terminal,
                        &state,
                        &mut execution_view,
                        &mut review_view,
                        &mut confirmation_view,
                        key,
                    )? {
                        if let Some(outcome) =
                            dispatch(&mut state, action, &mut execution_view, &mut effects)
                        {
                            return Ok(outcome);
                        }
                        if state
                            .apply()
                            .is_some_and(|apply| apply.stage() == ExecutionStage::Applying)
                        {
                            draw_if_needed(
                                &mut state,
                                terminal,
                                execution_view,
                                &review_view,
                                &confirmation_view,
                                &mut dirty,
                                Instant::now(),
                            )?;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn should_draw(state: &SessionState, dirty: bool, now: Instant) -> bool {
    dirty
        || state.execution().is_some()
        || state.apply().is_some_and(|apply| apply.result().is_none())
        || state
            .review()
            .is_some_and(|review| review.copy_flash_active(now) || review.copy_flash_pending())
        || state
            .apply()
            .is_some_and(|apply| apply.copy_flash_active(now) || apply.copy_flash_pending())
}

fn draw_if_needed<B: Backend>(
    state: &mut SessionState,
    terminal: &mut Terminal<B>,
    execution_view: execution::ExecutionViewState,
    review_view: &plan_review::PlanReviewViewState,
    confirmation_view: &plan_review::ApplyConfirmationViewState,
    dirty: &mut bool,
    now: Instant,
) -> Result<bool, B::Error> {
    if !should_draw(state, *dirty, now) {
        return Ok(false);
    }

    draw(
        state,
        terminal,
        execution_view,
        review_view,
        confirmation_view,
        now,
    )?;
    clear_expired_copy_flash(state, now);
    *dirty = false;
    Ok(true)
}

fn clear_expired_copy_flash(state: &mut SessionState, now: Instant) {
    match state {
        SessionState::Review(review)
            if review.copy_flash_pending() && !review.copy_flash_active(now) =>
        {
            review.clear_copy_flash();
        }
        SessionState::Apply(apply)
            if apply.copy_flash_pending() && !apply.copy_flash_active(now) =>
        {
            apply.clear_copy_flash();
        }
        SessionState::Execution(_)
        | SessionState::ApplyConfirmation(_)
        | SessionState::Review(_)
        | SessionState::Apply(_) => {}
    }
}

fn handle_key_event<B: Backend>(
    terminal: &Terminal<B>,
    state: &SessionState,
    execution_view: &mut execution::ExecutionViewState,
    review_view: &mut plan_review::PlanReviewViewState,
    confirmation_view: &mut plan_review::ApplyConfirmationViewState,
    key: KeyEvent,
) -> Result<Option<Action>, B::Error> {
    if let Some(execution) = state.execution() {
        return handle_execution_key_event(terminal, execution, execution_view, key);
    }

    if state.apply_confirmation().is_some() {
        return Ok(plan_review::apply_confirmation_key_to_input(key)
            .and_then(|input| confirmation_view.apply(input)));
    }

    if let Some(apply) = state.apply() {
        return handle_execution_key_event(terminal, apply, execution_view, key);
    }

    let Some(review) = state.review() else {
        return Ok(None);
    };
    Ok(
        match plan_review::key_to_input(key, review_view.searching()) {
            Some(plan_review::PlanReviewInput::Quit) => Some(Action::Quit),
            Some(plan_review::PlanReviewInput::Apply) => Some(Action::OpenApplyConfirmation),
            Some(plan_review::PlanReviewInput::Copy) => Some(Action::Copy(CopyTarget::Plan)),
            Some(input) => {
                let size = terminal.size()?;
                let body = plan_review::layout(
                    Rect::new(0, 0, size.width, size.height),
                    review_view.searching(),
                    review,
                );
                review_view
                    .apply(
                        input,
                        body.body(),
                        body.max_vertical(),
                        body.max_horizontal(),
                        review.review().search_query(),
                    )
                    .map(Action::ReviewSearchChanged)
            }
            None => None,
        },
    )
}

fn handle_execution_key_event<B: Backend>(
    terminal: &Terminal<B>,
    state: &ExecutionState,
    execution_view: &mut execution::ExecutionViewState,
    key: KeyEvent,
) -> Result<Option<Action>, B::Error> {
    Ok(
        match execution::execution_key_to_input(key, state.stage()) {
            Some(execution::ExecutionInput::Quit) => Some(Action::Quit),
            Some(execution::ExecutionInput::Action(action)) => Some(Action::Execution(action)),
            Some(execution::ExecutionInput::End) => {
                execution_view.end();
                None
            }
            Some(execution::ExecutionInput::Scroll(scroll)) => {
                let size = terminal.size()?;
                let layout =
                    execution::execution_layout(Rect::new(0, 0, size.width, size.height), state);
                let (current_vertical, _) =
                    execution::execution_scroll_position_with_view(state, *execution_view, &layout);
                match scroll {
                    execution::ExecutionScroll::Left
                    | execution::ExecutionScroll::Right
                    | execution::ExecutionScroll::LeftEdge
                    | execution::ExecutionScroll::RightEdge => {
                        let (current, max) =
                            execution::execution_horizontal_scroll_position_with_view(
                                *execution_view,
                                &layout,
                            );
                        execution_view.apply_horizontal_scroll(
                            scroll,
                            current,
                            max,
                            current_vertical,
                        );
                    }
                    _ => {
                        let (current, max) = execution::execution_scroll_position_with_view(
                            state,
                            *execution_view,
                            &layout,
                        );
                        execution_view.apply_scroll(scroll, current, max, layout.body().height);
                    }
                }
                None
            }
            Some(execution::ExecutionInput::Copy(target)) => Some(Action::Copy(target)),
            None => None,
        },
    )
}

fn draw<B: Backend>(
    state: &SessionState,
    terminal: &mut Terminal<B>,
    execution_view: execution::ExecutionViewState,
    review_view: &plan_review::PlanReviewViewState,
    confirmation_view: &plan_review::ApplyConfirmationViewState,
    now: Instant,
) -> Result<(), B::Error> {
    match state {
        SessionState::Execution(execution) => {
            terminal.draw(|frame| {
                execution::render_execution_with_view(frame, execution, execution_view, now);
            })?;
        }
        SessionState::Review(review) => {
            terminal.draw(|frame| plan_review::render(frame, review, review_view, now))?;
        }
        SessionState::ApplyConfirmation(confirmation) => {
            terminal.draw(|frame| {
                plan_review::render_apply_confirmation(frame, confirmation, confirmation_view);
            })?;
        }
        SessionState::Apply(execution) => {
            terminal.draw(|frame| {
                execution::render_execution_with_view(frame, execution, execution_view, now);
            })?;
        }
    }
    Ok(())
}

fn receive_messages<C: ClipboardWriter>(
    messages: &Receiver<PlanReviewMessage>,
    state: &mut SessionState,
    execution_view: &mut execution::ExecutionViewState,
    worker_disconnected: &mut bool,
    effects: &mut RuntimeEffects<'_, C>,
) -> (Option<SessionOutcome>, bool) {
    let mut received = false;
    loop {
        match messages.try_recv() {
            Ok(message) => {
                received = true;
                if let Some(outcome) = dispatch(
                    state,
                    SessionState::from_message(message),
                    execution_view,
                    effects,
                ) {
                    return (Some(outcome), received);
                }
            }
            Err(TryRecvError::Empty) => return (None, received),
            Err(TryRecvError::Disconnected) if *worker_disconnected => {
                return (None, received);
            }
            Err(TryRecvError::Disconnected) => {
                *worker_disconnected = true;
                received = true;
                let outcome = dispatch(state, Action::WorkerDisconnected, execution_view, effects);
                return (outcome, received);
            }
        }
    }
}

pub(super) fn update_session(
    state: &mut SessionState,
    action: Action,
    execution_view: &mut execution::ExecutionViewState,
    now: Instant,
) -> Option<Effect> {
    let was_apply = state.apply().is_some();
    let had_apply_result = state.apply().is_some_and(|apply| apply.result().is_some());
    let effect = session::update(state, action, now);
    let entered_apply = !was_apply && state.apply().is_some();
    let apply_result_ready =
        !had_apply_result && state.apply().is_some_and(|apply| apply.result().is_some());
    if entered_apply || apply_result_ready {
        *execution_view = execution::ExecutionViewState::default();
    }
    effect
}

fn dispatch<C: ClipboardWriter>(
    state: &mut SessionState,
    action: Action,
    execution_view: &mut execution::ExecutionViewState,
    effects: &mut RuntimeEffects<'_, C>,
) -> Option<SessionOutcome> {
    let effect = update_session(state, action, execution_view, Instant::now());
    apply_effect(state, effect, execution_view, effects)
}

fn apply_effect<C: ClipboardWriter>(
    state: &mut SessionState,
    effect: Option<Effect>,
    execution_view: &mut execution::ExecutionViewState,
    effects: &mut RuntimeEffects<'_, C>,
) -> Option<SessionOutcome> {
    match effect {
        None => None,
        Some(Effect::CancelExecution) => {
            effects.cancellation.cancel();
            None
        }
        Some(Effect::StartApply) => {
            let plan_path = effects
                .saved_plan_slot
                .lock()
                .ok()
                .and_then(|slot| slot.as_ref().map(|plan| plan.path().to_owned()));
            let Some(plan_path) = plan_path else {
                return dispatch(
                    state,
                    Action::ApplyFailed {
                        message: "The reviewed plan is no longer available.".to_owned(),
                    },
                    execution_view,
                    effects,
                );
            };
            match super::spawn_apply_worker(
                effects.root,
                &plan_path,
                effects.cancellation,
                effects.sender,
            ) {
                Ok(handle) => *effects.apply_worker = Some(handle),
                Err(error) => {
                    return dispatch(
                        state,
                        Action::ApplyFailed {
                            message: format!("failed to start the apply worker: {error}"),
                        },
                        execution_view,
                        effects,
                    );
                }
            }
            None
        }
        Some(Effect::WriteClipboard(effect)) => {
            let target = effect.target();
            let result = effects.clipboard.execute(&effect);
            dispatch(
                state,
                Action::CopyCompleted { target, result },
                execution_view,
                effects,
            )
        }
        Some(Effect::Finish(outcome)) => Some(outcome),
    }
}

trait ClipboardWriter {
    fn execute(&mut self, effect: &CopyEffect) -> CopyResult;
}

impl ClipboardWriter for ClipboardExecutor {
    fn execute(&mut self, effect: &CopyEffect) -> CopyResult {
        Self::execute(self, effect)
    }
}

struct RuntimeEffects<'a, C: ClipboardWriter = ClipboardExecutor> {
    root: &'a Path,
    sender: &'a std::sync::mpsc::Sender<PlanReviewMessage>,
    saved_plan_slot: &'a Arc<Mutex<Option<SavedPlan>>>,
    cancellation: &'a CancellationToken,
    clipboard: &'a mut C,
    apply_worker: &'a mut Option<JoinHandle<()>>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::{
        backend::TestBackend,
        style::{Color, Modifier},
    };
    use rstest::rstest;

    use super::*;
    use crate::app::{
        execution::{
            ApplyStatus, EventStream, ExecutionContext, ExecutionEvent, ExecutionEventKind,
            ExecutionLogLine,
        },
        review::{PlanMetadata, PlanReview, test_support::plan_document},
        session::{ApplyConfirmationState, ReviewSessionState},
    };

    struct DrawCase {
        name: &'static str,
        state: SessionState,
        dirty: bool,
        now: Instant,
        expected: bool,
    }

    #[derive(Debug, Clone, Copy)]
    enum CopyFlashTarget {
        Review,
        Apply,
    }

    struct TestClipboard;

    impl ClipboardWriter for TestClipboard {
        fn execute(&mut self, _effect: &CopyEffect) -> CopyResult {
            CopyResult::Written
        }
    }

    #[test]
    fn draw_decision_covers_dirty_and_runtime_states() {
        let started_at = Instant::now();

        assert_draw_cases([
            DrawCase {
                name: "dirty_confirmation",
                state: confirmation_state(),
                dirty: true,
                now: started_at,
                expected: true,
            },
            DrawCase {
                name: "clean_confirmation",
                state: confirmation_state(),
                dirty: false,
                now: started_at,
                expected: false,
            },
            DrawCase {
                name: "running_execution",
                state: SessionState::new(ExecutionState::with_context(
                    started_at,
                    ExecutionContext::loading("loading..."),
                )),
                dirty: false,
                now: started_at,
                expected: true,
            },
            DrawCase {
                name: "apply_in_progress",
                state: apply_state(started_at, None),
                dirty: false,
                now: started_at,
                expected: true,
            },
            DrawCase {
                name: "apply_succeeded",
                state: apply_state(started_at, Some(ApplyStatus::Succeeded)),
                dirty: false,
                now: started_at,
                expected: false,
            },
            DrawCase {
                name: "apply_failed",
                state: apply_state(started_at, Some(ApplyStatus::Failed)),
                dirty: false,
                now: started_at,
                expected: false,
            },
            DrawCase {
                name: "apply_interrupted",
                state: apply_state(started_at, Some(ApplyStatus::Interrupted)),
                dirty: false,
                now: started_at,
                expected: false,
            },
        ]);
    }

    #[test]
    fn draw_decision_covers_copy_flash_lifecycle() {
        let started_at = Instant::now();
        let flash_started_at = started_at + Duration::from_secs(1);
        let flash_active_at = flash_started_at + Duration::from_millis(100);
        let flash_expired_at = flash_started_at + Duration::from_millis(200);

        let mut review_flash = review_state();
        record_copy(
            &mut review_flash,
            CopyTarget::Plan,
            CopyResult::Written,
            flash_started_at,
        );

        let mut apply_flash = apply_state(started_at, Some(ApplyStatus::Succeeded));
        record_copy(
            &mut apply_flash,
            CopyTarget::Execution,
            CopyResult::Written,
            flash_started_at,
        );

        assert_draw_cases([
            DrawCase {
                name: "flash_before_start",
                state: review_state(),
                dirty: false,
                now: flash_started_at,
                expected: false,
            },
            DrawCase {
                name: "review_flash_active",
                state: review_flash.clone(),
                dirty: false,
                now: flash_active_at,
                expected: true,
            },
            DrawCase {
                name: "review_flash_expired",
                state: review_flash,
                dirty: false,
                now: flash_expired_at,
                expected: true,
            },
            DrawCase {
                name: "finished_apply_flash_active",
                state: apply_flash.clone(),
                dirty: false,
                now: flash_active_at,
                expected: true,
            },
            DrawCase {
                name: "finished_apply_flash_expired",
                state: apply_flash,
                dirty: false,
                now: flash_expired_at,
                expected: true,
            },
        ]);
    }

    fn assert_draw_cases(cases: impl IntoIterator<Item = DrawCase>) {
        for case in cases {
            assert_eq!(
                should_draw(&case.state, case.dirty, case.now),
                case.expected,
                "case: {}",
                case.name
            );
        }
    }

    #[test]
    fn confirmation_input_is_drawn_through_the_runtime_step() {
        let now = Instant::now();
        let mut state = confirmation_state();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut execution_view = execution::ExecutionViewState::default();
        let mut review_view = plan_review::PlanReviewViewState::default();
        let mut confirmation_view = plan_review::ApplyConfirmationViewState::default();
        let mut dirty = true;

        let action = handle_key_event(
            &terminal,
            &state,
            &mut execution_view,
            &mut review_view,
            &mut confirmation_view,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
        )
        .expect("confirmation input should be handled");

        assert_eq!(action, None);
        assert_eq!(confirmation_view.input(), "y");

        assert!(
            draw_if_needed(
                &mut state,
                &mut terminal,
                execution_view,
                &review_view,
                &confirmation_view,
                &mut dirty,
                now,
            )
            .expect("confirmation should render")
        );

        assert!(!dirty);
        assert!(terminal_text(&terminal).contains("Apply this plan? (yes/no): y|"));
    }

    #[rstest]
    #[case::review(CopyFlashTarget::Review)]
    #[case::apply(CopyFlashTarget::Apply)]
    fn successful_copy_flash_lifecycle_draws_through_the_runtime_step(
        #[case] target: CopyFlashTarget,
    ) {
        let started_at = Instant::now();
        let flash_active_at = started_at + Duration::from_millis(100);
        let expired_at = started_at + Duration::from_millis(200);
        let (mut state, mut terminal, execution_view, review_view, confirmation_view) =
            copy_runtime_fixture(target, CopyResult::Written, started_at);
        let mut dirty = false;
        assert!(
            draw_if_needed(
                &mut state,
                &mut terminal,
                execution_view,
                &review_view,
                &confirmation_view,
                &mut dirty,
                started_at,
            )
            .expect("copy result should render")
        );

        assert!(
            copy_target_has_flash_style(target, &terminal),
            "{}",
            terminal_text(&terminal)
        );
        assert!(
            draw_if_needed(
                &mut state,
                &mut terminal,
                execution_view,
                &review_view,
                &confirmation_view,
                &mut dirty,
                flash_active_at,
            )
            .expect("active flash should render")
        );

        assert!(copy_target_has_flash_style(target, &terminal));
        assert!(copy_flash_pending(&state, target));

        assert!(
            draw_if_needed(
                &mut state,
                &mut terminal,
                execution_view,
                &review_view,
                &confirmation_view,
                &mut dirty,
                expired_at,
            )
            .expect("expired flash should render")
        );

        assert!(!copy_flash_pending(&state, target));
        assert!(!should_draw(&state, false, expired_at));
        assert!(!copy_target_has_flash_style(target, &terminal));
        match target {
            CopyFlashTarget::Review => {
                assert!(terminal_text(&terminal).contains("Copied."));
                assert!(buffer_text_prefix_has_style(
                    &terminal,
                    "terraform_data.api",
                    "terraform_data",
                    Color::Rgb(0x11, 0x14, 0x19),
                    Color::Rgb(0xf4, 0x9e, 0x4c),
                    Modifier::BOLD,
                ));
            }
            CopyFlashTarget::Apply => {
                assert!(terminal_text(&terminal).contains("Apply complete"));
            }
        }

        assert!(
            !draw_if_needed(
                &mut state,
                &mut terminal,
                execution_view,
                &review_view,
                &confirmation_view,
                &mut dirty,
                expired_at,
            )
            .expect("static result should remain rendered")
        );
    }

    #[rstest]
    #[case::review(CopyFlashTarget::Review)]
    #[case::apply(CopyFlashTarget::Apply)]
    fn failed_copy_shows_only_the_notification_through_the_runtime_step(
        #[case] target: CopyFlashTarget,
    ) {
        let started_at = Instant::now();
        let (mut state, mut terminal, execution_view, review_view, confirmation_view) =
            copy_runtime_fixture(target, CopyResult::Failed, started_at);
        let mut dirty = true;

        assert!(
            draw_if_needed(
                &mut state,
                &mut terminal,
                execution_view,
                &review_view,
                &confirmation_view,
                &mut dirty,
                started_at,
            )
            .expect("failed copy result should render")
        );
        assert!(!copy_target_has_flash_style(target, &terminal));
        assert!(terminal_text(&terminal).contains("Copy failed: clipboard unavailable."));
        assert!(!should_draw(&state, false, started_at));
    }

    #[rstest]
    #[case::failed_end(ApplyStatus::Failed, KeyCode::End, KeyModifiers::NONE)]
    #[case::failed_alt_right(ApplyStatus::Failed, KeyCode::Char('>'), KeyModifiers::ALT)]
    #[case::interrupted_end(ApplyStatus::Interrupted, KeyCode::End, KeyModifiers::NONE)]
    #[case::interrupted_alt_right(ApplyStatus::Interrupted, KeyCode::Char('>'), KeyModifiers::ALT)]
    fn end_keys_reach_the_rendered_log_tail(
        #[case] status: ApplyStatus,
        #[case] code: KeyCode,
        #[case] modifiers: KeyModifiers,
    ) {
        let started_at = Instant::now();
        let mut state = long_apply_state(started_at, Some(status));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut execution_view = execution::ExecutionViewState::default();
        let review_view = plan_review::PlanReviewViewState::default();
        let confirmation_view = plan_review::ApplyConfirmationViewState::default();
        let key = KeyEvent::new(code, modifiers);
        let action = handle_key_event(
            &terminal,
            &state,
            &mut execution_view,
            &mut plan_review::PlanReviewViewState::default(),
            &mut plan_review::ApplyConfirmationViewState::default(),
            key,
        )
        .expect("end key should be handled");

        assert_eq!(action, None);
        let mut dirty = true;
        assert!(
            draw_if_needed(
                &mut state,
                &mut terminal,
                execution_view,
                &review_view,
                &confirmation_view,
                &mut dirty,
                started_at,
            )
            .expect("result should render")
        );
        assert!(terminal_text(&terminal).contains("tail marker"));
    }

    #[test]
    fn manual_scroll_survives_new_log_until_end_restores_following() {
        let started_at = Instant::now();
        let mut state = long_apply_state(started_at, None);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut execution_view = execution::ExecutionViewState::default();
        let mut review_view = plan_review::PlanReviewViewState::default();
        let mut confirmation_view = plan_review::ApplyConfirmationViewState::default();
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);

        assert_eq!(
            handle_key_event(
                &terminal,
                &state,
                &mut execution_view,
                &mut review_view,
                &mut confirmation_view,
                down,
            )
            .expect("scroll key should be handled"),
            None
        );
        assert!(!execution_view.follows_latest());

        let new_log = ExecutionEvent {
            received_at: started_at + Duration::from_secs(1),
            kind: ExecutionEventKind::Log(ExecutionLogLine {
                stream: EventStream::Stdout,
                text: "new tail marker".to_owned(),
            }),
        };
        let _ = update_session(
            &mut state,
            Action::ApplyWorkerEvent(new_log),
            &mut execution_view,
            started_at + Duration::from_secs(1),
        );
        assert!(!execution_view.follows_latest());

        let end = KeyEvent::new(KeyCode::End, KeyModifiers::NONE);
        assert_eq!(
            handle_key_event(
                &terminal,
                &state,
                &mut execution_view,
                &mut review_view,
                &mut confirmation_view,
                end,
            )
            .expect("end key should be handled"),
            None
        );
        assert!(execution_view.follows_latest());

        let mut dirty = true;
        draw_if_needed(
            &mut state,
            &mut terminal,
            execution_view,
            &review_view,
            &confirmation_view,
            &mut dirty,
            started_at + Duration::from_secs(1),
        )
        .expect("updated log should render");
        assert!(terminal_text(&terminal).contains("new tail marker"));
    }

    #[test]
    fn update_session_resets_only_when_apply_starts_or_finishes() {
        let now = Instant::now();
        let mut state = applyable_review_state();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut view = execution::ExecutionViewState::default();
        let mut review_view = plan_review::PlanReviewViewState::default();
        let mut confirmation_view = plan_review::ApplyConfirmationViewState::default();
        view.apply_scroll(execution::ExecutionScroll::Down, 10, 20, 10);
        view.apply_horizontal_scroll(execution::ExecutionScroll::Right, 2, 5, 10);

        let apply_action = handle_key_event(
            &terminal,
            &state,
            &mut view,
            &mut review_view,
            &mut confirmation_view,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        )
        .expect("apply key should be handled");
        assert!(
            update_session(
                &mut state,
                apply_action.expect("apply key should produce an action"),
                &mut view,
                now,
            )
            .is_none()
        );
        assert_eq!(view.horizontal(), 3);
        assert!(!view.follows_latest());

        for character in "yes".chars() {
            assert!(
                handle_key_event(
                    &terminal,
                    &state,
                    &mut view,
                    &mut review_view,
                    &mut confirmation_view,
                    KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
                )
                .expect("confirmation key should be handled")
                .is_none()
            );
        }
        let confirm_action = handle_key_event(
            &terminal,
            &state,
            &mut view,
            &mut review_view,
            &mut confirmation_view,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .expect("confirmation should be handled");
        assert!(matches!(
            update_session(
                &mut state,
                confirm_action.expect("yes should produce an action"),
                &mut view,
                now,
            ),
            Some(Effect::StartApply)
        ));
        assert_eq!(view.horizontal(), 0);
        assert_eq!(view.vertical_offset(2, 90), 2);

        assert_apply_start_path(
            &mut state,
            &mut terminal,
            &mut view,
            &mut review_view,
            &mut confirmation_view,
            now,
        );
        assert_apply_completion_and_copy_path(
            &mut state,
            &mut terminal,
            &mut view,
            &mut review_view,
            &mut confirmation_view,
            now,
        );
    }

    fn assert_apply_start_path(
        state: &mut SessionState,
        terminal: &mut Terminal<TestBackend>,
        view: &mut execution::ExecutionViewState,
        review_view: &mut plan_review::PlanReviewViewState,
        confirmation_view: &mut plan_review::ApplyConfirmationViewState,
        now: Instant,
    ) {
        for index in 0..40 {
            let text = if index == 0 {
                "apply log line 0 with enough width to exercise the production horizontal scrollbar after the result is complete".to_owned()
            } else if index == 39 {
                "tail apply marker".to_owned()
            } else {
                format!("apply log line {index}")
            };
            let _ = update_session(
                state,
                Action::ApplyWorkerEvent(ExecutionEvent {
                    received_at: now,
                    kind: ExecutionEventKind::Log(ExecutionLogLine {
                        stream: EventStream::Stdout,
                        text,
                    }),
                }),
                view,
                now,
            );
        }
        let text =
            render_apply_to_text(state, terminal, *view, review_view, confirmation_view, now);
        assert_eq!(view.horizontal(), 0);
        assert!(text.contains("tail apply marker"));

        for key in [
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        ] {
            assert!(
                handle_key_event(terminal, state, view, review_view, confirmation_view, key,)
                    .expect("manual execution key should be handled")
                    .is_none()
            );
        }
        assert!(execution_scroll_position(state, *view) > 0);
        assert!(view.horizontal() > 0);
    }

    fn assert_apply_completion_and_copy_path(
        state: &mut SessionState,
        terminal: &mut Terminal<TestBackend>,
        view: &mut execution::ExecutionViewState,
        review_view: &mut plan_review::PlanReviewViewState,
        confirmation_view: &mut plan_review::ApplyConfirmationViewState,
        now: Instant,
    ) {
        let _ = update_session(
            state,
            Action::ApplyCompleted {
                status: ApplyStatus::Succeeded,
                summary_line: None,
            },
            view,
            now,
        );
        assert_eq!(view.horizontal(), 0);
        assert_eq!(view.vertical_offset(2, 90), 2);
        let text =
            render_apply_to_text(state, terminal, *view, review_view, confirmation_view, now);
        assert!(text.contains("tail apply marker"));

        for key in [
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        ] {
            assert!(
                handle_key_event(terminal, state, view, review_view, confirmation_view, key,)
                    .expect("post-result execution key should be handled")
                    .is_none()
            );
        }
        let copied_vertical = execution_scroll_position(state, *view);
        let copied_horizontal = view.horizontal();
        assert!(copied_vertical > 0);
        assert!(copied_horizontal > 0);

        let (sender, _messages) = std::sync::mpsc::channel();
        let saved_plan_slot = Arc::new(Mutex::new(None));
        let cancellation = CancellationToken::new();
        let mut clipboard = TestClipboard;
        let mut apply_worker = None;
        let mut effects = RuntimeEffects {
            root: Path::new("/project"),
            sender: &sender,
            saved_plan_slot: &saved_plan_slot,
            cancellation: &cancellation,
            clipboard: &mut clipboard,
            apply_worker: &mut apply_worker,
        };
        assert!(
            dispatch(
                state,
                Action::Copy(CopyTarget::Execution),
                view,
                &mut effects,
            )
            .is_none()
        );
        assert!(state.apply().expect("apply state").copy_notice().is_some());
        let _ = render_apply_to_text(state, terminal, *view, review_view, confirmation_view, now);
        assert_eq!(execution_scroll_position(state, *view), copied_vertical);
        assert_eq!(view.horizontal(), copied_horizontal);
    }

    fn render_apply_to_text(
        state: &mut SessionState,
        terminal: &mut Terminal<TestBackend>,
        view: execution::ExecutionViewState,
        review_view: &plan_review::PlanReviewViewState,
        confirmation_view: &plan_review::ApplyConfirmationViewState,
        now: Instant,
    ) -> String {
        let mut dirty = true;
        assert!(
            draw_if_needed(
                state,
                terminal,
                view,
                review_view,
                confirmation_view,
                &mut dirty,
                now,
            )
            .expect("apply should render")
        );
        terminal_text(terminal)
    }

    fn execution_scroll_position(state: &SessionState, view: execution::ExecutionViewState) -> u16 {
        let apply = state.apply().expect("apply state");
        execution::execution_scroll_position_with_view(
            apply,
            view,
            &execution::execution_layout(ratatui::layout::Rect::new(0, 0, 80, 24), apply),
        )
        .0
    }

    fn terminal_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let area = buffer.area();
        let mut text = String::new();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                text.push_str(buffer.cell((x, y)).expect("test cell").symbol());
            }
            text.push('\n');
        }
        text
    }

    fn buffer_has_flash_style(terminal: &Terminal<TestBackend>) -> bool {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.bg == Color::Rgb(0xf4, 0x9e, 0x4c))
    }

    fn buffer_text_prefix_has_style(
        terminal: &Terminal<TestBackend>,
        text: &str,
        prefix: &str,
        foreground: Color,
        background: Color,
        modifier: Modifier,
    ) -> bool {
        let buffer = terminal.backend().buffer();
        let area = buffer.area();
        let symbols_per_row = |y| {
            (area.x..area.right())
                .map(|x| buffer.cell((x, y)).expect("text cell").symbol())
                .collect::<Vec<_>>()
        };
        for y in area.y..area.bottom() {
            let symbols = symbols_per_row(y);
            for start in 0..symbols.len() {
                if !symbols[start..]
                    .iter()
                    .copied()
                    .collect::<String>()
                    .starts_with(text)
                {
                    continue;
                }
                if (0..prefix.chars().count()).all(|offset| {
                    let cell = buffer
                        .cell((
                            area.x + u16::try_from(start + offset).expect("text offset"),
                            y,
                        ))
                        .expect("text cell");
                    cell.fg == foreground && cell.bg == background && cell.modifier == modifier
                }) {
                    return true;
                }
            }
        }
        false
    }

    fn copy_target_has_flash_style(
        target: CopyFlashTarget,
        terminal: &Terminal<TestBackend>,
    ) -> bool {
        match target {
            CopyFlashTarget::Review => buffer_text_prefix_has_style(
                terminal,
                "copy body marker",
                "copy body marker",
                Color::Rgb(0x11, 0x14, 0x19),
                Color::Rgb(0xf4, 0x9e, 0x4c),
                Modifier::empty(),
            ),
            CopyFlashTarget::Apply => buffer_has_flash_style(terminal),
        }
    }

    fn copy_flash_pending(state: &SessionState, target: CopyFlashTarget) -> bool {
        match target {
            CopyFlashTarget::Review => state.review().expect("review state").copy_flash_pending(),
            CopyFlashTarget::Apply => state.apply().expect("apply state").copy_flash_pending(),
        }
    }

    fn review_state() -> SessionState {
        SessionState::Review(Box::new(ReviewSessionState::new(PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document("No changes.\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
            Vec::new(),
        ))))
    }

    fn confirmation_state() -> SessionState {
        let SessionState::Review(review) = review_state() else {
            unreachable!();
        };
        SessionState::ApplyConfirmation(Box::new(ApplyConfirmationState::new(
            review.review().clone(),
        )))
    }

    fn apply_state(started_at: Instant, status: Option<ApplyStatus>) -> SessionState {
        let mut execution =
            ExecutionState::applying(started_at, ExecutionContext::loading("loading..."));
        if let Some(status) = status {
            execution.finish_apply(status, None, None, started_at);
        }
        SessionState::Apply(Box::new(execution))
    }

    fn long_apply_state(started_at: Instant, status: Option<ApplyStatus>) -> SessionState {
        let mut execution = ExecutionState::applying(
            started_at,
            ExecutionContext::loading("/project").with_workspace("default"),
        );
        for index in 0..40 {
            execution.record(ExecutionEvent {
                received_at: started_at,
                kind: ExecutionEventKind::Log(ExecutionLogLine {
                    stream: EventStream::Stdout,
                    text: if index == 39 {
                        "tail marker".to_owned()
                    } else {
                        format!("log line {index}")
                    },
                }),
            });
        }
        if let Some(status) = status {
            execution.finish_apply(status, None, None, started_at);
        }
        SessionState::Apply(Box::new(execution))
    }

    fn applyable_review_state() -> SessionState {
        SessionState::Review(Box::new(ReviewSessionState::new(PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            plan_document("Plan: 1 to add.\n".to_owned()),
            PlanMetadata::new(Vec::new(), Vec::new(), 1, 0, 0, true),
            Vec::new(),
        ))))
    }

    fn copy_flash_state(target: CopyFlashTarget, started_at: Instant) -> SessionState {
        match target {
            CopyFlashTarget::Review => {
                let mut review = PlanReview::new(
                    PathBuf::from("/project"),
                    "default".to_owned(),
                    plan_document(copy_plan_text()),
                    PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
                    Vec::new(),
                );
                review.set_search_query("terraform_data".to_owned());
                SessionState::Review(Box::new(ReviewSessionState::new(review)))
            }
            CopyFlashTarget::Apply => {
                let mut state = apply_state(started_at, None);
                if let SessionState::Apply(execution) = &mut state {
                    execution.record(ExecutionEvent {
                        received_at: started_at,
                        kind: ExecutionEventKind::Informational {
                            event_type: "log".to_owned(),
                            message: Some("flash".to_owned()),
                        },
                    });
                    execution.finish_apply(ApplyStatus::Succeeded, None, None, started_at);
                }
                state
            }
        }
    }

    fn copy_runtime_fixture(
        target: CopyFlashTarget,
        result: CopyResult,
        started_at: Instant,
    ) -> (
        SessionState,
        Terminal<TestBackend>,
        execution::ExecutionViewState,
        plan_review::PlanReviewViewState,
        plan_review::ApplyConfirmationViewState,
    ) {
        let mut state = copy_flash_state(target, started_at);
        record_copy(&mut state, target.copy_target(), result, started_at);
        let terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let execution_view = execution::ExecutionViewState::default();
        let review_view = match target {
            CopyFlashTarget::Review => copy_review_view(&state),
            CopyFlashTarget::Apply => plan_review::PlanReviewViewState::default(),
        };
        let confirmation_view = plan_review::ApplyConfirmationViewState::default();
        (
            state,
            terminal,
            execution_view,
            review_view,
            confirmation_view,
        )
    }

    fn copy_review_view(state: &SessionState) -> plan_review::PlanReviewViewState {
        let area = Rect::new(0, 0, 80, 24);
        let review = state.review().expect("review state");
        let query = review.review().search_query();
        let layout = plan_review::layout(area, false, review);
        let mut view = plan_review::PlanReviewViewState::default();
        view.apply(
            plan_review::PlanReviewInput::Down,
            area,
            layout.max_vertical(),
            layout.max_horizontal(),
            query,
        );
        view.apply(
            plan_review::PlanReviewInput::Right,
            area,
            layout.max_vertical(),
            layout.max_horizontal(),
            query,
        );
        view.apply(
            plan_review::PlanReviewInput::SearchStart,
            area,
            layout.max_vertical(),
            layout.max_horizontal(),
            query,
        );
        view
    }

    fn copy_plan_text() -> String {
        let mut lines = vec![
            "Terraform will perform the following actions:".to_owned(),
            String::new(),
            "xcopy body marker remains visible after the notification and has a long suffix for horizontal scrolling".to_owned(),
            "xterraform_data.api contains the search highlight that must return after the copy flash".to_owned(),
        ];
        lines.extend((0..40).map(|index| format!("synthetic plan page line {index}")));
        format!("{}\n", lines.join("\n"))
    }

    fn record_copy(state: &mut SessionState, target: CopyTarget, result: CopyResult, now: Instant) {
        session::update(state, Action::CopyCompleted { target, result }, now);
    }

    impl CopyFlashTarget {
        fn copy_target(self) -> CopyTarget {
            match self {
                Self::Review => CopyTarget::Plan,
                Self::Apply => CopyTarget::Execution,
            }
        }
    }
}
