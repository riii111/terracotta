use super::*;
use crate::{
    app::{
        copy::CopyResult,
        environments::{Environment, EnvironmentAvailability, EnvironmentIdentity, PlanResult},
        execution::Tool,
        review::{PlanMetadata, PlanReview, test_support::plan_document},
        session::Effect,
    },
    ui::test_support::{buffer_text, render_to_buffer},
};
use std::path::PathBuf;

fn partial_session() -> EnvironmentSession {
    let mut environments: Vec<_> = ["a-ready", "b-error", "c-running", "d-pending"]
        .into_iter()
        .map(|name| Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from(format!("/synthetic/{name}")),
                workspace: "default".to_owned(),
            }),
        })
        .collect();
    environments.push(Environment {
        tool: Tool::Terraform,
        availability: EnvironmentAvailability::ExcludedHcp {
            directory: PathBuf::from("/synthetic/e-hcp"),
        },
    });
    let mut state = EnvironmentSession::new(environments, false);
    let first = state.start_next().unwrap();
    let review = PlanReview::new(
        PathBuf::from("/synthetic/a-ready"),
        "default".to_owned(),
        plan_document("Synthetic plan text\n".to_owned()),
        PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
        Vec::new(),
    )
    .with_apply_allowed(false)
    .with_apply_entry(false);
    state.complete(
        first,
        PlanResult::Ready {
            review: Box::new(review),
            changed: false,
        },
        Vec::new(),
    );
    let second = state.start_next().unwrap();
    state.complete(
        second,
        PlanResult::Error("Missing required variable\nPass a variable before retrying.".to_owned()),
        Vec::new(),
    );
    state.start_next();
    state
}

#[test]
fn pending_running_ready_error_and_excluded_remain_distinct_at_supported_sizes() {
    let state = partial_session();
    for size in [(80, 24), (120, 40), (160, 60)] {
        let mut view = EnvironmentView {
            selection: EnvironmentSelection {
                column: 1,
                raw: None,
            },
            ..EnvironmentView::default()
        };
        let text = buffer_text(&render_to_buffer(size, |frame| view.render(frame, &state)));
        for marker in [
            "Ready: 1/5",
            "Pending",
            "Running",
            "Error",
            "Excluded: HCP execution",
            "Missing required variable",
        ] {
            assert!(
                text.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .contains(marker),
                "{size:?}: {marker}"
            );
        }
        if size == (80, 24) {
            insta::assert_snapshot!("environment_acquisition", text);
        }
    }
}

#[test]
fn multi_environment_help_groups_actions_and_scrolls_on_small_terminals() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    view.help();

    let buffer = render_to_buffer((80, 24), |frame| view.render(frame, &state));
    let text = buffer_text(&buffer);
    assert!(text.contains("Help"));
    assert!(text.contains("Navigation"));
    assert!(text.contains("select an environment"));
    assert!(!text.contains("0 / s"));
    assert_eq!(text.matches("close").count(), 1);
    assert!(
        buffer
            .cell((0, 0))
            .expect("dimmed background")
            .modifier
            .contains(ratatui::style::Modifier::DIM)
    );
    insta::assert_snapshot!("environment_help_80x24", text);

    let narrow = buffer_text(&render_to_buffer((40, 24), |frame| {
        view.render(frame, &state);
    }));
    insta::assert_snapshot!("environment_help_40x24", narrow);

    view.dialog_scroll = u16::MAX;
    let bottom = render_to_buffer((40, 16), |frame| view.render(frame, &state));
    let bottom_text = buffer_text(&bottom);
    assert!(bottom_text.contains("Exit"));
    assert!(bottom_text.contains("quit"));
    assert_eq!(bottom_text.matches("close").count(), 1);
    insta::assert_snapshot!("environment_help_40x16_bottom", bottom_text);
}

#[test]
fn help_explains_matrix_symbols_and_missing_rows() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    view.help();

    let text = buffer_text(&render_to_buffer((120, 60), |frame| {
        view.render(frame, &state);
    }));

    assert!(text.contains("blank: resource not in this environment."));
    assert!(text.contains(".: resource present, with no change."));
    assert!(text.contains("?: environment plan not fetched yet."));
    assert!(text.contains("why: missing = present in only some environments."));
}

