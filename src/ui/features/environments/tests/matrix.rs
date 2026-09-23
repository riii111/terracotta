use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

use ratatui::style::Modifier;
use rstest::rstest;

use super::*;
use crate::app::{
    execution::{ExecutionContext, SensitiveValue},
    plan::{
        Plan, PlanAction, PlanSummary, PlanValue, ResourceChange, ResourceChangeKind, ResourceMode,
    },
    review::{PlanBlock, PlanBlockKind, PlanDocument},
};

fn session(names: &[&str]) -> EnvironmentSession {
    EnvironmentSession::new(
        names
            .iter()
            .map(|name| Environment {
                tool: Tool::Terraform,
                availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                    directory: PathBuf::from(format!("/synthetic/{name}")),
                    workspace: "default".to_owned(),
                }),
            })
            .collect(),
        false,
    )
}

fn change(address: &str, kind: ResourceChangeKind) -> ResourceChange {
    let actions = match kind {
        ResourceChangeKind::Create => vec![PlanAction::Create],
        ResourceChangeKind::Delete => vec![PlanAction::Delete],
        ResourceChangeKind::Replace => vec![PlanAction::Create, PlanAction::Delete],
        ResourceChangeKind::NoOp => vec![PlanAction::NoOp],
        _ => vec![PlanAction::Update],
    };
    ResourceChange {
        address: address.to_owned(),
        provider: None,
        resource_type: Some("terraform_data".to_owned()),
        resource_name: Some("server".to_owned()),
        mode: ResourceMode::Managed,
        actions,
        kind,
        before: Some(PlanValue::Object(BTreeMap::from([(
            "input".to_owned(),
            PlanValue::String("old".to_owned()),
        )]))),
        after: Some(PlanValue::Object(BTreeMap::from([(
            "input".to_owned(),
            PlanValue::String("new".to_owned()),
        )]))),
        before_sensitive: None,
        after_sensitive: None,
        after_unknown: None,
        replace_paths: None,
        action_reason: None,
        previous_address: None,
        importing: None,
    }
}

fn complete(state: &mut EnvironmentSession, changes: Vec<ResourceChange>) {
    let mut lines = vec![
        "Terraform will perform the following actions:".to_owned(),
        String::new(),
    ];
    let mut blocks = vec![PlanBlock::new(0..2, PlanBlockKind::Common)];
    for change in changes
        .iter()
        .filter(|change| change.kind != ResourceChangeKind::NoOp)
    {
        let start = lines.len();
        lines.extend([
            format!("# {} will change", change.address),
            "~ input = old -> new".to_owned(),
            String::new(),
        ]);
        blocks.push(PlanBlock::with_addresses(
            start..lines.len(),
            PlanBlockKind::Resource,
            vec![change.address.clone()],
        ));
    }
    complete_with_plan_document(state, changes, lines.join("\n"), blocks, Vec::new());
}

fn complete_with_plan_document(
    state: &mut EnvironmentSession,
    changes: Vec<ResourceChange>,
    text: String,
    blocks: Vec<PlanBlock>,
    sensitive_values: Vec<SensitiveValue>,
) {
    let index = state.start_next().expect("pending environment");
    let directory = state.plans()[index].directory().to_owned();
    let count = |kind| changes.iter().filter(|change| change.kind == kind).count();
    let summary = PlanSummary {
        creates: count(ResourceChangeKind::Create),
        updates: count(ResourceChangeKind::Update),
        replaces: count(ResourceChangeKind::Replace),
        deletes: count(ResourceChangeKind::Delete),
    };
    let metadata = PlanMetadata::new(
        changes
            .iter()
            .map(|change| change.address.clone())
            .collect(),
        Vec::new(),
        summary.creates,
        summary.updates,
        summary.deletes,
        !changes.is_empty(),
    )
    .with_resource_changes(Vec::new(), summary.replaces)
    .with_sensitive_values(sensitive_values);
    let review = PlanReview::new(
        directory.clone(),
        "default".to_owned(),
        PlanDocument::with_blocks_and_line_kinds(text, blocks, Vec::new()),
        metadata,
        Vec::new(),
    )
    .with_context(
        ExecutionContext::loading(directory.display().to_string()).with_workspace("default"),
    )
    .with_apply_allowed(false)
    .with_apply_entry(false)
    .with_plan(Plan {
        resource_changes: changes,
        value_addresses: BTreeSet::new(),
        summary,
        unsupported_changes: Vec::new(),
        output_changes: Vec::new(),
    });
    state.complete(
        index,
        PlanResult::Ready {
            review: Box::new(review),
            changed: true,
        },
        Vec::new(),
    );
}

