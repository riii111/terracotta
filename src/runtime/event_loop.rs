use std::{
    io,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyEvent};
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use crate::{
    app::{
        execution::{ExecutionAction, ExecutionState},
        review::{DetailAction, PlanListAction, PlanReviewMessage},
        session::{self, Action, Effect, ReviewSessionState, SessionOutcome, SessionState},
    },
    infra::{CancellationToken, ClipboardExecutor},
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
    let mut execution_view = execution::ExecutionViewState::default();
    let mut state = SessionState::new(execution);
    let mut list_view = ListState::default();
    let mut detail_view = None;
    let mut diagnostics_view = None;
    let mut worker_disconnected = false;
    let mut dirty = true;

    loop {
        let (outcome, received_message) = receive_messages(
            messages,
            &mut state,
            &mut worker_disconnected,
            cancellation,
            clipboard,
        );
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

        let diagnostics_is_open = state
            .review()
            .is_some_and(|review| review.diagnostics().is_open());
        let detail_is_open = state
            .review()
            .is_some_and(|review| review.detail().is_some());
        if diagnostics_is_open {
            detail_view = None;
            if diagnostics_view.is_none() {
                diagnostics_view = Some(plan_review::DiagnosticsViewState::default());
            }
            if dirty
                && let Some(review) = state.review()
                && let Some(view) = diagnostics_view.as_mut()
            {
                let size = terminal.size()?;
                plan_review::clamp_diagnostics_scroll(
                    view,
                    review.diagnostics(),
                    Rect::new(0, 0, size.width, size.height),
                );
            }
        } else if detail_is_open {
            diagnostics_view = None;
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
            diagnostics_view = None;
        }

        if dirty || state.execution().is_some() {
            draw(
                &state,
                terminal,
                &mut list_view,
                detail_view.as_ref(),
                diagnostics_view.as_ref(),
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
                    let outcome = if let Some(execution) = state.execution() {
                        handle_execution_key(key, execution, &mut execution_view, terminal)?
                            .and_then(|action| {
                                dispatch(&mut state, action, now, cancellation, clipboard)
                            })
                    } else if state
                        .review()
                        .is_some_and(|review| review.diagnostics().is_open())
                    {
                        handle_diagnostics_key(key, &state, &mut diagnostics_view, terminal)?
                            .and_then(|action| {
                                dispatch(&mut state, action, now, cancellation, clipboard)
                            })
                    } else if state
                        .review()
                        .is_some_and(|review| review.detail().is_some())
                    {
                        if let Some(view) = detail_view.as_mut() {
                            handle_detail_key(
                                key,
                                &mut state,
                                view,
                                terminal,
                                cancellation,
                                clipboard,
                                now,
                            )?
                        } else {
                            None
                        }
                    } else if let Some(review) = state.review() {
                        handle_list_key(key, review, &mut diagnostics_view).and_then(|action| {
                            dispatch(&mut state, action, now, cancellation, clipboard)
                        })
                    } else {
                        None
                    };

                    if let Some(outcome) = outcome {
                        return Ok(outcome);
                    }
                }
                _ => {}
            }
        }
    }
}

fn handle_execution_key(
    key: KeyEvent,
    state: &ExecutionState,
    view: &mut execution::ExecutionViewState,
    terminal: &DefaultTerminal,
) -> io::Result<Option<Action>> {
    match execution::execution_key_to_input(key, state.stage()) {
        Some(execution::ExecutionInput::Quit) => Ok(Some(Action::Quit)),
        Some(execution::ExecutionInput::Action(ExecutionAction::RequestCancellation)) => Ok(Some(
            Action::Execution(ExecutionAction::RequestCancellation),
        )),
        Some(execution::ExecutionInput::End) => {
            view.end();
            Ok(None)
        }
        Some(execution::ExecutionInput::Scroll(scroll)) => {
            let size = terminal.size()?;
            let body =
                execution::execution_layout(Rect::new(0, 0, size.width, size.height), state).body();
            let (current, max) = execution::execution_scroll_position_with_view(state, *view, body);
            view.apply_scroll(scroll, current, max, body.height);
            Ok(None)
        }
        Some(execution::ExecutionInput::Copy(target)) => Ok(Some(Action::Copy(target))),
        None => Ok(None),
    }
}

fn handle_diagnostics_key(
    key: KeyEvent,
    state: &SessionState,
    view: &mut Option<plan_review::DiagnosticsViewState>,
    terminal: &DefaultTerminal,
) -> io::Result<Option<Action>> {
    match plan_review::key_to_diagnostics_input(key) {
        Some(plan_review::DiagnosticsInput::Back) => {
            *view = None;
            Ok(Some(Action::CloseDiagnostics))
        }
        Some(plan_review::DiagnosticsInput::Quit) => Ok(Some(Action::Quit)),
        Some(plan_review::DiagnosticsInput::Scroll(scroll)) => {
            if let Some(view) = view.as_mut()
                && let Some(review) = state.review()
            {
                let size = terminal.size()?;
                plan_review::apply_diagnostics_scroll(
                    view,
                    scroll,
                    review.diagnostics(),
                    Rect::new(0, 0, size.width, size.height),
                );
            }
            Ok(None)
        }
        None => Ok(None),
    }
}

