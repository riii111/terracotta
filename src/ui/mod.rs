use std::{
    io,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;
use ratatui::widgets::ListState;

use crate::app::{
    copy::{CopyEffect, CopyNotice, CopyResult},
    execution::{ExecutionAction, ExecutionStage, ExecutionState},
    review::{PlanListAction, PlanListState, PlanReviewMessage},
};

use self::execution::ExecutionInput;

mod execution;
mod input;
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
    execute_effect: &mut dyn FnMut(CopyEffect) -> CopyResult,
) -> io::Result<UiOutcome> {
    let mut execution = Some(state);
    let mut list = None;
    let mut list_view = ListState::default();
    let mut detail = None;
    let mut failed = false;

    loop {
        if let Some(outcome) = receive_messages(messages, &mut execution, &mut list, &mut failed)? {
            return Ok(outcome);
        }

        if let Some(state) = execution.as_ref() {
            terminal.draw(|frame| execution::render_execution(frame, state, Instant::now()))?;
        } else if let Some(state) = detail.as_mut() {
            terminal.draw(|frame| resource_detail::render_resource_detail(frame, state))?;
        } else if let Some(state) = list.as_ref() {
            terminal.draw(|frame| {
                plan_list::render_plan_list_with_state(frame, state, &mut list_view);
            })?;
        }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.is_press()
        {
            if let Some(state) = execution.as_mut() {
                if let Some(outcome) =
                    handle_execution_input(terminal, state, key, cancel, execute_effect)?
                {
                    return Ok(outcome);
                }
            } else if let Some(state) = detail.as_mut() {
                let size = terminal.size()?;
                let now = Instant::now();
                let viewport_height = state.viewport_height_at(size.height, now);
                match resource_detail::key_to_input(key) {
                    Some(resource_detail::DetailInput::Back) => {
                        if let Some(list_state) = list.as_mut() {
                            list_state.apply(PlanListAction::SelectResource(state.item_index()));
                        }
                        detail = None;
                    }
                    Some(resource_detail::DetailInput::Quit) => {
                        return Ok(UiOutcome::Reviewed);
                    }
                    Some(resource_detail::DetailInput::Navigate(navigation)) => {
                        if let Some(list_state) = list.as_mut() {
                            state.navigate(navigation, list_state);
                        }
                    }
                    Some(resource_detail::DetailInput::Copy(target)) => {
                        if let Some(notice) =
                            perform_copy(state.copy_effect(target), execute_effect)
                        {
                            state.set_copy_notice(notice);
                        }
                    }
                    Some(resource_detail::DetailInput::Action(action)) => {
                        state.apply_at(action, size.width.saturating_sub(2), viewport_height, now);
                    }
                    None => {}
                }
            } else if let Some(state) = list.as_mut() {
                if state.searching() {
                    if plan_list::handle_search_input(state, key) {
                        return Ok(UiOutcome::Reviewed);
                    }
                } else {
                    match plan_list::key_to_action(key) {
                        Some(plan_list::ListInput::Quit) => {
                            return Ok(if failed {
                                UiOutcome::Failed
                            } else {
                                UiOutcome::Reviewed
                            });
                        }
                        Some(plan_list::ListInput::Selection(action)) => state.apply(action),
                        Some(plan_list::ListInput::Copy(target)) => {
                            if let Some(notice) =
                                perform_copy(state.copy_effect(target), execute_effect)
                            {
                                state.set_copy_notice(notice);
                            }
                        }
                        Some(plan_list::ListInput::OpenDetail) => {
                            detail = resource_detail::ResourceDetailState::from_list(state);
                        }
                        Some(plan_list::ListInput::StartSearch) => {
                            state.apply(PlanListAction::BeginSearch);
                        }
                        None => {}
                    }
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
    execute_effect: &mut dyn FnMut(CopyEffect) -> CopyResult,
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
        Some(ExecutionInput::Copy(target)) => {
            if let Some(notice) = perform_copy(state.copy_effect(target), execute_effect) {
                state.set_copy_notice(notice);
            }
        }
        None => {}
    }
    Ok(None)
}

fn perform_copy(
    effect: Option<CopyEffect>,
    execute_effect: &mut dyn FnMut(CopyEffect) -> CopyResult,
) -> Option<CopyNotice> {
    let effect = effect?;
    let success_notice = effect.success_notice();
    Some(match execute_effect(effect) {
        CopyResult::Written => success_notice,
        CopyResult::Failed => CopyNotice::Failed,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::copy::CopyTarget;

    #[test]
    fn successful_copy_passes_redacted_text_and_reports_its_range() {
        let mut copied = None;
        let effect = CopyEffect::new(CopyTarget::Plan, 2, "Resources:\n  <sensitive>".to_owned());

        let notice = perform_copy(Some(effect), &mut |effect| {
            copied = Some((effect.target(), effect.text().to_owned()));
            CopyResult::Written
        });

        assert_eq!(
            copied,
            Some((CopyTarget::Plan, "Resources:\n  <sensitive>".to_owned()))
        );
        assert_eq!(
            notice,
            Some(CopyNotice::Copied {
                target: CopyTarget::Plan,
                resource_count: 2,
            })
        );
    }

    #[test]
    fn failed_copy_reports_failure_without_changing_the_effect_text() {
        let mut copied = None;
        let effect = CopyEffect::new(CopyTarget::Resource, 1, "<sensitive>".to_owned());

        let notice = perform_copy(Some(effect), &mut |effect| {
            copied = Some(effect.text().to_owned());
            CopyResult::Failed
        });

        assert_eq!(copied.as_deref(), Some("<sensitive>"));
        assert_eq!(notice, Some(CopyNotice::Failed));
    }
}
