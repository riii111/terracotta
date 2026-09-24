use super::*;
use crate::{
    app::{
        copy::CopyResult,
        environments::{Environment, EnvironmentAvailability, EnvironmentIdentity, PlanResult},
        execution::Tool,
        review::{PlanMetadata, PlanReview, test_support::plan_document},
        session::Effect,
    },
    ui::{
        test_support::{buffer_text, render_to_buffer},
        theme,
    },
};
use ratatui::style::{Color, Modifier};
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

fn overview_plan_session(names: &[&str]) -> EnvironmentSession {
    use crate::app::{
        plan::{Plan, PlanAction, ResourceChange, ResourceChangeKind, ResourceMode},
        review::{PlanBlock, PlanBlockKind, PlanDocument, PlanLineKind},
    };

    let environments = names
        .iter()
        .map(|name| Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from(format!("/synthetic/{name}")),
                workspace: "default".to_owned(),
            }),
        })
        .collect();
    let mut state = EnvironmentSession::new(environments, false);
    let mut lines = (0..45)
        .map(|line| format!("PLAN LINE {line:02}"))
        .collect::<Vec<_>>();
    lines[20] = "PLAN LINE 20 # terraform_data.api will be updated in-place".to_owned();
    let text = lines.join("\n");

    for name in names {
        let work = state.start_next().expect("environment should start");
        let document = PlanDocument::with_blocks_and_line_kinds(
            text.clone(),
            vec![
                PlanBlock::new(0..20, PlanBlockKind::Common),
                PlanBlock::with_addresses(
                    20..21,
                    PlanBlockKind::Resource,
                    vec!["terraform_data.api".to_owned()],
                ),
                PlanBlock::new(21..45, PlanBlockKind::Common),
            ],
            vec![PlanLineKind::Body; 45],
        );
        let mut plan = Plan::empty();
        plan.resource_changes.push(ResourceChange {
            address: "terraform_data.api".to_owned(),
            provider: None,
            resource_type: Some("terraform_data".to_owned()),
            resource_name: Some("api".to_owned()),
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: None,
            after: None,
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
            previous_address: None,
            importing: None,
        });
        let review = PlanReview::new(
            PathBuf::from(format!("/synthetic/{name}")),
            "default".to_owned(),
            document,
            PlanMetadata::new(Vec::new(), Vec::new(), 0, 0, 0, false),
            Vec::new(),
        )
        .with_plan(plan)
        .with_apply_allowed(false)
        .with_apply_entry(false);
        state.complete(
            work,
            PlanResult::Ready {
                review: Box::new(review),
                changed: true,
            },
            Vec::new(),
        );
    }

    state
}

