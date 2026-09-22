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
    let key = state.start_next().expect("pending environment");
    let directory = state.plans()[key.index].directory().to_owned();
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
        changes: Vec::new(),
        resource_changes: changes,
        value_addresses: BTreeSet::new(),
        summary,
        unsupported_changes: Vec::new(),
        output_changes: Vec::new(),
    });
    state.complete(
        key,
        PlanResult::Ready {
            review: Box::new(review),
            changed: true,
        },
        Vec::new(),
    );
}

fn press(view: &mut EnvironmentView, state: &mut EnvironmentSession, code: KeyCode) {
    if let Some(input) = view.handle_key(
        KeyEvent::new(code, KeyModifiers::NONE),
        Size::new(80, 24),
        state,
    ) {
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

    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.modifier.contains(Modifier::REVERSED))
    );
    insta::assert_snapshot!(
        format!("three_environments_{width}x{height}"),
        buffer_text(&buffer)
    );
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
    let key = state.start_next().unwrap();
    state.complete(
        key,
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