#[test]
fn ready_review_remains_available_and_quit_requires_confirmation_while_acquiring() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    let size = Size::new(80, 24);
    render_to_buffer((80, 24), |frame| view.render(frame, &state));
    view.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        size,
        &state,
    );
    let raw = buffer_text(&render_to_buffer((80, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(raw.contains("Synthetic plan text"));
    assert!(!raw.contains("a apply"));
    assert!(
        view.handle_key(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            size,
            &state
        )
        .is_none()
    );
    let confirmation = buffer_text(&render_to_buffer((80, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(confirmation.contains("Stop acquiring"));
    assert!(matches!(
        view.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            size,
            &state
        ),
        Some(EnvironmentInput::Interrupt)
    ));
}

#[test]
fn raw_environment_help_explains_tab_navigation_at_supported_widths() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    let size = Size::new(80, 24);

    view.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.selection.raw, Some(0));
    view.handle_key(
        KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        size,
        &state,
    );

    for (width, height) in [(80, 24), (40, 16)] {
        let text = buffer_text(&render_to_buffer((width, height), |frame| {
            view.render(frame, &state);
        }));
        let compact = text
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();

        assert!(compact.contains("Tab/"), "{width}x{height}: {text}");
        assert!(compact.contains("Shift-Tab"), "{width}x{height}: {text}");
        assert!(
            compact.contains("next/previous"),
            "{width}x{height}: {text}"
        );
        assert!(compact.contains("environment"), "{width}x{height}: {text}");
    }
}

#[test]
fn small_terminals_keep_cancel_and_quit_operable() {
    let state = partial_session();
    for size in [(0, 0), (1, 1), (16, 4), (40, 10)] {
        let mut view = EnvironmentView::default();
        render_to_buffer(size, |frame| view.render(frame, &state));
        view.handle_key(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            Size::new(size.0, size.1),
            &state,
        );
        render_to_buffer(size, |frame| view.render(frame, &state));
        assert!(matches!(
            view.handle_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                Size::new(size.0, size.1),
                &state
            ),
            Some(EnvironmentInput::Interrupt)
        ));
    }
}

#[test]
fn ready_review_keeps_position_filter_counts_and_copy_notices() {
    for size in [(80, 24), (120, 40), (160, 60)] {
        let mut state = partial_session();
        let mut view = EnvironmentView {
            selection: EnvironmentSelection {
                column: 0,
                raw: Some(0),
            },
            ..EnvironmentView::default()
        };

        let text = buffer_text(&render_to_buffer(size, |frame| view.render(frame, &state)));
        assert!(text.contains("Esc overview"), "{size:?}: {text}");
        assert!(text.contains("1/2"), "{size:?}: {text}");
        for (result, notice) in [
            (CopyResult::Written, "Copied."),
            (CopyResult::Failed, "Copy failed."),
        ] {
            let Some(EnvironmentInput::Review(index, action)) = view.handle_key(
                KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
                Size::new(size.0, size.1),
                &state,
            ) else {
                panic!("copy input should reach the environment review");
            };
            assert!(matches!(
                state.update_review(index, *action, std::time::Instant::now()),
                Some(Effect::WriteClipboard(_))
            ));
            state.update_review(
                index,
                Action::CopyCompleted {
                    target: CopyTarget::Plan,
                    result,
                },
                std::time::Instant::now(),
            );

            let text = buffer_text(&render_to_buffer(size, |frame| view.render(frame, &state)));
            assert!(text.contains(notice), "{size:?}: {text}");
            assert!(text.contains("Esc overview"), "{size:?}: {text}");
        }
        let mut filtered = partial_session();
        filtered.update_review(
            0,
            Action::ReviewSearchChanged("missing".to_owned()),
            std::time::Instant::now(),
        );

        let text = buffer_text(&render_to_buffer(size, |frame| {
            view.render(frame, &filtered);
        }));
        assert!(text.contains("No matches"), "{size:?}: {text}");
        assert!(text.contains("Esc clear"), "{size:?}: {text}");
        assert!(!text.contains("Esc overview"), "{size:?}: {text}");
    }
}

mod matrix;
