use std::{
    io,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use crate::{
    app::{
        execution::{ExecutionAction, ExecutionState},
        review::{DetailAction, PlanListAction, PlanReviewMessage},
        session::{self, Action, Effect, SessionOutcome, SessionState},
    },
    infra::{ClipboardExecutor, terraform::CancellationToken},
    ui::features::{execution, plan_review},
};

#[allow(
    clippy::too_many_lines,
    reason = "the event loop keeps worker, input, rendering, and effect ordering together"
)]
pub(crate) fn run_connected(
    terminal: &mut DefaultTerminal,
    execution: ExecutionState,
    messages: &Receiver<PlanReviewMessage>,
    cancellation: &CancellationToken,
    clipboard: &mut ClipboardExecutor,
) -> io::Result<SessionOutcome> {
    let mut execution_view = execution::ExecutionViewState::from_state(&execution);
    let mut state = SessionState::new(execution);
    let mut list_view = ListState::default();
    let mut detail_view = None;
    let mut worker_disconnected = false;
    let mut dirty = true;

    loop {
        let (outcome, received_message) = receive_messages(
            messages,
            &mut state,
            &mut worker_disconnected,
            cancellation,
            clipboard,
        )?;
        dirty |= received_message;
        if let Some(outcome) = outcome {
            return Ok(outcome);
        }
        let now = Instant::now();
        let reveal_present = state
            .review()
            .and_then(|review| review.detail())
            .is_some_and(|detail| detail.reveal().is_some());
        if let Some(outcome) = dispatch(
            &mut state,
            Action::TimeUpdated,
            now,
            cancellation,
            clipboard,
        ) {
            return Ok(outcome);
        }
        dirty |= reveal_present;

        let detail_is_open = state
            .review()
            .is_some_and(|review| review.detail().is_some());
        if detail_is_open {
            let size = terminal.size()?;
            if (size.width < 48 || size.height < 8)
                && let Some(outcome) = dispatch(
                    &mut state,
                    Action::DetailAreaTooSmall,
                    Instant::now(),
                    cancellation,
                    clipboard,
                )
            {
                return Ok(outcome);
            }
            if detail_view.is_none() {
                detail_view = Some(plan_review::DetailViewState::default());
            }
            if dirty
                && let Some(review) = state.review()
                && let Some(detail) = review.detail()
                && let Some(view) = detail_view.as_mut()
            {
                let size = terminal.size()?;
                plan_review::clamp_detail_scroll(
                    view,
                    review.list(),
                    detail,
                    review.copy_notice(),
                    now,
                    Rect::new(0, 0, size.width, size.height),
                );
            }
        } else if dirty {
            detail_view = None;
        }

        if dirty || state.execution().is_some() {
            draw(
                &state,
                terminal,
                &mut list_view,
                detail_view.as_ref(),
                execution_view,
            )?;
            dirty = false;
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Resize(_, _) => dirty = true,
                Event::Key(key) if key.is_press() => {
                    dirty = true;
                    let now = Instant::now();
                    let action = if let Some(execution) = state.execution() {
                        match execution::execution_key_to_input(key, execution.stage()) {
                            Some(execution::ExecutionInput::Quit) => Some(Action::Quit),
                            Some(execution::ExecutionInput::Action(
                                ExecutionAction::RequestCancellation,
                            )) => Some(Action::Execution(ExecutionAction::RequestCancellation)),
                            Some(execution::ExecutionInput::End) => {
                                execution_view.end();
                                None
                            }
                            Some(execution::ExecutionInput::Scroll(scroll)) => {
                                let size = terminal.size()?;
                                let body = execution::execution_chunks(
                                    Rect::new(0, 0, size.width, size.height),
                                    execution,
                                )[2];
                                let (current, max) = execution::execution_scroll_position_with_view(
                                    execution,
                                    execution_view,
                                    body,
                                );
                                execution_view.apply_scroll(scroll, current, max, body.height);
                                None
                            }
                            Some(execution::ExecutionInput::Copy(target)) => {
                                Some(Action::Copy(target))
                            }
                            None => None,
                        }
                    } else if state
                        .review()
                        .is_some_and(|review| review.detail().is_some())
                    {
                        match plan_review::key_to_detail_input(key) {
                            Some(plan_review::DetailInput::Back) => Some(Action::CloseDetail),
                            Some(plan_review::DetailInput::Quit) => Some(Action::Quit),
                            Some(plan_review::DetailInput::Navigate(navigation)) => {
                                if let Some(view) = detail_view.as_mut() {
                                    view.reset();
                                }
                                Some(Action::Navigate(navigation))
                            }
                            Some(plan_review::DetailInput::Copy(target)) => {
                                Some(Action::Copy(target))
                            }
                            Some(plan_review::DetailInput::Scroll(scroll)) => {
                                if let Some(view) = detail_view.as_mut() {
                                    let size = terminal.size()?;
                                    if let Some(review) = state.review()
                                        && let Some(detail) = review.detail()
                                    {
                                        plan_review::apply_detail_scroll(
                                            view,
                                            scroll,
                                            review.list(),
                                            detail,
                                            review.copy_notice(),
                                            now,
                                            Rect::new(0, 0, size.width, size.height),
                                        );
                                    }
                                }
                                None
                            }
                            Some(plan_review::DetailInput::Action(action)) => {
                                if let Some(outcome) = dispatch(
                                    &mut state,
                                    Action::Detail(action),
                                    now,
                                    cancellation,
                                    clipboard,
                                ) {
                                    return Ok(outcome);
                                }
                                if matches!(
                                    action,
                                    DetailAction::SelectPrevious
                                        | DetailAction::SelectNext
                                        | DetailAction::ToggleExpansion
                                ) && let Some(review) = state.review()
                                    && let Some(detail) = review.detail()
                                    && let Some(view) = detail_view.as_mut()
                                {
                                    let size = terminal.size()?;
                                    plan_review::ensure_detail_selection_visible(
                                        view,
                                        review.list(),
                                        detail,
                                        review.copy_notice(),
                                        now,
                                        Rect::new(0, 0, size.width, size.height),
                                    );
                                }
                                None
                            }
                            None => None,
                        }
                    } else if let Some(review) = state.review() {
                        if review.list().searching() {
                            match plan_review::search_key_to_input(key) {
                                Some(plan_review::SearchInput::Quit) => Some(Action::Quit),
                                _ => plan_review::search_key_to_action(review.list(), key)
                                    .map(Action::List),
                            }
                        } else {
                            match plan_review::key_to_list_input(key) {
                                Some(plan_review::ListInput::Quit) => Some(Action::Quit),
                                Some(plan_review::ListInput::Selection(action)) => {
                                    Some(Action::List(action))
                                }
                                Some(plan_review::ListInput::Copy(target)) => {
                                    Some(Action::Copy(target))
                                }
                                Some(plan_review::ListInput::OpenDetail) => {
                                    Some(Action::OpenDetail)
                                }
                                Some(plan_review::ListInput::StartSearch) => {
                                    Some(Action::List(PlanListAction::BeginSearch))
                                }
                                None => None,
                            }
                        }
                    } else {
                        None
                    };

                    if let Some(action) = action
                        && let Some(outcome) =
                            dispatch(&mut state, action, now, cancellation, clipboard)
                    {
                        return Ok(outcome);
                    }
                }
                _ => {}
            }
        }
    }
}

