use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::DefaultTerminal;

use crate::{
    app::{
        copy::CopyResult,
        execution::{
            ApplyStatus, EventStream, ExecutionAction, ExecutionContext, ExecutionEvent,
            ExecutionEventKind, ExecutionLogLine, ExecutionPhase, ExecutionState, Tool,
        },
        review::{PlanBlock, PlanBlockKind, PlanDocument, PlanLineKind, PlanMetadata, PlanReview},
        session::{Action, ApplyConfirmationState, Effect, ReviewSessionState, SessionState},
    },
    ui::{
        QuitConfirmationInput,
        features::{execution, plan_review},
        quit_confirmation_key_to_input,
    },
};

pub(super) fn run_synthetic() -> io::Result<()> {
    let mut state = SessionState::Review(Box::new(synthetic_review()));
    let mut view = plan_review::PlanReviewViewState::default();
    let mut confirmation_view = plan_review::ApplyConfirmationViewState::default();
    let mut complete_apply_at: Option<Instant> = None;
    let mut execution_view = execution::ExecutionViewState::default();
    let mut quit_confirmation = false;

    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| {
                render_synthetic(
                    frame,
                    &state,
                    &view,
                    &confirmation_view,
                    execution_view,
                    quit_confirmation,
                );
            })?;

            if complete_apply_at.is_some_and(|at| Instant::now() >= at) {
                finish_synthetic_apply(&mut state, &mut execution_view);
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
                &mut state,
                &mut view,
                &mut confirmation_view,
                &mut execution_view,
                &mut quit_confirmation,
            )?
            else {
                continue;
            };
            match super::event_loop::update_session(
                &mut state,
                action,
                &mut execution_view,
                Instant::now(),
            ) {
                Some(Effect::StartApply) => {
                    complete_apply_at = Some(Instant::now() + Duration::from_millis(250));
                }
                Some(Effect::WriteClipboard(effect)) => {
                    let target = effect.target();
                    let _ = super::event_loop::update_session(
                        &mut state,
                        Action::CopyCompleted {
                            target,
                            result: CopyResult::Written,
                        },
                        &mut execution_view,
                        Instant::now(),
                    );
                }
                Some(Effect::Finish(_)) => return Ok(()),
                Some(Effect::CancelExecution) | None => {}
            }
        }
    })
}