fn matrix_header(text: &str) -> &str {
    text.lines()
        .find(|line| line.contains("Address"))
        .expect("matrix header should be rendered")
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
        let markers = if size.0 >= 120 {
            vec![
                "Pending",
                "Running",
                "Ready",
                "Error",
                "Excluded",
                "Missing required variable",
            ]
        } else {
            vec!["b-error", "Error", "Missing required variable"]
        };
        for marker in markers {
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
fn relations_explain_pending_running_error_and_excluded_environments() {
    let state = partial_session();
    let size = Size::new(120, 40);
    let mut view = EnvironmentView {
        selection: EnvironmentSelection {
            column: 1,
            raw: None,
        },
        ..EnvironmentView::default()
    };

    let error = buffer_text(&render_to_buffer((size.width, size.height), |frame| {
        view.render(frame, &state);
    }));
    assert!(error.contains("b-error · whole env"), "{error}");
    assert!(error.contains("Plan failed"), "{error}");

    for (next, status, explanation) in [
        (2, "c-running", "Plan running"),
        (3, "d-pending", "Plan pending"),
        (4, "e-hcp", "Plan excluded"),
    ] {
        view.select_environment(next);
        let rendered = buffer_text(&render_to_buffer((size.width, size.height), |frame| {
            view.render(frame, &state);
        }));
        assert!(
            rendered.contains(&format!("{status} · whole env")),
            "{rendered}"
        );
        assert!(rendered.contains(explanation), "{rendered}");
    }
}

#[test]
fn multi_environment_help_groups_actions_and_scrolls_on_small_terminals() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    view.help();

    for (width, height) in [(40, 16), (40, 24), (80, 24), (120, 40), (160, 60)] {
        let buffer = render_to_buffer((width, height), |frame| view.render(frame, &state));
        let text = buffer_text(&buffer);
        assert!(text.contains("Help"), "{width}x{height}: {text}");
        assert!(text.contains("Current"), "{width}x{height}: {text}");
        if height <= 24 {
            assert!(text.contains("↑ / ↓ / j / k"), "{width}x{height}: {text}");
        } else {
            assert!(text.contains("Comparison"), "{width}x{height}: {text}");
        }
        if (width, height) == (80, 24) {
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(text.contains("focus Differs"));
            assert!(!text.contains("1 / 2"));
            assert!(!text.contains("include or exclude"));
            assert!(!text.contains("toggle the Envs sidebar"));
            assert!(!text.contains("Tab"));
            assert!(normalized.contains("expand or collapse the selected group"));
            assert!(!text.contains("environment filter"));
        }
        assert!(text.contains("Esc"), "{width}x{height}: {text}");
        if width == 80 {
            assert!(!text.contains("1 opens Envs"), "{width}x{height}: {text}");
            assert!(
                !text.contains("toggle the Envs sidebar"),
                "{width}x{height}: {text}"
            );
        }
        if width >= 90 {
            assert!(text.contains("1 / 2"), "{width}x{height}: {text}");
            assert!(
                text.contains("toggle the Envs sidebar"),
                "{width}x{height}: {text}"
            );
        }
        if width == 80 {
            assert!(
                buffer
                    .cell((0, 0))
                    .expect("dimmed background")
                    .modifier
                    .contains(ratatui::style::Modifier::DIM)
            );
        }
        insta::assert_snapshot!(format!("environment_help_{width}x{height}"), text);
    }

    let wide = buffer_text(&render_to_buffer((160, 60), |frame| {
        view.render(frame, &state);
    }));
    assert!(wide.contains("environment"), "{wide}");
    assert!(
        wide.contains("compare only the selected environment / all environments"),
        "{wide}"
    );

    view.dialog_scroll = u16::MAX;
    let bottom = render_to_buffer((40, 16), |frame| view.render(frame, &state));
    let bottom_text = buffer_text(&bottom);
    assert!(bottom_text.contains("Excluded"));
    assert!(bottom_text.contains("Scope"));
    assert_eq!(bottom_text.matches("close").count(), 1);
    insta::assert_snapshot!("environment_help_40x16_bottom", bottom_text);
}

#[test]
fn help_scroll_keys_do_not_reach_the_environment_overview() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    let size = Size::new(80, 24);

    view.handle_key(
        KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        size,
        &state,
    );
    for character in ['h', 'l', 'g', 'G'] {
        view.handle_key(
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            size,
            &state,
        );
    }
    assert!(view.dialog.is_some());
    assert_eq!(view.dialog_scroll, 0);
    assert_eq!(view.focus, EnvironmentPane::Matrix);
    view.handle_key(
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.dialog_scroll, 1);
    view.handle_key(
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.dialog_scroll, 2);
    view.handle_key(
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.dialog_scroll, 6);
    view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), size, &state);
    assert_eq!(view.dialog_scroll, 5);
    view.handle_key(
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.dialog_scroll, 4);
    view.handle_key(
        KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.dialog_scroll, 0);

    view.handle_key(
        KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        size,
        &state,
    );
    view.handle_key(
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.selection.column, 0);
    assert_eq!(view.matrix.filter(), "");
    assert!(!view.matrix.searching());

    view.handle_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        size,
        &state,
    );
    assert!(view.dialog.is_none());
    view.handle_key(
        KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.selection.column, 0);
    assert_eq!(view.focus, EnvironmentPane::Matrix);
}

#[test]
fn help_explains_matrix_symbols_and_missing_rows() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    view.help();
    view.dialog_scroll = u16::MAX;

    let text = buffer_text(&render_to_buffer((120, 60), |frame| {
        view.render(frame, &state);
    }));
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");

    for marker in [
        "Same changes",
        "Comparison",
        "Scope",
        "Excluded",
        "Ready plans",
        "unknown",
        "values may differ",
        "+ / ~ / -",
        "+/- / -/+",
        "blank",
        "resource absent from this environment",
        ".",
        "resource present, with no change",
        "action unknown",
        "why: missing",
        "some Ready plans",
        "not retried",
    ] {
        assert!(compact.contains(marker), "{marker}: {text}");
    }
}