fn press(view: &mut EnvironmentView, state: &mut EnvironmentSession, code: KeyCode) {
    press_event(view, state, KeyEvent::new(code, KeyModifiers::NONE));
}

fn press_event(view: &mut EnvironmentView, state: &mut EnvironmentSession, key: KeyEvent) {
    if let Some(input) = view.handle_key(key, Size::new(80, 24), state) {
        match input {
            EnvironmentInput::Review(index, action) => {
                state.update_review(index, *action, Instant::now());
            }
            EnvironmentInput::Retry(index) => {
                assert!(state.retry(index));
            }
            _ => panic!("unexpected exit"),
        }
    }
}

#[test]
fn tab_keys_open_the_same_target_in_adjacent_environments_from_raw_plan() {
    let mut state = session(&["dev", "prod", "stg"]);
    for _ in 0..3 {
        complete(
            &mut state,
            vec![change("terraform_data.api", ResourceChangeKind::Update)],
        );
    }
    let mut view = EnvironmentView::default();

    press(&mut view, &mut state, KeyCode::Right);
    assert_eq!(view.selection.column, 1);
    press(&mut view, &mut state, KeyCode::Tab);
    assert_eq!(view.selection.column, 1);

    press(&mut view, &mut state, KeyCode::Char('v'));
    assert_eq!(view.selection.raw, Some(1));
    let selected = view.matrix.cell(1).unwrap().members.clone();

    press(&mut view, &mut state, KeyCode::Tab);
    assert_eq!(view.selection.raw, Some(2));
    assert_eq!(view.matrix.cell(2).unwrap().members, selected);

    press_event(
        &mut view,
        &mut state,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
    );
    assert_eq!(view.selection.raw, Some(1));
    assert_eq!(view.matrix.cell(1).unwrap().members, selected);
}

fn text(view: &mut EnvironmentView, state: &EnvironmentSession, size: (u16, u16)) -> String {
    buffer_text(&render_to_buffer(size, |frame| view.render(frame, state)))
}

#[rstest]
#[case::small(80, 24)]
#[case::medium(120, 40)]
#[case::large(160, 60)]
#[case::narrow(40, 16)]
fn three_environments_show_groups_actions_and_totals(#[case] width: u16, #[case] height: u16) {
    let mut state = session(&["dev", "prod", "stg"]);
    for (count, action) in [
        (20, ResourceChangeKind::Update),
        (200, ResourceChangeKind::Replace),
        (20, ResourceChangeKind::Delete),
    ] {
        let mut changes: Vec<_> = (0..count)
            .map(|index| {
                change(
                    &format!("terraform_data.server[{index}]"),
                    ResourceChangeKind::Update,
                )
            })
            .collect();
        changes.push(change("terraform_data.api", action));
        complete(&mut state, changes);
    }
    let mut view = EnvironmentView::default();
    let buffer = render_to_buffer((width, height), |frame| view.render(frame, &state));

    let reversed_cells = buffer
        .content
        .iter()
        .filter(|cell| cell.modifier.contains(Modifier::REVERSED))
        .count();
    assert_eq!(reversed_cells, 1);
    let rendered = buffer_text(&buffer);
    assert!(rendered.contains("> dev"));
    assert!(rendered.contains("> terraform_data.api"));
    insta::assert_snapshot!(format!("three_environments_{width}x{height}"), rendered);
}

#[test]
fn short_terminal_keeps_environment_actions_without_boundary_rows() {
    let state = session(&["dev", "prod", "stg"]);
    let mut view = EnvironmentView::default();
    let rendered = text(&mut view, &state, (40, 14));

    assert!(rendered.contains("Enter preview  v full plan"));
    assert!(rendered.contains("↑↓ row  ←→ env"));
    assert!(!rendered.contains(&"─".repeat(40)));
}

#[test]
fn narrow_matrix_keeps_why_visible_with_a_long_selected_environment_name() {
    let state = session(&["production-eu-west-1"]);
    let mut view = EnvironmentView::default();
    let rendered = text(&mut view, &state, (40, 16));
    let header = rendered
        .lines()
        .find(|line| line.starts_with("Address"))
        .expect("matrix header");

    assert!(header.contains('>'));
    assert!(header.contains("why"));
}

