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
    ui,
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
    let mut execution_view = ui::execution::ExecutionViewState::from_state(&execution);
    let mut state = SessionState::new(execution);
    let mut list_view = ListState::default();
    let mut detail_view = None;
    let mut worker_disconnected = false;

    loop {
        if let Some(outcome) = receive_messages(
            messages,
            &mut state,
            &mut worker_disconnected,
            cancellation,
            clipboard,
        )? {
            return Ok(outcome);
        }
        if let Some(outcome) = dispatch(
            &mut state,
            Action::TimeUpdated,
            Instant::now(),
            cancellation,
            clipboard,
        ) {
            return Ok(outcome);
        }

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
            if let Some(review) = state.review()
                && let Some(detail) = review.detail()
            {
                let scroll = detail_view
                    .as_ref()
                    .map_or(0, ui::resource_detail::ResourceDetailState::scroll);
                detail_view = ui::resource_detail::ResourceDetailState::from_session(
                    review.list(),
                    detail,
                    review.copy_notice(),
                    scroll,
                );
            }
        } else {
            detail_view = None;
        }

        draw(
            &mut state,
            terminal,
            &mut list_view,
            &mut detail_view,
            execution_view,
        )?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.is_press()
        {
            let now = Instant::now();
            let action = if let Some(execution) = state.execution() {
                match ui::execution::execution_key_to_input(key, execution.stage()) {
                    Some(ui::execution::ExecutionInput::Quit) => Some(Action::Quit),
                    Some(ui::execution::ExecutionInput::Action(
                        ExecutionAction::RequestCancellation,
                    )) => Some(Action::Execution(ExecutionAction::RequestCancellation)),
                    Some(ui::execution::ExecutionInput::End) => {
                        execution_view.end();
                        None
                    }
                    Some(ui::execution::ExecutionInput::Scroll(scroll)) => {
                        let size = terminal.size()?;
                        let body = ui::execution::execution_chunks(
                            Rect::new(0, 0, size.width, size.height),
                            execution,
                        )[2];
                        let (current, max) = ui::execution::execution_scroll_position_with_view(
                            execution,
                            execution_view,
                            body,
                        );
                        execution_view.apply_scroll(scroll, current, max);
                        None
                    }
                    Some(ui::execution::ExecutionInput::Copy(target)) => Some(Action::Copy(target)),
                    None => None,
                }
            } else if state
                .review()
                .is_some_and(|review| review.detail().is_some())
            {
                match ui::resource_detail::key_to_input(key) {
                    Some(ui::resource_detail::DetailInput::Back) => Some(Action::CloseDetail),
                    Some(ui::resource_detail::DetailInput::Quit) => Some(Action::Quit),
                    Some(ui::resource_detail::DetailInput::Navigate(navigation)) => {
                        if let Some(view) = detail_view.as_mut() {
                            view.reset_scroll();
                        }
                        Some(Action::Navigate(navigation))
                    }
                    Some(ui::resource_detail::DetailInput::Copy(target)) => {
                        Some(Action::Copy(target))
                    }
                    Some(ui::resource_detail::DetailInput::Action(
                        action @ (DetailAction::PageUp | DetailAction::PageDown),
                    )) => {
                        if let Some(view) = detail_view.as_mut() {
                            let size = terminal.size()?;
                            view.apply_at(
                                action,
                                size.width.saturating_sub(2),
                                size.height.saturating_sub(7),
                                now,
                            );
                        }
                        None
                    }
                    Some(ui::resource_detail::DetailInput::Action(action)) => {
                        if let Some(view) = detail_view.as_mut() {
                            let size = terminal.size()?;
                            view.apply_at(
                                action,
                                size.width.saturating_sub(2),
                                size.height.saturating_sub(7),
                                now,
                            );
                        }
                        Some(Action::Detail(action))
                    }
                    None => None,
                }
            } else if let Some(review) = state.review() {
                if review.list().searching() {
                    match ui::plan_list::search_key_to_input(key) {
                        Some(ui::plan_list::SearchInput::Quit) => Some(Action::Quit),
                        _ => ui::plan_list::search_key_to_action(review.list(), key)
                            .map(Action::List),
                    }
                } else {
                    match ui::plan_list::key_to_action(key) {
                        Some(ui::plan_list::ListInput::Quit) => Some(Action::Quit),
                        Some(ui::plan_list::ListInput::Selection(action)) => {
                            Some(Action::List(action))
                        }
                        Some(ui::plan_list::ListInput::Copy(target)) => Some(Action::Copy(target)),
                        Some(ui::plan_list::ListInput::OpenDetail) => Some(Action::OpenDetail),
                        Some(ui::plan_list::ListInput::StartSearch) => {
                            Some(Action::List(PlanListAction::BeginSearch))
                        }
                        None => None,
                    }
                }
            } else {
                None
            };

            if let Some(action) = action
                && let Some(outcome) = dispatch(&mut state, action, now, cancellation, clipboard)
            {
                return Ok(outcome);
            }
        }
    }
}

fn draw(
    state: &mut SessionState,
    terminal: &mut DefaultTerminal,
    list_view: &mut ListState,
    detail_view: &mut Option<ui::resource_detail::ResourceDetailState>,
    execution_view: ui::execution::ExecutionViewState,
) -> io::Result<()> {
    match state {
        SessionState::Execution(execution) => {
            terminal.draw(|frame| {
                ui::execution::render_execution_with_view(
                    frame,
                    execution,
                    execution_view,
                    Instant::now(),
                );
            })?;
        }
        SessionState::Review(review) => {
            if let Some(detail) = detail_view.as_mut() {
                terminal
                    .draw(|frame| ui::resource_detail::render_resource_detail(frame, detail))?;
            } else {
                terminal.draw(|frame| {
                    ui::plan_list::render_plan_list_with_state(frame, review.list(), list_view);
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
) -> io::Result<Option<SessionOutcome>> {
    loop {
        match messages.try_recv() {
            Ok(message) => {
                if let Some(outcome) = dispatch(
                    state,
                    SessionState::from_message(message),
                    Instant::now(),
                    cancellation,
                    clipboard,
                ) {
                    return Ok(Some(outcome));
                }
            }
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) if *worker_disconnected => return Ok(None),
            Err(TryRecvError::Disconnected) => {
                *worker_disconnected = true;
                if let Some(outcome) = dispatch(
                    state,
                    Action::WorkerDisconnected,
                    Instant::now(),
                    cancellation,
                    clipboard,
                ) {
                    return Ok(Some(outcome));
                }
                return Ok(None);
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
