use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

use ratatui::style::Modifier;
use rstest::rstest;

use super::*;
use crate::app::{
    execution::ExecutionContext,
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
    let index = state.start_next().expect("pending environment");
    let directory = state.plans()[index].directory().to_owned();
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
    .with_resource_changes(Vec::new(), summary.replaces);
    let review = PlanReview::new(
        directory.clone(),
        "default".to_owned(),
        PlanDocument::with_blocks_and_line_kinds(lines.join("\n"), blocks, Vec::new()),
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

    press(&mut view, &mut state, KeyCode::Enter);
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
    assert!(!rendered.contains("Space expand"));
    assert!(!rendered.contains("Space collapse"));
    insta::assert_snapshot!(format!("three_environments_{width}x{height}"), rendered);
}

#[test]
fn selected_group_footer_tracks_expansion_children_and_filtered_rows() {
    let mut state = session(&["dev", "prod", "stg"]);
    for _ in 0..3 {
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
    let mut view = EnvironmentView::default();

    let collapsed = text(&mut view, &state, (80, 24));
    assert!(collapsed.contains("[+] terraform_data.server[*]"));
    assert!(collapsed.contains("Space expand"));
    assert!(text(&mut view, &state, (40, 16)).contains("Space expand"));

    press(&mut view, &mut state, KeyCode::Char(' '));
    let expanded = text(&mut view, &state, (80, 24));
    assert!(expanded.contains("[-] terraform_data.server[*]"));
    assert!(expanded.contains("terraform_data.server[0]"));
    assert!(expanded.contains("Space collapse"));
    assert!(expanded.contains("q quit"));

    press(&mut view, &mut state, KeyCode::Char('j'));
    let child = text(&mut view, &state, (80, 24));
    assert!(!child.contains("Space expand"));
    assert!(!child.contains("Space collapse"));
    press(&mut view, &mut state, KeyCode::Char(' '));
    assert_eq!(text(&mut view, &state, (80, 24)), child);

    press(&mut view, &mut state, KeyCode::Char('k'));
    assert!(text(&mut view, &state, (80, 24)).contains("Space collapse"));
    press(&mut view, &mut state, KeyCode::Char(' '));
    let collapsed_again = text(&mut view, &state, (80, 24));
    assert!(collapsed_again.contains("[+] terraform_data.server[*]"));
    assert!(collapsed_again.contains("Space expand"));
    assert!(!collapsed_again.contains("terraform_data.server[0]"));

    press(&mut view, &mut state, KeyCode::Char('/'));
    for character in "server[1]".chars() {
        press(&mut view, &mut state, KeyCode::Char(character));
    }
    press(&mut view, &mut state, KeyCode::Enter);
    let filtered = text(&mut view, &state, (80, 24));
    assert!(filtered.contains("terraform_data.server[1]"));
    assert!(!filtered.contains("terraform_data.server[*]"));
    assert!(!filtered.contains("Space expand"));
    assert!(!filtered.contains("Space collapse"));
    press(&mut view, &mut state, KeyCode::Char(' '));
    assert_eq!(text(&mut view, &state, (80, 24)), filtered);
}

#[test]
fn short_terminal_keeps_environment_actions_without_boundary_rows() {
    let state = session(&["dev", "prod", "stg"]);
    let mut view = EnvironmentView::default();
    let rendered = text(&mut view, &state, (40, 14));

    assert!(rendered.contains("Enter open  e env filter  q quit"));
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
    press(&mut view, &mut state, KeyCode::Enter);
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
    assert!(text(&mut view, &state, (80, 24)).contains("does not exist"));
    assert!(view.selection.raw.is_none());
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
    press(&mut view, &mut state, KeyCode::Enter);
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

#[test]
fn environment_filter_updates_comparison_columns_and_preserves_global_ready_progress() {
    let mut state = session(&["dev", "prod", "stg"]);
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::Update)],
    );
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::Update)],
    );
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::Create)],
    );
    let mut view = EnvironmentView::default();

    press(&mut view, &mut state, KeyCode::Char('e'));
    press(&mut view, &mut state, KeyCode::Down);
    press(&mut view, &mut state, KeyCode::Down);
    press(&mut view, &mut state, KeyCode::Char(' '));
    press(&mut view, &mut state, KeyCode::Enter);

    let output = text(&mut view, &state, (80, 24));
    assert!(output.contains("Ready: 3/3   [Env filter ON]"), "{output}");
    assert!(
        output.contains("Same change across selected envs"),
        "{output}"
    );
    assert!(output.contains("1 dev  2 prod"), "{output}");
    assert!(!output.contains("3 stg"), "{output}");
    assert!(!output.contains("Compared:"), "{output}");
}