#[test]
fn columns_remain_selectable_without_rows_and_scroll_beyond_nine() {
    let names: Vec<_> = (0..12).map(|index| format!("env-{index:02}")).collect();
    let mut state = session(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let mut view = EnvironmentView::default();
    for _ in 0..11 {
        press(&mut view, &mut state, KeyCode::Right);
    }
    assert_eq!(view.selection.column, 11);
    insta::assert_snapshot!(
        "twelve_pending_selected_last",
        text(&mut view, &state, (80, 24))
    );
    for _ in 0..12 {
        complete(
            &mut state,
            vec![change("terraform_data.api", ResourceChangeKind::Update)],
        );
    }
    insta::assert_snapshot!(
        "twelve_ready_selected_last",
        text(&mut view, &state, (80, 24))
    );
    press(&mut view, &mut state, KeyCode::Char('v'));
    assert_eq!(view.selection.raw, Some(11));
    press(&mut view, &mut state, KeyCode::Char('['));
    assert_eq!(view.selection.raw, Some(10));
    press(&mut view, &mut state, KeyCode::Esc);
    assert_eq!(view.selection.column, 11);
}

#[test]
fn no_op_missing_and_unavailable_open_distinct_guidance() {
    let mut state = session(&["a", "b", "c"]);
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::Update)],
    );
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::NoOp)],
    );
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Char('2'));
    assert_eq!(view.selection.raw, Some(1));
    assert!(text(&mut view, &state, (80, 24)).contains("no changes"));
    press(&mut view, &mut state, KeyCode::Esc);
    press(&mut view, &mut state, KeyCode::Char('3'));
    assert!(text(&mut view, &state, (80, 24)).contains("Pending"));
    assert!(view.selection.raw.is_none());
    press(&mut view, &mut state, KeyCode::Esc);
    complete(&mut state, Vec::new());
    press(&mut view, &mut state, KeyCode::Enter);
    assert!(
        text(&mut view, &state, (80, 24)).contains("absent from the selected environment's plan")
    );
    assert!(view.selection.raw.is_none());
    assert!(view.preview_open);
}

#[test]
fn selected_cell_preview_follows_selection_and_survives_full_plan_round_trip() {
    let mut state = session(&["dev", "prod"]);
    for _ in 0..2 {
        complete(
            &mut state,
            vec![
                change("terraform_data.api", ResourceChangeKind::Update),
                change("terraform_data.worker", ResourceChangeKind::Update),
            ],
        );
    }
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Enter);
    assert!(view.preview_open);
    assert!(view.selection.raw.is_none());

    let api = text(&mut view, &state, (120, 40));
    assert!(api.contains("dev · terraform_data.api"), "{api}");
    assert!(api.contains("# terraform_data.api will change"), "{api}");
    press(&mut view, &mut state, KeyCode::Enter);
    assert!(view.selection.raw.is_none());

    press(&mut view, &mut state, KeyCode::Down);
    press(&mut view, &mut state, KeyCode::Right);
    let worker = text(&mut view, &state, (120, 40));
    assert!(worker.contains("prod · terraform_data.worker"), "{worker}");
    assert!(
        worker.contains("# terraform_data.worker will change"),
        "{worker}"
    );

    press(&mut view, &mut state, KeyCode::Char('v'));
    assert_eq!(view.selection.raw, Some(1));
    let full_plan = text(&mut view, &state, (120, 40));
    assert!(full_plan.contains("Terraform will perform the following actions:"));
    press(&mut view, &mut state, KeyCode::Esc);
    assert!(view.selection.raw.is_none());
    assert!(view.preview_open);
    let returned = text(&mut view, &state, (120, 40));
    assert!(
        returned.contains("prod · terraform_data.worker"),
        "{returned}"
    );
    assert!(
        returned.contains("# terraform_data.worker will change"),
        "{returned}"
    );

    press(&mut view, &mut state, KeyCode::Esc);
    assert!(!view.preview_open);
    assert!(view.selection.raw.is_none());
}

#[test]
fn selected_cell_preview_preserves_search_cancel_priority() {
    let mut state = session(&["dev"]);
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::Update)],
    );
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Enter);
    press(&mut view, &mut state, KeyCode::Char('/'));
    press(&mut view, &mut state, KeyCode::Char('a'));
    press(&mut view, &mut state, KeyCode::Esc);

    assert!(view.preview_open);
    assert!(!view.matrix.searching());
    assert_eq!(view.matrix.filter(), "");
    press(&mut view, &mut state, KeyCode::Esc);
    assert!(!view.preview_open);
}

