use std::{
    io,
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyEvent};
use ratatui::{DefaultTerminal, layout::Rect};

use crate::{
    app::{
        copy::CopyTarget,
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
        let (outcome, received) =
            receive_messages(messages, &mut state, &mut worker_disconnected, &mut effects);
        dirty |= received;
        if let Some(outcome) = outcome {
            return Ok(outcome);
        }

        let now = Instant::now();
        let clear_copy_flash = state
            .review()
            .is_some_and(|review| review.copy_flash_pending() && !review.copy_flash_active(now));
        let clear_apply_copy_flash = state
            .apply()
            .is_some_and(|apply| apply.copy_flash_pending() && !apply.copy_flash_active(now));
        if dirty
            || state.execution().is_some()
            || state.apply_confirmation().is_some()
            || state.apply().is_some()
            || state
                .review()
                .is_some_and(|review| review.copy_flash_active(now))
            || state
                .apply()
                .is_some_and(|apply| apply.copy_flash_active(now))
            || clear_copy_flash
            || clear_apply_copy_flash
        {
            draw(
                &state,
                terminal,
                execution_view,
                &review_view,
                &confirmation_view,
            )?;
            if clear_copy_flash && let SessionState::Review(review) = &mut state {
                review.clear_copy_flash();
            }
            if clear_apply_copy_flash && let SessionState::Apply(apply) = &mut state {
                apply.clear_copy_flash();
            }
            dirty = false;
        }

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
                        if let Some(outcome) = dispatch(&mut state, action, &mut effects) {
                            return Ok(outcome);
                        }
                        if state
                            .apply()
                            .is_some_and(|apply| apply.stage() == ExecutionStage::Applying)
                        {
                            draw(
                                &state,
                                terminal,
                                execution_view,
                                &review_view,
                                &confirmation_view,
                            )?;
                            dirty = false;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn handle_key_event(
    terminal: &DefaultTerminal,
    state: &SessionState,
    execution_view: &mut execution::ExecutionViewState,
    review_view: &mut plan_review::PlanReviewViewState,
    confirmation_view: &mut plan_review::ApplyConfirmationViewState,
    key: KeyEvent,
) -> io::Result<Option<Action>> {
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

fn handle_execution_key_event(
    terminal: &DefaultTerminal,
    state: &ExecutionState,
    execution_view: &mut execution::ExecutionViewState,
    key: KeyEvent,
) -> io::Result<Option<Action>> {
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
                let body =
                    execution::execution_layout(Rect::new(0, 0, size.width, size.height), state)
                        .body();
                let (current_vertical, _) =
                    execution::execution_scroll_position_with_view(state, *execution_view, body);
                match scroll {
                    execution::ExecutionScroll::Left
                    | execution::ExecutionScroll::Right
                    | execution::ExecutionScroll::LeftEdge
                    | execution::ExecutionScroll::RightEdge => {
                        let (current, max) =
                            execution::execution_horizontal_scroll_position_with_view(
                                state,
                                *execution_view,
                                body,
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
                            body,
                        );
                        execution_view.apply_scroll(scroll, current, max, body.height);
                    }
                }
                None
            }
            Some(execution::ExecutionInput::Copy(target)) => Some(Action::Copy(target)),
            None => None,
        },
    )
}

fn draw(
    state: &SessionState,
    terminal: &mut DefaultTerminal,
    execution_view: execution::ExecutionViewState,
    review_view: &plan_review::PlanReviewViewState,
    confirmation_view: &plan_review::ApplyConfirmationViewState,
) -> io::Result<()> {
    match state {
        SessionState::Execution(execution) => {
            terminal.draw(|frame| {
                execution::render_execution_with_view(
                    frame,
                    execution,
                    execution_view,
                    Instant::now(),
                );
            })?;
        }
        SessionState::Review(review) => {
            terminal.draw(|frame| {
                plan_review::render(frame, review, review_view, Instant::now());
            })?;
        }
        SessionState::ApplyConfirmation(confirmation) => {
            terminal.draw(|frame| {
                plan_review::render_apply_confirmation(frame, confirmation, confirmation_view);
            })?;
        }
        SessionState::Apply(execution) => {
            terminal.draw(|frame| {
                execution::render_execution_with_view(
                    frame,
                    execution,
                    execution_view,
                    Instant::now(),
                );
            })?;
        }
    }
    Ok(())
}

fn receive_messages(
    messages: &Receiver<PlanReviewMessage>,
    state: &mut SessionState,
    worker_disconnected: &mut bool,
    effects: &mut RuntimeEffects<'_>,
) -> (Option<SessionOutcome>, bool) {
    let mut received = false;
    loop {
        match messages.try_recv() {
            Ok(message) => {
                received = true;
                if let Some(outcome) = dispatch(state, SessionState::from_message(message), effects)
                {
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
                let outcome = dispatch(state, Action::WorkerDisconnected, effects);
                return (outcome, received);
            }
        }
    }
}

fn dispatch(
    state: &mut SessionState,
    action: Action,
    effects: &mut RuntimeEffects<'_>,
) -> Option<SessionOutcome> {
    let effect = session::update(state, action, Instant::now());
    apply_effect(state, effect, effects)
}

fn apply_effect(
    state: &mut SessionState,
    effect: Option<Effect>,
    effects: &mut RuntimeEffects<'_>,
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
                        effects,
                    );
                }
            }
            None
        }
        Some(Effect::WriteClipboard(effect)) => {
            let target = effect.target();
            let result = effects.clipboard.execute(&effect);
            dispatch(state, Action::CopyCompleted { target, result }, effects)
        }
        Some(Effect::Finish(outcome)) => Some(outcome),
    }
}

struct RuntimeEffects<'a> {
    root: &'a Path,
    sender: &'a std::sync::mpsc::Sender<PlanReviewMessage>,
    saved_plan_slot: &'a Arc<Mutex<Option<SavedPlan>>>,
    cancellation: &'a CancellationToken,
    clipboard: &'a mut ClipboardExecutor,
    apply_worker: &'a mut Option<JoinHandle<()>>,
}