fn handle_synthetic_event(
    event: &Event,
    terminal: &DefaultTerminal,
    state: &mut SessionState,
    view: &mut plan_review::PlanReviewViewState,
    confirmation_view: &mut plan_review::ApplyConfirmationViewState,
    execution_view: &mut execution::ExecutionViewState,
    quit_confirmation: &mut bool,
) -> io::Result<Option<Action>> {
    let Event::Key(key) = event else {
        if let Event::Resize(width, height) = event
            && let SessionState::Review(review) = state
        {
            let layout = plan_review::layout_with_quit_confirmation(
                ratatui::layout::Rect::new(0, 0, *width, *height),
                view.searching(),
                review,
                *quit_confirmation,
            );
            view.reconcile(
                layout.body(),
                layout.max_vertical(),
                layout.max_horizontal(),
                layout.matches(),
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
                handle_synthetic_key(
                    terminal,
                    state,
                    view,
                    confirmation_view,
                    execution_view,
                    key,
                )?
            }
        }
    } else {
        handle_synthetic_key(
            terminal,
            state,
            view,
            confirmation_view,
            execution_view,
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

fn handle_synthetic_key(
    terminal: &DefaultTerminal,
    state: &mut SessionState,
    view: &mut plan_review::PlanReviewViewState,
    confirmation_view: &mut plan_review::ApplyConfirmationViewState,
    execution_view: &mut execution::ExecutionViewState,
    key: KeyEvent,
) -> io::Result<Option<Action>> {
    match state {
        SessionState::Review(review) => synthetic_review_key(terminal, view, review, key),
        SessionState::ApplyConfirmation(confirmation) => {
            if confirmation_view.overlay().is_some() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('?') => confirmation_view.close_overlay(),
                    KeyCode::Up | KeyCode::Char('k') => confirmation_view.scroll_overlay(-1),
                    KeyCode::Down | KeyCode::Char('j') => confirmation_view.scroll_overlay(1),
                    KeyCode::PageUp => confirmation_view.scroll_overlay(-8),
                    KeyCode::PageDown => confirmation_view.scroll_overlay(8),
                    KeyCode::Home => confirmation_view.overlay_top(),
                    KeyCode::End => confirmation_view.overlay_bottom(),
                    _ => {}
                }
                Ok(None)
            } else {
                Ok(synthetic_confirmation_key(
                    confirmation_view,
                    confirmation,
                    key,
                ))
            }
        }
        SessionState::Apply(execution) => {
            synthetic_execution_key(terminal, execution, execution_view, key)
        }
        SessionState::Execution(_) => Ok(None),
    }
}

fn synthetic_review() -> ReviewSessionState {
    let plan = PlanReview::new(
        PathBuf::from("/workspace/infra/prod"),
        "default".to_owned(),
        PlanDocument::with_blocks_and_line_kinds(
            "Terraform will perform the following actions:\n\n  # terraform_data.example will be updated in-place\n  ~ resource \"terraform_data.example\" {\n      ~ input = \"before\" -> \"after\"\n      note = \"searchable synthetic value\"\n    }\n\nPlan: 0 to add, 1 to change, 0 to destroy.\n"
                .to_owned(),
            vec![
                PlanBlock::new(0..2, PlanBlockKind::Common),
                PlanBlock::new(
                    2..7,
                    PlanBlockKind::Resource,
                ),
                PlanBlock::new(7..10, PlanBlockKind::Common),
            ],
            vec![
                PlanLineKind::Intro,
                PlanLineKind::Intro,
                PlanLineKind::Note,
                PlanLineKind::Body,
                PlanLineKind::Body,
                PlanLineKind::Body,
                PlanLineKind::Body,
                PlanLineKind::Body,
                PlanLineKind::Summary,
                PlanLineKind::Body,
            ],
        ),
        PlanMetadata::new(
            vec!["terraform_data.example".to_owned()],
            Vec::new(),
            0,
            1,
            0,
            true,
        ),
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

fn render_synthetic(
    frame: &mut ratatui::Frame<'_>,
    state: &SessionState,
    view: &plan_review::PlanReviewViewState,
    confirmation_view: &plan_review::ApplyConfirmationViewState,
    execution_view: execution::ExecutionViewState,
    quit_confirmation: bool,
) {
    match state {
        SessionState::Review(review) => plan_review::render_with_quit_confirmation(
            frame,
            review,
            view,
            Instant::now(),
            quit_confirmation,
        ),
        SessionState::ApplyConfirmation(confirmation) => {
            plan_review::render_apply_confirmation(frame, confirmation, confirmation_view);
        }
        SessionState::Apply(execution) | SessionState::Execution(execution) => {
            execution::render_execution_with_quit_confirmation(
                frame,
                execution,
                execution_view,
                Instant::now(),
                quit_confirmation,
            );
        }
    }
}

fn synthetic_review_key(
    terminal: &DefaultTerminal,
    view: &mut plan_review::PlanReviewViewState,
    review: &ReviewSessionState,
    key: KeyEvent,
) -> io::Result<Option<Action>> {
    if view.overlay().is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => view.close_overlay(),
            KeyCode::Up | KeyCode::Char('k') => view.scroll_overlay(-1),
            KeyCode::Down | KeyCode::Char('j') => view.scroll_overlay(1),
            KeyCode::PageUp => view.scroll_overlay(-8),
            KeyCode::PageDown => view.scroll_overlay(8),
            KeyCode::Home => view.overlay_top(),
            KeyCode::End => view.overlay_bottom(),
            _ => {}
        }
        return Ok(None);
    }
    Ok(
        match plan_review::key_to_input(
            key,
            view.searching(),
            !review.review().search_query().is_empty(),
        ) {
            Some(plan_review::PlanReviewInput::Quit) => Some(Action::Quit),
            Some(plan_review::PlanReviewInput::Apply) => Some(Action::OpenApplyConfirmation),
            Some(input) => {
                let size = terminal.size()?;
                let layout = plan_review::layout(
                    ratatui::layout::Rect::new(0, 0, size.width, size.height),
                    view.searching(),
                    review,
                );
                view.apply_with_matches(
                    input,
                    layout.body(),
                    layout.max_vertical(),
                    layout.max_horizontal(),
                    review.review().search_query(),
                    layout.matches(),
                )
                .map(Action::ReviewSearchChanged)
            }
            None => None,
        },
    )
}

fn synthetic_execution_key(
    terminal: &DefaultTerminal,
    state: &ExecutionState,
    view: &mut execution::ExecutionViewState,
    key: KeyEvent,
) -> io::Result<Option<Action>> {
    match execution::execution_key_to_input(key, state.stage(), view.logs_open()) {
        Some(execution::ExecutionInput::Quit) => Ok(Some(Action::Quit)),
        Some(execution::ExecutionInput::OpenLogs) => {
            view.open_logs();
            Ok(None)
        }
        Some(execution::ExecutionInput::CloseLogs) => {
            view.close_logs();
            Ok(None)
        }
        Some(execution::ExecutionInput::End) => {
            view.end();
            Ok(None)
        }
        Some(execution::ExecutionInput::Scroll(scroll)) => {
            let size = terminal.size()?;
            let layout = execution::execution_layout_with_view(
                ratatui::layout::Rect::new(0, 0, size.width, size.height),
                state,
                *view,
            );
            let (current_vertical, _) =
                execution::execution_scroll_position_with_view(state, *view, &layout);
            match scroll {
                execution::ExecutionScroll::Left
                | execution::ExecutionScroll::Right
                | execution::ExecutionScroll::LeftEdge
                | execution::ExecutionScroll::RightEdge => {
                    let (current, max) =
                        execution::execution_horizontal_scroll_position_with_view(*view, &layout);
                    view.apply_horizontal_scroll(scroll, current, max, current_vertical);
                }
                _ => {
                    let (current, max) =
                        execution::execution_scroll_position_with_view(state, *view, &layout);
                    view.apply_scroll(scroll, current, max, layout.body().height);
                }
            }
            Ok(None)
        }
        Some(execution::ExecutionInput::Copy(target)) => Ok(Some(Action::Copy(target))),
        Some(execution::ExecutionInput::Action(action)) => Ok(Some(Action::Execution(action))),
        None => Ok(None),
    }
}

fn synthetic_confirmation_key(
    view: &mut plan_review::ApplyConfirmationViewState,
    state: &ApplyConfirmationState,
    key: KeyEvent,
) -> Option<Action> {
    let expected = state.review().confirmation_input();
    plan_review::apply_confirmation_key_to_input(key).and_then(|input| view.apply(input, &expected))
}

fn finish_synthetic_apply(
    state: &mut SessionState,
    execution_view: &mut execution::ExecutionViewState,
) {
    let _ = super::event_loop::update_session(
        state,
        Action::ApplyCompleted {
            status: ApplyStatus::Succeeded,
            summary_line: Some(
                "Apply complete! Resources: 0 added, 1 changed, 0 destroyed.".to_owned(),
            ),
        },
        execution_view,
        Instant::now(),
    );
}

pub(super) fn run_synthetic_execution() -> io::Result<()> {
    let started = Instant::now();
    let mut state = ExecutionState::with_context(started, ExecutionContext::loading("infra/prod"));
    state.record(ExecutionEvent {
        received_at: started,
        kind: ExecutionEventKind::Phase(ExecutionPhase::Planning),
    });
    state.record(ExecutionEvent {
        received_at: started,
        kind: ExecutionEventKind::Log(ExecutionLogLine {
            stream: EventStream::Stdout,
            text: "Planning Terraform changes...".to_owned(),
        }),
    });
    let mut view = execution::ExecutionViewState::default();
    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| {
                execution::render_execution_with_quit_confirmation(
                    frame,
                    &state,
                    view,
                    Instant::now(),
                    false,
                );
            })?;
            if let Event::Key(key) = event::read()? {
                if !key.is_press() {
                    continue;
                }
                if key.code == KeyCode::Esc {
                    return Ok(());
                }
                if synthetic_execution_key(terminal, &state, &mut view, key)?
                    == Some(Action::Execution(ExecutionAction::RequestCancellation))
                {
                    state.apply(ExecutionAction::RequestCancellation);
                }
            }
        }
    })
}