#[test]
fn selected_cell_preview_explains_absent_unchanged_and_unavailable_cells() {
    let mut state = session(&["dev", "missing", "unchanged", "pending"]);
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::Update)],
    );
    complete(&mut state, Vec::new());
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::NoOp)],
    );
    let mut view = EnvironmentView::default();

    press(&mut view, &mut state, KeyCode::Right);
    press(&mut view, &mut state, KeyCode::Enter);
    assert!(
        text(&mut view, &state, (80, 24)).contains("absent from the selected environment's plan")
    );
    press(&mut view, &mut state, KeyCode::Esc);

    press(&mut view, &mut state, KeyCode::Right);
    press(&mut view, &mut state, KeyCode::Enter);
    assert!(text(&mut view, &state, (80, 24)).contains("has no changes"));
    press(&mut view, &mut state, KeyCode::Esc);

    press(&mut view, &mut state, KeyCode::Right);
    press(&mut view, &mut state, KeyCode::Enter);
    assert!(text(&mut view, &state, (80, 24)).contains("has not been acquired yet"));
}

#[test]
fn selected_cell_preview_reports_missing_raw_block_without_using_another_cell() {
    let mut state = session(&["dev", "prod"]);
    let address = "terraform_data.api";
    let missing_address = "terraform_data.worker";
    let changes = vec![
        change(address, ResourceChangeKind::Update),
        change(missing_address, ResourceChangeKind::Update),
    ];
    for _ in 0..2 {
        complete_with_plan_document(
            &mut state,
            changes.clone(),
            format!("# {address} will change\n~ input = old -> new"),
            vec![PlanBlock::with_addresses(
                0..2,
                PlanBlockKind::Resource,
                vec![address.to_owned()],
            )],
            Vec::new(),
        );
    }
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Down);
    assert_eq!(view.matrix.selected_address(), Some(missing_address));
    press(&mut view, &mut state, KeyCode::Enter);

    let rendered = text(&mut view, &state, (80, 24));
    assert!(rendered.contains("no matching raw block"), "{rendered}");
    assert!(!rendered.contains("# terraform_data.api will change"));
}

#[test]
fn grouped_cell_preview_requires_expansion_before_showing_a_resource_block() {
    let mut state = session(&["dev", "prod"]);
    let changes: Vec<_> = (0..2)
        .map(|index| {
            change(
                &format!("terraform_data.server[{index}]"),
                ResourceChangeKind::Update,
            )
        })
        .collect();
    complete(&mut state, changes.clone());
    complete(&mut state, changes);
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Enter);

    assert!(view.matrix.selected_is_group());
    let rendered = text(&mut view, &state, (80, 24));
    assert!(rendered.contains("Press Space to expand"), "{rendered}");
    assert!(!rendered.contains("# terraform_data.server[0] will change"));
}

#[test]
fn preview_uses_and_redacts_long_original_blocks_across_supported_sizes() {
    let mut state = session(&["dev", "prod"]);
    let address = "terraform_data.api";
    let mut lines = vec![
        format!("# {address} will change"),
        "~ password = synthetic-secret -> rotated".to_owned(),
    ];
    lines.extend((1..=24).map(|index| format!("  # synthetic block line {index}")));
    let plan_text = lines.join("\n");
    let block = PlanBlock::with_addresses(
        0..lines.len(),
        PlanBlockKind::Resource,
        vec![address.to_owned()],
    );
    for _ in 0..2 {
        complete_with_plan_document(
            &mut state,
            vec![change(address, ResourceChangeKind::Update)],
            plan_text.clone(),
            vec![block.clone()],
            vec![SensitiveValue::Text("synthetic-secret".to_owned())],
        );
    }
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Enter);

    for size in [(40, 16), (80, 24), (120, 40), (160, 60)] {
        let rendered = text(&mut view, &state, size);
        assert!(
            rendered.contains("# terraform_data.api will change"),
            "{size:?}: {rendered}"
        );
        assert!(
            !rendered.contains("synthetic-secret"),
            "{size:?}: {rendered}"
        );
        if size == (160, 60) {
            assert!(rendered.contains("synthetic block line 24"), "{rendered}");
        }
    }
}