#[test]
fn ready_review_remains_available_and_quit_requires_confirmation_while_acquiring() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    let size = Size::new(80, 24);
    render_to_buffer((80, 24), |frame| view.render(frame, &state));
    view.handle_key(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
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
    assert!(view.confirming_quit);
    for character in ['h', 'l', 'g', 'G'] {
        view.handle_key(
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            size,
            &state,
        );
    }
    assert!(view.confirming_quit);
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
fn plan_scroll_resets_when_resize_makes_the_full_document_fit() {
    let state = overview_plan_session(&["a-ready", "b-ready"]);
    let mut view = EnvironmentView::default();
    let small = Size::new(80, 24);

    view.handle_key(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        small,
        &state,
    );
    for _ in 0..10 {
        view.handle_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            small,
            &state,
        );
    }

    let narrow = buffer_text(&render_to_buffer((80, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(narrow.contains("PLAN LINE 10"), "{narrow}");
    assert!(narrow.contains("Line 11/45"), "{narrow}");

    let medium = buffer_text(&render_to_buffer((120, 40), |frame| {
        view.render(frame, &state);
    }));
    assert!(medium.contains("PLAN LINE 10"), "{medium}");
    assert!(medium.contains("Line 11/45"), "{medium}");

    let wide = buffer_text(&render_to_buffer((160, 60), |frame| {
        view.render(frame, &state);
    }));
    assert!(wide.contains("PLAN LINE 00"), "{wide}");
    assert!(wide.contains("Line 1/45"), "{wide}");
    assert_eq!(view.reviews[0].scroll().0, 0);
}

#[test]
fn overview_round_trip_opens_the_full_plan_from_the_top() {
    let state = overview_plan_session(&["a-ready", "b-ready"]);

    for size in [(80, 24), (120, 40), (160, 60)] {
        let mut view = EnvironmentView::default();
        let terminal = Size::new(size.0, size.1);
        view.handle_key(
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
            terminal,
            &state,
        );
        let opened = buffer_text(&render_to_buffer(size, |frame| view.render(frame, &state)));
        assert!(opened.contains("PLAN LINE 00"), "{size:?}: {opened}");
        assert!(opened.contains("Line 1/45"), "{size:?}: {opened}");

        view.handle_key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
            terminal,
            &state,
        );
        let overview = buffer_text(&render_to_buffer(size, |frame| view.render(frame, &state)));
        assert!(
            overview.contains("Same change across envs"),
            "{size:?}: {overview}"
        );
        view.handle_key(
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
            terminal,
            &state,
        );

        let reopened = buffer_text(&render_to_buffer(size, |frame| view.render(frame, &state)));
        assert!(reopened.contains("PLAN LINE 00"), "{size:?}: {reopened}");
        assert!(reopened.contains("Line 1/45"), "{size:?}: {reopened}");
    }
}

#[test]
fn selecting_visible_environments_preserves_matrix_columns_across_layout_changes() {
    let state = overview_plan_session(&["dev", "stg", "prod"]);
    let mut view = EnvironmentView::default();
    let wide = Size::new(165, 50);

    for _ in 0..2 {
        view.handle_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            wide,
            &state,
        );
    }

    let all_columns = buffer_text(&render_to_buffer((165, 50), |frame| {
        view.render(frame, &state);
    }));
    for environment in ["dev", "stg", "prod"] {
        assert!(
            matrix_header(&all_columns).contains(environment),
            "{all_columns}"
        );
    }

    view.handle_key(
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        wide,
        &state,
    );
    view.handle_key(
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        wide,
        &state,
    );
    view.handle_key(
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
        wide,
        &state,
    );

    let maximized = buffer_text(&render_to_buffer((165, 50), |frame| {
        view.render(frame, &state);
    }));
    for environment in ["dev", "stg", "prod"] {
        assert!(
            matrix_header(&maximized).contains(environment),
            "{maximized}"
        );
    }

    let resized = buffer_text(&render_to_buffer((120, 40), |frame| {
        view.render(frame, &state);
    }));
    for environment in ["dev", "stg", "prod"] {
        assert!(matrix_header(&resized).contains(environment), "{resized}");
    }

    view.handle_key(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        wide,
        &state,
    );
    let raw = buffer_text(&render_to_buffer((165, 50), |frame| {
        view.render(frame, &state);
    }));
    assert!(raw.contains("PLAN LINE 00"), "{raw}");
    view.handle_key(
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        wide,
        &state,
    );
    let returned = buffer_text(&render_to_buffer((165, 50), |frame| {
        view.render(frame, &state);
    }));
    for environment in ["dev", "stg", "prod"] {
        assert!(matrix_header(&returned).contains(environment), "{returned}");
    }

    let narrowed = buffer_text(&render_to_buffer((55, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(matrix_header(&narrowed).contains("stg"), "{narrowed}");
    assert!(matrix_header(&narrowed).contains("prod"), "{narrowed}");
    assert!(!matrix_header(&narrowed).contains("dev"), "{narrowed}");
}

#[test]
fn narrow_selection_keeps_the_previous_column_and_manual_scroll_position() {
    let state = overview_plan_session(&["dev", "stg", "prod"]);
    let mut view = EnvironmentView::default();
    let narrow = Size::new(55, 24);

    for _ in 0..2 {
        view.handle_key(
            KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE),
            narrow,
            &state,
        );
    }

    let selected = buffer_text(&render_to_buffer((55, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(matrix_header(&selected).contains("stg"), "{selected}");
    assert!(matrix_header(&selected).contains("prod"), "{selected}");
    assert!(!matrix_header(&selected).contains("dev"), "{selected}");

    view.handle_key(
        KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        narrow,
        &state,
    );
    let manually_scrolled = buffer_text(&render_to_buffer((55, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(
        matrix_header(&manually_scrolled).contains("prod"),
        "{manually_scrolled}"
    );
    assert!(
        !matrix_header(&manually_scrolled).contains("stg"),
        "{manually_scrolled}"
    );

    view.handle_key(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        narrow,
        &state,
    );
    let raw = buffer_text(&render_to_buffer((55, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(raw.contains("PLAN LINE 00"), "{raw}");
    view.handle_key(
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        narrow,
        &state,
    );
    let returned = buffer_text(&render_to_buffer((55, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(matrix_header(&returned).contains("prod"), "{returned}");
    assert!(!matrix_header(&returned).contains("stg"), "{returned}");

    for _ in 0..2 {
        view.handle_key(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            narrow,
            &state,
        );
    }
    let manually_scrolled = buffer_text(&render_to_buffer((55, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(
        matrix_header(&manually_scrolled).contains("dev"),
        "{manually_scrolled}"
    );
    assert!(
        matrix_header(&manually_scrolled).contains("stg"),
        "{manually_scrolled}"
    );
    assert!(
        !matrix_header(&manually_scrolled).contains("prod"),
        "{manually_scrolled}"
    );

    let resized_after_manual_scroll = buffer_text(&render_to_buffer((50, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(
        matrix_header(&resized_after_manual_scroll).contains("dev"),
        "{resized_after_manual_scroll}"
    );
    assert!(
        !matrix_header(&resized_after_manual_scroll).contains("prod"),
        "{resized_after_manual_scroll}"
    );
}

#[test]
fn selecting_an_excluded_environment_keeps_the_matrix_start() {
    let state = overview_plan_session(&["dev", "stg", "prod"]);
    let mut view = EnvironmentView::default();
    let size = Size::new(165, 50);

    for _ in 0..2 {
        view.handle_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            size,
            &state,
        );
    }
    render_to_buffer((165, 50), |frame| view.render(frame, &state));
    view.handle_key(
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        size,
        &state,
    );
    view.handle_key(
        KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        size,
        &state,
    );
    render_to_buffer((165, 50), |frame| view.render(frame, &state));
    view.handle_key(
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        size,
        &state,
    );
    view.handle_key(
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        size,
        &state,
    );
    view.handle_key(
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        size,
        &state,
    );

    for (key, expected) in [(KeyCode::Up, 1), (KeyCode::Down, 2)] {
        view.handle_key(KeyEvent::new(key, KeyModifiers::NONE), size, &state);
        let rendered = buffer_text(&render_to_buffer((165, 50), |frame| {
            view.render(frame, &state);
        }));
        let header = matrix_header(&rendered);
        assert!(header.contains("stg"), "{rendered}");
        assert!(!header.contains("dev"), "{rendered}");
        assert!(!header.contains("prod"), "{rendered}");
        assert_eq!(view.selection.column, expected);
    }
}

#[test]
fn filtered_plan_position_tracks_the_visible_source_line_after_resize() {
    let mut state = overview_plan_session(&["a-ready", "b-ready"]);
    let mut view = EnvironmentView::default();
    let small = Size::new(80, 24);
    view.handle_key(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        small,
        &state,
    );
    view.handle_key(
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        small,
        &state,
    );
    for character in "terraform_data.api".chars() {
        if let Some(EnvironmentInput::Review(index, action)) = view.handle_key(
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            small,
            &state,
        ) {
            state.update_review(index, *action, std::time::Instant::now());
        }
    }
    view.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        small,
        &state,
    );

    let filtered = buffer_text(&render_to_buffer((80, 24), |frame| {
        view.render(frame, &state);
    }));
    let first_visible_line = usize::from(view.reviews[0].scroll().0) + 1;
    assert!(filtered.contains("PLAN LINE 20"), "{filtered}");
    assert!(
        filtered.contains(&format!("Line {first_visible_line}/45")),
        "{filtered}"
    );

    let medium = buffer_text(&render_to_buffer((120, 40), |frame| {
        view.render(frame, &state);
    }));
    let first_visible_line = usize::from(view.reviews[0].scroll().0) + 1;
    assert!(
        medium.contains(&format!("Line {first_visible_line}/45")),
        "{medium}"
    );

    let wide = buffer_text(&render_to_buffer((160, 60), |frame| {
        view.render(frame, &state);
    }));
    assert!(wide.contains("PLAN LINE 00"), "{wide}");
    assert!(wide.contains("Line 1/45"), "{wide}");
    assert_eq!(view.reviews[0].scroll().0, 0);
}

#[test]
fn raw_environment_help_explains_bracket_navigation_at_supported_widths() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    let size = Size::new(80, 24);

    view.handle_key(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
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

        assert!(compact.contains("[/]"), "{width}x{height}: {text}");
        assert!(compact.contains("next"), "{width}x{height}: {text}");
        assert!(compact.contains("previous"), "{width}x{height}: {text}");
        assert!(compact.contains("environment"), "{width}x{height}: {text}");
    }
}

#[test]
fn raw_environment_help_scrolls_by_line_and_page_without_moving_the_plan() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    let size = Size::new(80, 24);

    view.handle_key(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        size,
        &state,
    );
    view.handle_key(
        KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        size,
        &state,
    );
    let plan_scroll = view.reviews[0].scroll();

    for (key, expected) in [
        (KeyCode::Down, 1),
        (KeyCode::Char('j'), 2),
        (KeyCode::PageDown, 10),
        (KeyCode::Up, 9),
        (KeyCode::Char('k'), 8),
        (KeyCode::PageUp, 0),
    ] {
        view.handle_key(KeyEvent::new(key, KeyModifiers::NONE), size, &state);
        assert_eq!(view.reviews[0].overlay_scroll(), expected, "{key:?}");
    }

    assert_eq!(view.selection.raw, Some(0));
    assert_eq!(view.reviews[0].scroll(), plan_scroll);
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
fn environment_sidebar_filters_comparison_without_changing_the_selected_plan() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    let size = Size::new(160, 60);
    view.handle_key(
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.selection.column, 1);
    view.handle_key(
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.selection.column, 1);
    assert_eq!(view.selected_environments, Some(vec![0, 2, 3, 4]));
    view.sync(&state);
    let filtered = buffer_text(&render_to_buffer((160, 60), |frame| {
        view.render(frame, &state);
    }));
    let header = filtered
        .lines()
        .find(|line| line.contains("Address"))
        .unwrap();
    assert!(header.contains("a-ready"), "{header}");
    assert!(!header.contains("b-error"), "{header}");

    view.handle_key(
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.selected_environments, Some(vec![1]));
    view.sync(&state);
    let filtered = buffer_text(&render_to_buffer((160, 60), |frame| {
        view.render(frame, &state);
    }));
    let header = filtered
        .lines()
        .find(|line| line.contains("Address"))
        .unwrap();
    assert!(header.contains("b-error"), "{header}");
    view.handle_key(
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.selected_environments, Some(vec![1]));
    assert!(
        view.notice
            .as_deref()
            .is_some_and(|notice| notice.to_lowercase().contains("at least one"))
    );

    view.handle_key(
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.selected_environments, None);

    let text = buffer_text(&render_to_buffer((80, 24), |frame| {
        view.render(frame, &state);
    }));
    assert!(text.contains("b-error"), "{text}");
    assert!(text.contains("Error"), "{text}");
    assert!(
        text.contains("Plan failed: Missing required variable"),
        "{text}"
    );
}

#[test]
fn sidebar_width_thresholds_restore_the_manual_setting() {
    let state = partial_session();
    let mut view = EnvironmentView::default();

    let wide = buffer_text(&render_to_buffer((120, 40), |frame| {
        view.render(frame, &state);
    }));
    assert_eq!(view.sidebar, SidebarSetting::Open);
    assert_eq!(view.focus, EnvironmentPane::Environments);
    assert!(wide.contains("[1] Envs"));

    let mut narrow = EnvironmentView::default();
    let text = buffer_text(&render_to_buffer((119, 40), |frame| {
        narrow.render(frame, &state);
    }));
    assert_eq!(narrow.sidebar, SidebarSetting::Closed);
    assert_eq!(narrow.focus, EnvironmentPane::Matrix);
    assert!(!text.contains("[1] Envs"));
    let summary = text.lines().nth(1).unwrap();
    assert!(summary.contains("a-ready"), "{summary}");
    assert!(!summary.contains("b-error"), "{summary}");

    narrow.handle_key(
        KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        Size::new(90, 40),
        &state,
    );
    assert_eq!(narrow.sidebar, SidebarSetting::Open);
    let hidden = buffer_text(&render_to_buffer((89, 40), |frame| {
        narrow.render(frame, &state);
    }));
    assert!(!hidden.contains("[1] Envs"));
    let restored = buffer_text(&render_to_buffer((90, 40), |frame| {
        narrow.render(frame, &state);
    }));
    assert!(restored.contains("[1] Envs"));
}

#[test]
fn short_terminal_keeps_the_matrix_frame_and_shows_resize_guidance() {
    let state = partial_session();
    let mut view = EnvironmentView::default();
    let buffer = render_to_buffer((80, 5), |frame| view.render(frame, &state));
    let text = buffer_text(&buffer);

    assert!(
        text.contains("Resize terminal to view pane content"),
        "{text}"
    );
    assert_eq!(buffer.cell((0, 1)).unwrap().symbol(), "┌");
    assert_eq!(buffer.cell((79, 3)).unwrap().symbol(), "┘");
}

#[test]
fn sidebar_focus_and_maximize_shortcuts_preserve_each_other() {
    let state = partial_session();
    let size = ratatui::layout::Size::new(120, 40);
    let mut view = EnvironmentView::default();
    let press = |view: &mut EnvironmentView, code| {
        view.handle_key(KeyEvent::new(code, KeyModifiers::NONE), size, &state);
    };

    let _ = render_to_buffer((120, 40), |frame| view.render(frame, &state));
    press(&mut view, KeyCode::Char('f'));
    assert_eq!(view.maximized, Some(EnvironmentPane::Environments));
    let maximized = buffer_text(&render_to_buffer((120, 40), |frame| {
        view.render(frame, &state);
    }));
    assert!(!maximized.contains("Resize terminal to view pane content"));

    let hidden = buffer_text(&render_to_buffer((89, 40), |frame| {
        view.render(frame, &state);
    }));
    assert!(!hidden.contains("[1] Envs"), "{hidden}");
    assert_eq!(view.maximized, Some(EnvironmentPane::Environments));
    let restored = buffer_text(&render_to_buffer((90, 40), |frame| {
        view.render(frame, &state);
    }));
    assert!(restored.contains("[1] Envs"), "{restored}");

    press(&mut view, KeyCode::Char('b'));
    assert_eq!(view.sidebar, SidebarSetting::Open);
    assert_eq!(view.maximized, Some(EnvironmentPane::Environments));
    press(&mut view, KeyCode::Char('f'));
    assert_eq!(view.maximized, None);

    press(&mut view, KeyCode::Char('2'));
    press(&mut view, KeyCode::Char('f'));
    assert_eq!(view.maximized, Some(EnvironmentPane::Matrix));
    press(&mut view, KeyCode::Esc);
    assert_eq!(view.maximized, None);
    press(&mut view, KeyCode::Char('f'));
    assert_eq!(view.maximized, Some(EnvironmentPane::Matrix));
    press(&mut view, KeyCode::Char('1'));
    assert_eq!(view.maximized, None);
    assert_eq!(view.focus, EnvironmentPane::Environments);
    press(&mut view, KeyCode::Char('b'));
    assert_eq!(view.sidebar, SidebarSetting::Closed);
    assert_eq!(view.focus, EnvironmentPane::Matrix);
}

#[test]
fn single_environment_hides_sidebar_and_its_shortcuts() {
    let state = EnvironmentSession::new(
        vec![Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from("/synthetic/only-env"),
                workspace: "default".to_owned(),
            }),
        }],
        false,
    );
    let size = Size::new(120, 40);
    let mut view = EnvironmentView::default();
    let text = buffer_text(&render_to_buffer((120, 40), |frame| {
        view.render(frame, &state);
    }));
    assert_eq!(view.sidebar, SidebarSetting::Closed);
    assert!(!text.contains("[1] Envs"), "{text}");
    assert!(text.contains("[3] Relations"), "{text}");
    assert!(text.contains("only-env Pending"), "{text}");
    assert!(!text.contains("toggle envs"), "{text}");
    assert!(!text.contains("1/2 focus"), "{text}");
    assert!(!text.contains("[/] env"), "{text}");
    assert!(text.contains("2/3 focus"), "{text}");

    for key in [KeyCode::Char('1'), KeyCode::Char('b')] {
        view.handle_key(KeyEvent::new(key, KeyModifiers::NONE), size, &state);
        assert_eq!(view.sidebar, SidebarSetting::Closed);
        assert_eq!(view.focus, EnvironmentPane::Matrix);
    }
    view.handle_key(
        KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
        size,
        &state,
    );
    assert_eq!(view.focus, EnvironmentPane::Relations);
    assert_eq!(view.active_pane(size.width), EnvironmentPane::Relations);

    view.help();
    let help = buffer_text(&render_to_buffer((120, 40), |frame| {
        view.render(frame, &state);
    }));
    assert!(!help.contains("1 opens Envs"), "{help}");
    assert!(!help.contains("toggle the Envs sidebar"), "{help}");
    assert!(help.contains("Current: Overview"), "{help}");
    assert!(!help.contains("Multi-environment Overview"), "{help}");
    assert!(help.contains("2 / 3"), "{help}");
    assert!(help.contains("scroll [3]"), "{help}");
    assert!(help.contains("scroll columns in [2] or [3]"), "{help}");
    assert!(
        help.contains("[3] opens the plan from the top; [2] opens the selected source"),
        "{help}"
    );
    assert!(!help.contains("[1] or [3] opens"), "{help}");
}

#[test]
fn message_dialog_scrolls_through_long_error_details() {
    let mut state = EnvironmentSession::new(
        vec![Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from("/synthetic/error"),
                workspace: "default".to_owned(),
            }),
        }],
        false,
    );
    let index = state.start_next().unwrap();
    let detail = (0..30)
        .map(|line| format!("Diagnostic line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    state.complete(index, PlanResult::Error(detail), Vec::new());

    let mut view = EnvironmentView::default();
    let _ = view.open(&state, index);
    let size = Size::new(40, 16);
    let top = buffer_text(&render_to_buffer((40, 16), |frame| {
        view.render(frame, &state);
    }));
    assert!(top.contains("Diagnostic line 00"), "{top}");
    assert!(top.contains("Diagnostic line 12"), "{top}");

    view.handle_key(
        KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        size,
        &state,
    );
    let scrolled = buffer_text(&render_to_buffer((40, 16), |frame| {
        view.render(frame, &state);
    }));
    assert!(!scrolled.contains("Diagnostic line 00"), "{scrolled}");
    assert!(scrolled.contains("Diagnostic line 04"), "{scrolled}");
}

#[test]
fn environment_layout_reserves_the_sidebar_and_four_six_right_panes() {
    let layout = environments::overview_layout(
        ratatui::layout::Rect::new(0, 0, 160, 53),
        41,
        true,
        None,
        false,
        true,
    );
    assert_eq!(layout.environments.width, 41);
    assert_eq!(layout.matrix.height, 20);
    assert_eq!(layout.relations.height, 30);

    assert_eq!(environments::sidebar_width(partial_session().plans()), 24);

    let ordinary_name = "x".repeat(20);
    let ordinary = EnvironmentSession::new(
        vec![Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from(format!("/synthetic/{ordinary_name}")),
                workspace: "default".to_owned(),
            }),
        }],
        false,
    );
    assert_eq!(environments::sidebar_width(ordinary.plans()), 24);

    let production_name = format!("prod-{}", "x".repeat(15));
    let production = EnvironmentSession::new(
        vec![Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from(format!("/synthetic/{production_name}")),
                workspace: "default".to_owned(),
            }),
        }],
        false,
    );
    assert_eq!(environments::sidebar_width(production.plans()), 30);

    let mixed = EnvironmentSession::new(
        vec![
            Environment {
                tool: Tool::Terraform,
                availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                    directory: PathBuf::from(format!("/synthetic/{ordinary_name}")),
                    workspace: "default".to_owned(),
                }),
            },
            Environment {
                tool: Tool::Terraform,
                availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                    directory: PathBuf::from("/synthetic/prod"),
                    workspace: "default".to_owned(),
                }),
            },
        ],
        false,
    );
    assert_eq!(environments::sidebar_width(mixed.plans()), 24);

    let name = "x".repeat(60);
    let state = EnvironmentSession::new(
        vec![Environment {
            tool: Tool::Terraform,
            availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                directory: PathBuf::from(format!("/synthetic/{name}")),
                workspace: "default".to_owned(),
            }),
        }],
        false,
    );
    assert_eq!(environments::sidebar_width(state.plans()), 41);
}

#[test]
fn pending_production_environment_shows_its_badge_before_the_plan_finishes() {
    let state = EnvironmentSession::new(
        vec![
            Environment {
                tool: Tool::Terraform,
                availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                    directory: PathBuf::from("/synthetic/prod"),
                    workspace: "default".to_owned(),
                }),
            },
            Environment {
                tool: Tool::Terraform,
                availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                    directory: PathBuf::from("/synthetic/dev"),
                    workspace: "default".to_owned(),
                }),
            },
        ],
        false,
    );
    let mut view = EnvironmentView::default();

    let text = buffer_text(&render_to_buffer((120, 40), |frame| {
        view.render(frame, &state);
    }));

    assert!(text.contains("prod [PROD]"), "{text}");
    assert!(text.contains("Pending"), "{text}");
}

