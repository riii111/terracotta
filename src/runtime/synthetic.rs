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
            ApplyStatus, EventStream, ExecutionContext, ExecutionEvent, ExecutionEventKind,
            ExecutionLogLine, ExecutionPhase, ExecutionState,
        },
        review::{PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata, PlanReview},
        session::{Action, Effect, ReviewSessionState, SessionState},
    },
    ui::features::{execution, plan_review},
};

pub(super) fn run_synthetic() -> io::Result<()> {
    let mut state = SessionState::Review(Box::new(synthetic_review()));
    let mut view = plan_review::PlanReviewViewState::default();
    let mut confirmation_view = plan_review::ApplyConfirmationViewState::default();
    let mut complete_apply_at: Option<Instant> = None;
    let mut execution_view = execution::ExecutionViewState::default();

    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| {
                render_synthetic(frame, &state, &view, &confirmation_view, execution_view);
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
            if event::poll(timeout)? {
                let event = event::read()?;
                if let Event::Resize(width, height) = event {
                    if let SessionState::Review(review) = &state {
                        let layout = plan_review::layout(
                            ratatui::layout::Rect::new(0, 0, width, height),
                            view.searching(),
                            review,
                        );
                        view.reconcile(
                            layout.body(),
                            layout.max_vertical(),
                            layout.max_horizontal(),
                            layout.matches(),
                        );
                    }
                    continue;
                }
                let Event::Key(key) = event else {
                    continue;
                };
                if !key.is_press() {
                    continue;
                }
                let action = match &mut state {
                    SessionState::Review(review) => {
                        synthetic_review_key(terminal, &mut view, review, key)?
                    }
                    SessionState::ApplyConfirmation(_) => {
                        synthetic_confirmation_key(&mut confirmation_view, key)
                    }
                    SessionState::Apply(execution) => {
                        synthetic_execution_key(terminal, execution, &mut execution_view, key)?
                    }
                    SessionState::Execution(_) => None,
                };
                let Some(action) = action else {
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
        }
    })
}

fn synthetic_review() -> ReviewSessionState {
    ReviewSessionState::new(PlanReview::new(
        PathBuf::from("infra/prod"),
        "default".to_owned(),
        PlanDocument::with_blocks(
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
    ))
}

fn render_synthetic(
    frame: &mut ratatui::Frame<'_>,
    state: &SessionState,
    view: &plan_review::PlanReviewViewState,
    confirmation_view: &plan_review::ApplyConfirmationViewState,
    execution_view: execution::ExecutionViewState,
) {
    match state {
        SessionState::Review(review) => plan_review::render(frame, review, view, Instant::now()),
        SessionState::ApplyConfirmation(confirmation) => {
            plan_review::render_apply_confirmation(frame, confirmation, confirmation_view);
        }
        SessionState::Apply(execution) | SessionState::Execution(execution) => {
            execution::render_execution_with_view(frame, execution, execution_view, Instant::now());
        }
    }
}

fn synthetic_review_key(
    terminal: &DefaultTerminal,
    view: &mut plan_review::PlanReviewViewState,
    review: &ReviewSessionState,
    key: KeyEvent,
) -> io::Result<Option<Action>> {
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
    match execution::execution_key_to_input(key, state.stage()) {
        Some(execution::ExecutionInput::Quit) => Ok(Some(Action::Quit)),
        Some(execution::ExecutionInput::End) => {
            view.end();
            Ok(None)
        }
        Some(execution::ExecutionInput::Scroll(scroll)) => {
            let size = terminal.size()?;
            let layout = execution::execution_layout(
                ratatui::layout::Rect::new(0, 0, size.width, size.height),
                state,
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
    key: KeyEvent,
) -> Option<Action> {
    plan_review::apply_confirmation_key_to_input(key).and_then(|input| view.apply(input))
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
    let view = execution::ExecutionViewState::default();
    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| {
                execution::render_execution_with_view(frame, &state, view, Instant::now());
            })?;
            if let Event::Key(key) = event::read()?
                && key.is_press()
                && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
            {
                return Ok(());
            }
        }
    })
}
