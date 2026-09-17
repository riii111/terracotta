use std::{
    io,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use crate::app::{
    execution::{ExecutionAction, ExecutionStage, ExecutionState},
    plan_list::PlanListState,
    review::PlanReviewMessage,
};

use self::execution::ExecutionInput;

mod execution;
mod plan_list;
mod resource_detail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UiOutcome {
    Reviewed,
    Failed,
    Interrupted,
}

pub(crate) fn run_connected(
    terminal: &mut DefaultTerminal,
    state: ExecutionState,
    messages: &Receiver<PlanReviewMessage>,
    cancel: &mut dyn FnMut(),
) -> io::Result<UiOutcome> {
    let mut execution = Some(state);
    let mut list = None;
    let mut detail = None;
    let mut failed = false;

    loop {
        if let Some(outcome) = receive_messages(messages, &mut execution, &mut list, &mut failed)? {
            return Ok(outcome);
        }

        if let Some(state) = execution.as_ref() {
            terminal.draw(|frame| execution::render_execution(frame, state, Instant::now()))?;
        } else if let Some(state) = detail.as_ref() {
            terminal.draw(|frame| resource_detail::render_resource_detail(frame, state))?;
        } else if let Some(state) = list.as_ref() {
            terminal.draw(|frame| plan_list::render_plan_list(frame, state))?;
        }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.is_press()
        {
            if let Some(state) = execution.as_mut() {
                if let Some(outcome) = handle_execution_input(terminal, state, key, cancel)? {
                    return Ok(outcome);
                }
            } else if let Some(state) = detail.as_mut() {
                let size = terminal.size()?;
                let viewport_height = state.viewport_height(size.height);
                match resource_detail::key_to_input(key) {
                    Some(resource_detail::DetailInput::Back) => detail = None,
                    Some(resource_detail::DetailInput::Quit) => {
                        return Ok(UiOutcome::Reviewed);
                    }
                    Some(resource_detail::DetailInput::Action(action)) => {
                        state.apply(action, viewport_height);
                    }
                    None => {}
                }
            } else if let Some(state) = list.as_mut() {
                match plan_list::key_to_action(key) {
                    Some(plan_list::ListInput::Quit) => {
                        return Ok(if failed {
                            UiOutcome::Failed
                        } else {
                            UiOutcome::Reviewed
                        });
                    }
                    Some(plan_list::ListInput::Selection(action)) => state.apply(action),
                    Some(plan_list::ListInput::OpenDetail) => {
                        detail = resource_detail::ResourceDetailState::from_list(state);
                    }
                    None => {}
                }
            }
        }
    }
}

fn receive_messages(
    messages: &Receiver<PlanReviewMessage>,
    execution: &mut Option<ExecutionState>,
    list: &mut Option<PlanListState>,
    failed: &mut bool,
) -> io::Result<Option<UiOutcome>> {
    loop {
        match messages.try_recv() {
            Ok(PlanReviewMessage::Event(event)) => {
                if let Some(state) = execution.as_mut() {
                    state.record(event);
                }
            }
            Ok(PlanReviewMessage::Completed(review)) => {
                let was_cancelled = execution
                    .as_ref()
                    .is_some_and(ExecutionState::cancellation_requested);
                if was_cancelled {
                    return Ok(Some(UiOutcome::Interrupted));
                }
                let state = PlanListState::from_review(&review)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                *execution = None;
                *list = Some(state);
            }
            Ok(PlanReviewMessage::Failed {
                message,
                interrupted,
            }) => {
                if interrupted
                    || execution
                        .as_ref()
                        .is_some_and(ExecutionState::cancellation_requested)
                {
                    return Ok(Some(UiOutcome::Interrupted));
                }
                if let Some(state) = execution.as_mut() {
                    state.fail(message, Instant::now());
                    *failed = true;
                }
            }
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) if list.is_some() || *failed => return Ok(None),
            Err(TryRecvError::Disconnected) => {
                return Err(io::Error::other("plan worker stopped without a result"));
            }
        }
    }
}

fn handle_execution_input(
    terminal: &DefaultTerminal,
    state: &mut ExecutionState,
    key: crossterm::event::KeyEvent,
    cancel: &mut dyn FnMut(),
) -> io::Result<Option<UiOutcome>> {
    match execution::execution_key_to_input(key, state.stage()) {
        Some(ExecutionInput::Quit) => {
            return Ok((state.stage() == ExecutionStage::Failed).then_some(UiOutcome::Failed));
        }
        Some(ExecutionInput::Action(action)) => {
            if matches!(action, ExecutionAction::RequestCancellation) {
                cancel();
            }
            state.apply(action);
        }
        Some(ExecutionInput::Scroll(action)) => {
            let size = terminal.size()?;
            let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
            let body = execution::execution_chunks(area, state)[2];
            let (current_offset, max_offset) = execution::execution_scroll_position(state, body);
            state.apply_scroll(action, current_offset, max_offset);
        }
        None => {}
    }
    Ok(None)
}

/// Runs the development-only plan list with synthetic plan and attribution data.
///
/// # Errors
///
/// Returns an I/O error when terminal drawing or input handling fails.
pub fn run_synthetic() -> io::Result<()> {
    plan_list::run_synthetic()
}

/// Runs the development-only execution screen with synthetic event data.
///
/// # Errors
///
/// Returns an I/O error when terminal drawing or input handling fails.
pub fn run_synthetic_execution() -> io::Result<()> {
    execution::run_synthetic_execution()
}

#[cfg(test)]
mod test_support;