#[test]
fn sidebar_focus_and_selected_name_use_ansi_colors_and_terminal_defaults() {
    let state = overview_plan_session(&["a-ready", "b-ready"]);
    let mut view = EnvironmentView::default();
    let buffer = render_to_buffer((120, 40), |frame| view.render(frame, &state));
    assert_eq!(view.sidebar_width, 24);
    let focused_border = buffer.cell((0, 1)).unwrap();
    assert_eq!(focused_border.fg, Color::Cyan);
    assert_eq!(focused_border.bg, Color::Reset);

    let matrix_border = buffer
        .cell((view.sidebar_width, 1))
        .expect("unfocused matrix border");
    assert_eq!(matrix_border.fg, Color::DarkGray);
    assert_eq!(matrix_border.bg, Color::Reset);

    let selected_name = buffer.cell((7, 2)).expect("selected environment name");
    assert!(selected_name.modifier.contains(Modifier::UNDERLINED));
    assert_eq!(selected_name.bg, Color::Reset);

    for (style, foreground) in [
        (theme::overview_total_add_style(), Color::Green),
        (theme::overview_total_update_style(), Color::Yellow),
        (theme::overview_total_destroy_style(), Color::Red),
        (theme::overview_total_replace_style(), Color::Magenta),
    ] {
        assert_eq!(style.fg, Some(foreground));
        assert_eq!(style.bg, Some(Color::Reset));
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