fn handle_detail_key(
    key: KeyEvent,
    state: &mut SessionState,
    view: &mut plan_review::DetailViewState,
    terminal: &DefaultTerminal,
    cancellation: &CancellationToken,
    clipboard: &mut ClipboardExecutor,
    now: Instant,
) -> io::Result<Option<SessionOutcome>> {
    match plan_review::key_to_detail_input(key) {
        Some(plan_review::DetailInput::Back) => Ok(dispatch(
            state,
            Action::CloseDetail,
            now,
            cancellation,
            clipboard,
        )),
        Some(plan_review::DetailInput::Quit) => {
            Ok(dispatch(state, Action::Quit, now, cancellation, clipboard))
        }
        Some(plan_review::DetailInput::Navigate(navigation)) => {
            view.reset();
            Ok(dispatch(
                state,
                Action::Navigate(navigation),
                now,
                cancellation,
                clipboard,
            ))
        }
        Some(plan_review::DetailInput::Copy(target)) => Ok(dispatch(
            state,
            Action::Copy(target),
            now,
            cancellation,
            clipboard,
        )),
        Some(plan_review::DetailInput::ToggleSources) => {
            if let Some(review) = state.review()
                && !review.list().source_files().is_empty()
            {
                view.toggle_sources();
                let size = terminal.size()?;
                if let Some(detail) = review.detail() {
                    plan_review::clamp_detail_scroll(
                        view,
                        review.list(),
                        detail,
                        review.copy_notice(),
                        now,
                        Rect::new(0, 0, size.width, size.height),
                    );
                }
            }
            Ok(None)
        }
        Some(plan_review::DetailInput::Scroll(scroll)) => {
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
            Ok(None)
        }
        Some(plan_review::DetailInput::Action(action)) => {
            if let Some(outcome) =
                dispatch(state, Action::Detail(action), now, cancellation, clipboard)
            {
                return Ok(Some(outcome));
            }
            if matches!(
                action,
                DetailAction::SelectPrevious
                    | DetailAction::SelectNext
                    | DetailAction::ToggleExpansion
            ) && let Some(review) = state.review()
                && let Some(detail) = review.detail()
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
            Ok(None)
        }
        None => Ok(None),
    }
}

fn handle_list_key(
    key: KeyEvent,
    review: &ReviewSessionState,
    diagnostics_view: &mut Option<plan_review::DiagnosticsViewState>,
) -> Option<Action> {
    if review.list().searching() {
        return match plan_review::search_key_to_input(key) {
            Some(plan_review::SearchInput::Quit) => Some(Action::Quit),
            _ => plan_review::search_key_to_action(review.list(), key).map(Action::List),
        };
    }

    match plan_review::key_to_list_input(key) {
        Some(plan_review::ListInput::Quit) => Some(Action::Quit),
        Some(plan_review::ListInput::Selection(action)) => Some(Action::List(action)),
        Some(plan_review::ListInput::Copy(target)) => Some(Action::Copy(target)),
        Some(plan_review::ListInput::OpenDetail) => Some(Action::OpenDetail),
        Some(plan_review::ListInput::OpenDiagnostics) => {
            diagnostics_view
                .get_or_insert_with(Default::default)
                .reset();
            Some(Action::OpenDiagnostics)
        }
        Some(plan_review::ListInput::StartSearch) => {
            Some(Action::List(PlanListAction::BeginSearch))
        }
        None => None,
    }
}

fn draw(
    state: &SessionState,
    terminal: &mut DefaultTerminal,
    list_view: &mut ListState,
    detail_view: Option<&plan_review::DetailViewState>,
    diagnostics_view: Option<&plan_review::DiagnosticsViewState>,
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
            if review.diagnostics().is_open() {
                if let Some(view) = diagnostics_view {
                    terminal.draw(|frame| {
                        plan_review::render_diagnostics(frame, review.diagnostics(), *view);
                    })?;
                }
            } else if let Some(view) = detail_view {
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
                    plan_review::render_plan_list_with_diagnostics(
                        frame,
                        review.list(),
                        review.diagnostics(),
                        review.copy_notice(),
                        list_view,
                    );
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
) -> (Option<SessionOutcome>, bool) {
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
                if let Some(outcome) = dispatch(
                    state,
                    Action::WorkerDisconnected,
                    Instant::now(),
                    cancellation,
                    clipboard,
                ) {
                    return (Some(outcome), received);
                }
                return (None, received);
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
    let effect = session::update(state, action, now);
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
            let resource_count = effect.resource_count();
            let result = clipboard.execute(&effect);
            dispatch(
                state,
                Action::CopyCompleted {
                    target,
                    resource_count,
                    result,
                },
                Instant::now(),
                cancellation,
                clipboard,
            )
        }
        Some(Effect::Finish(outcome)) => Some(outcome),
    }
}