#[test]
fn environment_filter_escape_discards_unapplied_toggles() {
    let mut state = session(&["dev", "prod"]);
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::Update)],
    );
    complete(
        &mut state,
        vec![change("terraform_data.api", ResourceChangeKind::Update)],
    );
    let mut view = EnvironmentView::default();

    press(&mut view, &mut state, KeyCode::Char('e'));
    press(&mut view, &mut state, KeyCode::Char(' '));
    press(&mut view, &mut state, KeyCode::Esc);

    let output = text(&mut view, &state, (80, 24));
    assert!(!output.contains("[Env filter ON]"), "{output}");
    assert!(output.contains("1 dev  2 prod"), "{output}");
}

#[test]
fn filtered_numbered_tabs_open_the_environment_with_the_visible_number() {
    let names = (0..10)
        .map(|index| format!("env-{index:02}"))
        .collect::<Vec<_>>();
    let mut state = session(&names.iter().map(String::as_str).collect::<Vec<_>>());
    for _ in 0..10 {
        complete(
            &mut state,
            vec![change("terraform_data.api", ResourceChangeKind::Update)],
        );
    }
    let mut view = EnvironmentView::default();

    press(&mut view, &mut state, KeyCode::Char('e'));
    for index in 0..10 {
        if index != 2 && index != 9 {
            press(&mut view, &mut state, KeyCode::Char(' '));
        }
        if index < 9 {
            press(&mut view, &mut state, KeyCode::Down);
        }
    }
    press(&mut view, &mut state, KeyCode::Enter);

    let output = text(&mut view, &state, (120, 40));
    assert!(output.contains("1 env-02  2 env-09"), "{output}");
    assert!(!output.contains("env-00"), "{output}");
    press(&mut view, &mut state, KeyCode::Char('3'));
    assert!(view.selection.raw.is_none());
    press(&mut view, &mut state, KeyCode::Char('2'));
    assert_eq!(view.selection.raw, Some(9));
}

#[test]
fn filter_can_narrow_a_three_environment_matrix_to_one_selected_environment() {
    let mut state = session(&["dev", "prod", "stg"]);
    for kind in [
        ResourceChangeKind::Update,
        ResourceChangeKind::Update,
        ResourceChangeKind::Create,
    ] {
        complete(&mut state, vec![change("terraform_data.api", kind)]);
    }
    let mut view = EnvironmentView::default();

    press(&mut view, &mut state, KeyCode::Char('e'));
    press(&mut view, &mut state, KeyCode::Char(' '));
    press(&mut view, &mut state, KeyCode::Down);
    press(&mut view, &mut state, KeyCode::Down);
    press(&mut view, &mut state, KeyCode::Char(' '));
    press(&mut view, &mut state, KeyCode::Enter);

    let output = text(&mut view, &state, (80, 24));
    assert!(output.contains("Ready: 3/3   [Env filter ON]"), "{output}");
    assert!(output.contains("1 prod"), "{output}");
    assert!(!output.contains("1 dev"), "{output}");
    assert!(!output.contains("2 stg"), "{output}");
    assert_eq!(view.selection.column, 1);
    press(&mut view, &mut state, KeyCode::Enter);
    assert_eq!(view.selection.raw, Some(1));
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