#[test]
fn preview_falls_back_when_it_would_shrink_the_matrix_below_minimum() {
    let mut state = session(&["dev", "prod"]);
    for _ in 0..2 {
        complete(
            &mut state,
            vec![change("terraform_data.api", ResourceChangeKind::Update)],
        );
    }
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Enter);
    let selected_address = view.matrix.selected_address().unwrap().to_owned();

    let small = text(&mut view, &state, (40, 12));
    assert!(
        small.contains("Preview unavailable; resize terminal"),
        "{small}"
    );
    assert!(small.contains("v full plan"), "{small}");
    assert!(small.contains("Address"), "{small}");
    assert_eq!(
        view.matrix.selected_address(),
        Some(selected_address.as_str())
    );

    let large = text(&mut view, &state, (80, 24));
    assert!(
        large.contains("# terraform_data.api will change"),
        "{large}"
    );
    assert!(view.preview_open);
    assert_eq!(
        view.matrix.selected_address(),
        Some(selected_address.as_str())
    );
}

#[test]
fn filter_uses_complete_member_addresses_and_raw_return_preserves_expansion() {
    let mut state = session(&["a", "b", "c"]);
    for _ in 0..3 {
        complete(
            &mut state,
            (0..200)
                .map(|index| {
                    change(
                        &format!("module.long_name.terraform_data.server[{index}]"),
                        ResourceChangeKind::Update,
                    )
                })
                .collect(),
        );
    }
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Char('/'));
    for character in "server[198]".chars() {
        press(&mut view, &mut state, KeyCode::Char(character));
    }
    press(&mut view, &mut state, KeyCode::Enter);
    press(&mut view, &mut state, KeyCode::Char(' '));
    press(&mut view, &mut state, KeyCode::Down);
    press(&mut view, &mut state, KeyCode::Right);
    let before = text(&mut view, &state, (80, 24));
    press(&mut view, &mut state, KeyCode::Char('3'));
    assert_eq!(view.selection.raw, Some(2));
    assert!(view.reviews[2].scroll().0 > 500);
    press(&mut view, &mut state, KeyCode::Esc);
    assert_eq!(view.selection.column, 1);
    assert_eq!(text(&mut view, &state, (80, 24)), before);
    assert_eq!(
        view.matrix.cell(1).unwrap().members,
        ["module.long_name.terraform_data.server[198]"]
    );
}

#[test]
fn matrix_search_edits_graphemes_and_restores_the_selected_anchor_on_cancel() {
    let mut state = session(&["a"]);
    complete(
        &mut state,
        vec![
            change("terraform_data.alpha", ResourceChangeKind::Update),
            change("terraform_data.beta", ResourceChangeKind::Update),
        ],
    );
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Down);
    assert_eq!(
        view.matrix.cell(0).unwrap().members,
        ["terraform_data.beta"]
    );

    press(&mut view, &mut state, KeyCode::Char('/'));
    for character in "aあe\u{301}👩💻".chars() {
        press(&mut view, &mut state, KeyCode::Char(character));
    }
    press(&mut view, &mut state, KeyCode::Home);
    press(&mut view, &mut state, KeyCode::Right);
    press(&mut view, &mut state, KeyCode::Right);
    press(&mut view, &mut state, KeyCode::Backspace);
    assert_eq!(view.matrix.filter(), "ae\u{301}👩💻");

    press(&mut view, &mut state, KeyCode::End);
    press(&mut view, &mut state, KeyCode::Left);
    press(&mut view, &mut state, KeyCode::Char('\u{200d}'));
    press(&mut view, &mut state, KeyCode::Char('x'));
    assert_eq!(view.matrix.filter(), "ae\u{301}👩\u{200d}💻x");

    press(&mut view, &mut state, KeyCode::Backspace);
    press(&mut view, &mut state, KeyCode::Backspace);
    press(&mut view, &mut state, KeyCode::Backspace);
    assert_eq!(view.matrix.filter(), "a");
    press(&mut view, &mut state, KeyCode::Home);
    press(&mut view, &mut state, KeyCode::Char('X'));
    press(&mut view, &mut state, KeyCode::End);
    press(&mut view, &mut state, KeyCode::Char('Y'));
    press(&mut view, &mut state, KeyCode::Esc);
    assert_eq!(view.matrix.filter(), "");
    assert_eq!(
        view.matrix.cell(0).unwrap().members,
        ["terraform_data.beta"]
    );

    press(&mut view, &mut state, KeyCode::Char('/'));
    for character in "terraform_data.beta".chars() {
        press(&mut view, &mut state, KeyCode::Char(character));
    }
    press(&mut view, &mut state, KeyCode::Enter);
    assert_eq!(view.matrix.filter(), "terraform_data.beta");
    assert_eq!(
        view.matrix.cell(0).unwrap().members,
        ["terraform_data.beta"]
    );

    press(&mut view, &mut state, KeyCode::Char('/'));
    press(&mut view, &mut state, KeyCode::Char('x'));
    press(&mut view, &mut state, KeyCode::Esc);
    assert_eq!(view.matrix.filter(), "terraform_data.beta");
    assert_eq!(
        view.matrix.cell(0).unwrap().members,
        ["terraform_data.beta"]
    );
}

