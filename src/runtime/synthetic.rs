use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::DefaultTerminal;

use crate::{
    app::{
        execution::{
            ApplyStatus, EventStream, ExecutionContext, ExecutionEvent, ExecutionEventKind,
            ExecutionLogLine, ExecutionPhase, ExecutionState,
        },
        review::{PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata, PlanReview},
        session::{self, Action, Effect, ReviewSessionState, SessionState},
    },
    ui::features::{execution, plan_review},
};

pub(super) fn run_synthetic() -> io::Result<()> {
    let mut state = SessionState::Review(Box::new(synthetic_review()));
    let mut view = plan_review::PlanReviewViewState::default();
    let mut confirmation_input = String::new();
    let mut confirmation_cursor = 0;
    let mut complete_apply_at = None;

    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| {
                render_synthetic(
                    frame,
                    &state,
                    &view,
                    &confirmation_input,
                    confirmation_cursor,
                );
            })?;

            if complete_apply_at.is_some_and(|at| Instant::now() >= at) {
                finish_synthetic_apply(&mut state);
                complete_apply_at = None;
                continue;
            }

            if let Event::Key(key) = event::read()?
                && key.is_press()
            {
                let action = match &state {
                    SessionState::Review(review) => {
                        synthetic_review_key(terminal, &mut view, review, key)?
                    }
                    SessionState::ApplyConfirmation(_) => synthetic_confirmation_key(
                        &mut confirmation_input,
                        &mut confirmation_cursor,
                        key,
                    ),
                    SessionState::Apply(execution) => {
                        if matches!(
                            execution::execution_key_to_input(key, execution.stage()),
                            Some(execution::ExecutionInput::Quit)
                        ) {
                            Some(Action::Quit)
                        } else {
                            None
                        }
                    }
                    SessionState::Execution(_) => None,
                };
                if action == Some(Action::Quit) {
                    return Ok(());
                }
                if let Some(action) = action
                    && matches!(
                        session::update(&mut state, action, Instant::now()),
                        Some(Effect::StartApply)
                    )
                {
                    complete_apply_at = Some(Instant::now() + Duration::from_millis(250));
                }
            } else if complete_apply_at.is_some() {
                std::thread::sleep(Duration::from_millis(100));
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
                    PlanBlockKind::Resource("terraform_data.example".to_owned()),
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
    confirmation_input: &str,
    confirmation_cursor: usize,
) {
    match state {
        SessionState::Review(review) => plan_review::render(frame, review, view, Instant::now()),
        SessionState::ApplyConfirmation(confirmation) => plan_review::render_apply_confirmation(
            frame,
            confirmation,
            confirmation_input,
            confirmation_cursor,
        ),
        SessionState::Apply(execution) | SessionState::Execution(execution) => {
            execution::render_execution_with_view(
                frame,
                execution,
                execution::ExecutionViewState::default(),
                Instant::now(),
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
    Ok(match plan_review::key_to_input(key, view.searching()) {
        Some(plan_review::PlanReviewInput::Quit) => Some(Action::Quit),
        Some(plan_review::PlanReviewInput::Apply) => Some(Action::OpenApplyConfirmation),
        Some(input) => {
            let size = terminal.size()?;
            let body = ratatui::layout::Rect::new(
                1,
                3,
                size.width.saturating_sub(2),
                size.height.saturating_sub(5),
            );
            view.apply(input, body, review)
                .map(Action::ReviewSearchChanged)
        }
        None => None,
    })
}

fn synthetic_confirmation_key(
    input: &mut String,
    cursor: &mut usize,
    key: KeyEvent,
) -> Option<Action> {
    match plan_review::apply_confirmation_key_to_input(key) {
        Some(plan_review::ApplyConfirmationInput::Character(character)) => {
            input.insert(*cursor, character);
            *cursor += character.len_utf8();
            None
        }
        Some(plan_review::ApplyConfirmationInput::Backspace) => {
            if *cursor > 0 {
                let previous = input[..*cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(index, _)| index);
                input.drain(previous..*cursor);
                *cursor = previous;
            }
            None
        }
        Some(plan_review::ApplyConfirmationInput::Left) => {
            *cursor = input[..*cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index);
            None
        }
        Some(plan_review::ApplyConfirmationInput::Right) => {
            *cursor = input[*cursor..]
                .char_indices()
                .nth(1)
                .map_or(input.len(), |(index, _)| *cursor + index);
            None
        }
        Some(plan_review::ApplyConfirmationInput::Home) => {
            *cursor = 0;
            None
        }
        Some(plan_review::ApplyConfirmationInput::End) => {
            *cursor = input.len();
            None
        }
        Some(plan_review::ApplyConfirmationInput::Confirm) if input == "yes" => {
            input.clear();
            *cursor = 0;
            Some(Action::ConfirmApply)
        }
        Some(plan_review::ApplyConfirmationInput::Confirm) if input == "no" => {
            input.clear();
            *cursor = 0;
            Some(Action::CancelApply)
        }
        Some(plan_review::ApplyConfirmationInput::Cancel) => {
            input.clear();
            *cursor = 0;
            Some(Action::CancelApply)
        }
        _ => None,
    }
}

fn finish_synthetic_apply(state: &mut SessionState) {
    let _ = session::update(
        state,
        Action::ApplyCompleted {
            status: ApplyStatus::Succeeded,
            summary_line: Some(
                "Apply complete! Resources: 0 added, 1 changed, 0 destroyed.".to_owned(),
            ),
        },
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
