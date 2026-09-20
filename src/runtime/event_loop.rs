use std::{
    io,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyEvent};
use ratatui::{DefaultTerminal, layout::Rect};

use crate::{
    app::{
        copy::CopyTarget,
        execution::{ExecutionAction, ExecutionState},
        review::PlanReviewMessage,
        session::{self, Action, Effect, SessionOutcome, SessionState},
    },
    infra::{CancellationToken, ClipboardExecutor},
    ui::features::{execution, plan_review},
};

pub(crate) fn run_connected(
    terminal: &mut DefaultTerminal,
    execution: ExecutionState,
    messages: &Receiver<PlanReviewMessage>,
    cancellation: &CancellationToken,
    clipboard: &mut ClipboardExecutor,
) -> io::Result<SessionOutcome> {
    let mut execution_view = execution::ExecutionViewState::default();
    let mut review_view = plan_review::PlanReviewViewState::default();
    let mut state = SessionState::new(execution);
    let mut worker_disconnected = false;
    let mut dirty = true;

    loop {
        let (outcome, received) = receive_messages(
            messages,
            &mut state,
            &mut worker_disconnected,
            cancellation,
            clipboard,
        );
        dirty |= received;
        if let Some(outcome) = outcome {
            return Ok(outcome);
        }

        let now = Instant::now();
        let clear_copy_flash = state
            .review()
            .is_some_and(|review| review.copy_flash_pending() && !review.copy_flash_active(now));
        if dirty
            || state.execution().is_some()
            || state
                .review()
                .is_some_and(|review| review.copy_flash_active(now))
            || clear_copy_flash
        {
            draw(&state, terminal, execution_view, &review_view)?;
            if clear_copy_flash && let SessionState::Review(review) = &mut state {
                review.clear_copy_flash();
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
                        key,
                    )? && let Some(outcome) =
                        dispatch(&mut state, action, cancellation, clipboard)
                    {
                        return Ok(outcome);
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
    key: KeyEvent,
) -> io::Result<Option<Action>> {
    if let Some(execution) = state.execution() {
        return Ok(
            match execution::execution_key_to_input(key, execution.stage()) {
                Some(execution::ExecutionInput::Quit) => Some(Action::Quit),
                Some(execution::ExecutionInput::Action(ExecutionAction::RequestCancellation)) => {
                    Some(Action::Execution(ExecutionAction::RequestCancellation))
                }
                Some(execution::ExecutionInput::End) => {
                    execution_view.end();
                    None
                }
                Some(execution::ExecutionInput::Scroll(scroll)) => {
                    let size = terminal.size()?;
                    let body = execution::execution_layout(
                        Rect::new(0, 0, size.width, size.height),
                        execution,
                    )
                    .body();
                    match scroll {
                        execution::ExecutionScroll::Left
                        | execution::ExecutionScroll::Right
                        | execution::ExecutionScroll::LeftEdge
                        | execution::ExecutionScroll::RightEdge => {
                            let (current, max) =
                                execution::execution_horizontal_scroll_position_with_view(
                                    execution,
                                    *execution_view,
                                    body,
                                );
                            let (current_vertical, _) =
                                execution::execution_scroll_position_with_view(
                                    execution,
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
                                execution,
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
        );
    }

    let Some(review) = state.review() else {
        return Ok(None);
    };
    Ok(
        match plan_review::key_to_input(key, review_view.searching()) {
            Some(plan_review::PlanReviewInput::Quit) => Some(Action::Quit),
            Some(plan_review::PlanReviewInput::Copy) => Some(Action::Copy(CopyTarget::Plan)),
            Some(input) => {
                let size = terminal.size()?;
                let body = plan_review::layout(
                    Rect::new(0, 0, size.width, size.height),
                    review_view.searching(),
                    review,
                )
                .body();
                review_view
                    .apply(input, body, review)
                    .map(Action::ReviewSearchChanged)
            }
            None => None,
        },
    )
}

fn draw(
    state: &SessionState,
    terminal: &mut DefaultTerminal,
    execution_view: execution::ExecutionViewState,
    review_view: &plan_review::PlanReviewViewState,
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
    }
    Ok(())
}

fn receive_messages(
    messages: &Receiver<PlanReviewMessage>,
    state: &mut SessionState,
    worker_disconnected: &mut bool,
    cancellation: &CancellationToken,
    clipboard: &mut ClipboardExecutor,
) -> (Option<SessionOutcome>, bool) {
    let mut received = false;
    loop {
        match messages.try_recv() {
            Ok(message) => {
                received = true;
                if let Some(outcome) = dispatch(
                    state,
                    SessionState::from_message(message),
                    cancellation,
                    clipboard,
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
                let outcome = dispatch(state, Action::WorkerDisconnected, cancellation, clipboard);
                return (outcome, received);
            }
        }
    }
}

fn dispatch(
    state: &mut SessionState,
    action: Action,
    cancellation: &CancellationToken,
    clipboard: &mut ClipboardExecutor,
) -> Option<SessionOutcome> {
    let effect = session::update(state, action, Instant::now());
    apply_effect(state, effect, cancellation, clipboard)
}

fn apply_effect(
    state: &mut SessionState,
    effect: Option<Effect>,
    cancellation: &CancellationToken,
    clipboard: &mut ClipboardExecutor,
) -> Option<SessionOutcome> {
    match effect {
        None => None,
        Some(Effect::CancelExecution) => {
            cancellation.cancel();
            None
        }
        Some(Effect::WriteClipboard(effect)) => {
            let target = effect.target();
            let result = clipboard.execute(&effect);
            dispatch(
                state,
                Action::CopyCompleted { target, result },
                cancellation,
                clipboard,
            )
        }
        Some(Effect::Finish(outcome)) => Some(outcome),
    }
}