#[test]
fn retry_and_new_ready_environment_keep_member_when_group_disappears() {
    let mut state = session(&["a", "b", "c"]);
    for _ in 0..2 {
        complete(
            &mut state,
            (0..2)
                .map(|index| {
                    change(
                        &format!("terraform_data.server[{index}]"),
                        ResourceChangeKind::Update,
                    )
                })
                .collect(),
        );
    }
    let index = state.start_next().unwrap();
    state.complete(
        index,
        PlanResult::Error("synthetic error".to_owned()),
        Vec::new(),
    );
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Right);
    press(&mut view, &mut state, KeyCode::Right);
    press(&mut view, &mut state, KeyCode::Char('r'));
    assert_eq!(view.selection.column, 2);
    assert_eq!(
        view.matrix.cell(0).unwrap().members[0],
        "terraform_data.server[0]"
    );
    complete(
        &mut state,
        vec![
            change("terraform_data.server[0]", ResourceChangeKind::Replace),
            change("terraform_data.server[1]", ResourceChangeKind::NoOp),
        ],
    );
    let output = text(&mut view, &state, (80, 24));
    assert_eq!(view.selection.column, 2);
    assert_eq!(
        view.matrix.cell(0).unwrap().members,
        ["terraform_data.server[0]"]
    );
    assert!(output.contains("Ready: 3/3"));
    assert!(!output.contains("Compared:"));
}

#[test]
fn raw_filter_escape_clears_the_query_before_returning_to_overview() {
    let mut state = session(&["a", "b"]);
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::Update)],
    );
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Char('v'));
    press(&mut view, &mut state, KeyCode::Char('/'));
    for character in "api".chars() {
        press(&mut view, &mut state, KeyCode::Char(character));
    }
    press(&mut view, &mut state, KeyCode::Enter);
    press(&mut view, &mut state, KeyCode::Esc);

    assert_eq!(view.selection.raw, Some(0));
    assert!(
        state.plans()[0]
            .review()
            .unwrap()
            .review()
            .search_query()
            .is_empty()
    );
    press(&mut view, &mut state, KeyCode::Esc);
    assert!(view.selection.raw.is_none());
}

#[rstest]
#[case::small(80, 24)]
#[case::narrow(40, 16)]
fn shared_workspace_names_keep_retry_directory_and_tool_visible(
    #[case] width: u16,
    #[case] height: u16,
) {
    let mut state = EnvironmentSession::new(
        ["dev", "prod"]
            .map(|name| Environment {
                tool: Tool::OpenTofu,
                availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                    directory: PathBuf::from(format!("/synthetic/{name}")),
                    workspace: "staging".to_owned(),
                }),
            })
            .into_iter()
            .collect(),
        false,
    );
    for _ in 0..2 {
        let index = state.start_next().unwrap();
        state.complete(
            index,
            PlanResult::Error("Synthetic acquisition error".to_owned()),
            Vec::new(),
        );
    }
    let mut view = EnvironmentView::default();
    press(&mut view, &mut state, KeyCode::Right);
    let output = text(&mut view, &state, (width, height));

    assert!(!output.contains("/synthetic/prod"));
    assert!(output.contains("staging · tofu"));
    insta::assert_snapshot!(format!("shared_workspace_{width}x{height}"), output);
    press(&mut view, &mut state, KeyCode::Char('c'));
    assert!(text(&mut view, &state, (width, height)).contains("/synthetic/prod"));
    press(&mut view, &mut state, KeyCode::Esc);
    let input = view.handle_key(
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        Size::new(width, height),
        &state,
    );
    assert!(matches!(input, Some(EnvironmentInput::Retry(1))));
}