fn draw(
    state: &SessionState,
    terminal: &mut DefaultTerminal,
    list_view: &mut ListState,
    detail_view: Option<&plan_review::DetailViewState>,
    execution_view: execution::ExecutionViewState,
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
            if let Some(view) = detail_view {
                if let Some(detail) = review.detail() {
                    terminal.draw(|frame| {
                        plan_review::render_resource_detail(
                            frame,
                            review.list(),
                            detail,
                            review.copy_notice(),
                            view,
                        );
                    })?;
                }
            } else {
                terminal.draw(|frame| {
                    plan_review::render_plan_list_with_state(frame, review.list(), list_view);
                })?;
            }
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
) -> io::Result<(Option<SessionOutcome>, bool)> {
    let mut received = false;
    loop {
        match messages.try_recv() {
            Ok(message) => {
                received = true;
                if let Some(outcome) = dispatch(
                    state,
                    SessionState::from_message(message),
                    Instant::now(),
                    cancellation,
                    clipboard,
                ) {
                    return Ok((Some(outcome), received));
                }
            }
            Err(TryRecvError::Empty) => return Ok((None, received)),
            Err(TryRecvError::Disconnected) if *worker_disconnected => {
                return Ok((None, received));
            }
            Err(TryRecvError::Disconnected) => {
                *worker_disconnected = true;
                received = true;
                if let Some(outcome) = dispatch(
                    state,
                    Action::WorkerDisconnected,
                    Instant::now(),
                    cancellation,
                    clipboard,
                ) {
                    return Ok((Some(outcome), received));
                }
                return Ok((None, received));
            }
        }
    }
}

fn dispatch(
    state: &mut SessionState,
    action: Action,
    now: Instant,
    cancellation: &CancellationToken,
    clipboard: &mut ClipboardExecutor,
) -> Option<SessionOutcome> {
    let effects = session::update(state, action, now);
    apply_effects(state, effects, cancellation, clipboard)
}

fn apply_effects(
    state: &mut SessionState,
    effects: Vec<Effect>,
    cancellation: &CancellationToken,
    clipboard: &mut ClipboardExecutor,
) -> Option<SessionOutcome> {
    let mut pending = effects;
    while let Some(effect) = pending.pop() {
        match effect {
            Effect::CancelExecution => cancellation.cancel(),
            Effect::WriteClipboard(effect) => {
                let target = effect.target();
                let resource_count = effect.resource_count();
                let result = clipboard.execute(&effect);
                pending.extend(session::update(
                    state,
                    Action::CopyCompleted {
                        target,
                        resource_count,
                        result,
                    },
                    Instant::now(),
                ));
            }
            Effect::Finish(outcome) => return Some(outcome),
        }
    }
    None
}
