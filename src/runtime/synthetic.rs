use std::{io, path::PathBuf, time::Instant};

use crossterm::event::{self, Event, KeyCode};

use crate::{
    app::{
        execution::{
            EventStream, ExecutionContext, ExecutionEvent, ExecutionEventKind, ExecutionLogLine,
            ExecutionPhase, ExecutionState,
        },
        review::{PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata, PlanReview},
        session::ReviewSessionState,
    },
    ui::features::{execution, plan_review},
};

pub(super) fn run_synthetic() -> io::Result<()> {
    let mut review = ReviewSessionState::new(PlanReview::new(
        PathBuf::from("infra/prod"),
        "default".to_owned(),
        PlanDocument::with_blocks(
            "Terraform will perform the following actions:\n\n  # terraform_data.example will be updated in-place\n  ~ resource \"terraform_data\" \"example\" {\n      ~ input = \"before\" -> \"after\"\n      note = \"searchable synthetic value\"\n    }\n\nPlan: 0 to add, 1 to change, 0 to destroy.\n"
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
    ));
    let mut view = plan_review::PlanReviewViewState::default();
    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| {
                plan_review::render(frame, &review, &view, Instant::now());
            })?;
            if let Event::Key(key) = event::read()?
                && key.is_press()
            {
                if !view.searching() && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    return Ok(());
                }
                if let Some(input) = plan_review::key_to_input(key, view.searching()) {
                    let size = terminal.size()?;
                    if let Some(query) = view.apply(
                        input,
                        ratatui::layout::Rect::new(
                            1,
                            3,
                            size.width.saturating_sub(2),
                            size.height.saturating_sub(5),
                        ),
                        &review,
                    ) {
                        review.set_search_query(query);
                    }
                }
            }
        }
    })
}

pub(super) fn run_synthetic_execution() -> io::Result<()> {
    let started = Instant::now();
    let mut state = ExecutionState::with_context(
        started,
        ExecutionContext::loading("infra/prod", "Git comparison paused"),
    );
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
