use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

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
    press_event_at(view, state, key, Size::new(80, 24));
}

fn press_event_at(
    view: &mut EnvironmentView,
    state: &mut EnvironmentSession,
    key: KeyEvent,
    size: Size,
) {
    if let Some(input) = view.handle_key(key, size, state) {
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

fn press_at(view: &mut EnvironmentView, state: &mut EnvironmentSession, code: KeyCode, size: Size) {
    press_event_at(view, state, KeyEvent::new(code, KeyModifiers::NONE), size);
}

#[test]
fn brackets_open_the_adjacent_full_plan_from_raw_plan() {
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
    press(&mut view, &mut state, KeyCode::Char('['));
    assert_eq!(view.selection.column, 0);
    press(&mut view, &mut state, KeyCode::Char(']'));
    assert_eq!(view.selection.column, 1);

    press(&mut view, &mut state, KeyCode::Char('v'));
    assert_eq!(view.selection.raw, Some(1));
    assert!(text(&mut view, &state, (80, 24)).contains("# terraform_data.api will change"));

    press(&mut view, &mut state, KeyCode::Char(']'));
    assert_eq!(view.selection.raw, Some(2));
    assert!(text(&mut view, &state, (80, 24)).contains("# terraform_data.api will change"));

    press_event(
        &mut view,
        &mut state,
        KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE),
    );
    assert_eq!(view.selection.raw, Some(1));
    assert!(text(&mut view, &state, (80, 24)).contains("# terraform_data.api will change"));
}

fn text(view: &mut EnvironmentView, state: &EnvironmentSession, size: (u16, u16)) -> String {
    buffer_text(&render_to_buffer(size, |frame| view.render(frame, state)))
}

#[rstest]
#[case::small(80, 24)]
#[case::medium(120, 40)]
#[case::large(160, 60)]
#[case::narrow(40, 16)]
fn three_environments_show_matrix_actions_across_supported_widths(
    #[case] width: u16,
    #[case] height: u16,
) {
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
    assert_eq!(reversed_cells, 0, "the matrix has no selected resource row");
    let rendered = buffer_text(&buffer);
    assert!(rendered.contains("terracotta ▸ dev"));
    assert!(!rendered.contains("0 Overview"));
    if (width, height) == (40, 16) {
        view.handle_key(
            KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
            Size::new(width, height),
            &state,
        );
        let bottom = text(&mut view, &state, (width, height));
        assert!(bottom.contains("blank: absent"), "{bottom}");
        assert!(bottom.contains("?: plan unavailable"), "{bottom}");
        assert!(!bottom.contains("Total"), "{bottom}");
    } else {
        assert!(rendered.contains("blank: absent"));
        assert!(rendered.contains("?: plan unavailable"));
    }
    assert!(!rendered.contains("> terraform_data.api"));
    assert!(rendered.contains("[2] Differs across envs"));
    assert!(!rendered.contains("Total"));
    let matrix_header = rendered
        .lines()
        .position(|line| line.contains("Address"))
        .unwrap();
    assert!(
        !rendered
            .lines()
            .nth(matrix_header)
            .unwrap()
            .contains("> dev")
    );
    assert_eq!(
        buffer
            .cell((2, u16::try_from(matrix_header).unwrap()))
            .unwrap()
            .bg,
        Color::Reset
    );
    assert_eq!(buffer.cell((0, 0)).unwrap().bg, Color::Reset);
    assert_eq!(
        buffer.cell((width - 1, height - 1)).unwrap().bg,
        Color::Reset
    );
    if width >= 120 {
        assert!(rendered.contains("* [1] Envs"));
        assert!(rendered.contains("Space include/exclude"));
    } else {
        assert!(rendered.contains(if width == 40 {
            "Space expand"
        } else {
            "Space expand all"
        }));
        assert!(!rendered.contains("[1] Envs"));
    }
    insta::assert_snapshot!(format!("three_environments_{width}x{height}"), rendered);
}

#[test]
fn selected_environment_stays_viewable_after_leaving_the_comparison() {
    let mut state = session(&["dev", "prod"]);
    complete(
        &mut state,
        vec![change("terraform_data.alpha", ResourceChangeKind::Update)],
    );
    complete(
        &mut state,
        vec![change("terraform_data.zeta", ResourceChangeKind::Update)],
    );
    let mut view = EnvironmentView::default();
    press_at(&mut view, &mut state, KeyCode::Down, Size::new(120, 40));
    press_at(
        &mut view,
        &mut state,
        KeyCode::Char('1'),
        Size::new(120, 40),
    );
    press_at(
        &mut view,
        &mut state,
        KeyCode::Char(' '),
        Size::new(120, 40),
    );
    assert_eq!(view.selected_environments, Some(vec![0]));
    assert_eq!(view.selection.column, 1);

    let rendered = text(&mut view, &state, (120, 40));
    let header = rendered
        .lines()
        .find(|line| line.contains("Address"))
        .unwrap();
    assert!(header.contains("dev"), "{header}");
    assert!(!header.contains("prod"), "{header}");
    assert!(rendered.contains("> [ ] prod"), "{rendered}");

    press_at(&mut view, &mut state, KeyCode::Enter, Size::new(120, 40));
    assert_eq!(view.selection.raw, Some(1));
}

#[test]
fn sidebar_counts_and_matrix_actions_use_ansi_operation_colors() {
    let mut state = session(&["dev", "test", "stg", "prod"]);
    for kind in [
        ResourceChangeKind::Create,
        ResourceChangeKind::Update,
        ResourceChangeKind::Delete,
        ResourceChangeKind::Replace,
    ] {
        complete(&mut state, vec![change("terraform_data.api", kind)]);
    }
    let mut view = EnvironmentView::default();
    let buffer = render_to_buffer((160, 40), |frame| view.render(frame, &state));
    let rendered = buffer_text(&buffer);

    for (label, foreground) in [
        ("+1", Color::Green),
        ("~1", Color::Yellow),
        ("-1", Color::Red),
        ("1 replace", Color::Magenta),
    ] {
        let (y, line) = rendered
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(label))
            .unwrap_or_else(|| panic!("missing sidebar count {label}: {rendered}"));
        let byte_index = line.find(label).unwrap();
        let x = u16::try_from(ratatui::text::Line::from(&line[..byte_index]).width()).unwrap();
        let cell = buffer.cell((x, u16::try_from(y).unwrap())).unwrap();
        assert_eq!(cell.fg, foreground, "{label}");
        assert_eq!(cell.bg, Color::Reset, "{label}");
    }

    let (row_y, row) = rendered
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("terraform_data.api"))
        .expect("matrix API row");
    let replace_byte_index = row.find("+/-").expect("replace action");
    let replace_x =
        u16::try_from(ratatui::text::Line::from(&row[..replace_byte_index]).width()).unwrap();
    let replace_cell = buffer
        .cell((replace_x, u16::try_from(row_y).unwrap()))
        .unwrap();
    assert_eq!(replace_cell.fg, Color::Magenta);
    assert_eq!(replace_cell.bg, Color::Reset);
}

#[test]
fn space_toggles_all_groups_independently_of_matrix_scroll() {
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
    assert!(collapsed.contains("Space expand all"));
    assert!(text(&mut view, &state, (40, 16)).contains("Space expand"));
    for (width, height) in [(40, 16), (80, 24), (120, 40), (160, 60)] {
        let rendered = text(&mut view, &state, (width, height));
        let environment_hint = "[/] env";
        for hint in [
            environment_hint,
            "/ filter",
            if width == 40 {
                "Space expand"
            } else {
                "Space expand all"
            },
            if width == 40 {
                "?/q help/quit"
            } else {
                "? help"
            },
        ] {
            assert!(rendered.contains(hint), "{width}x{height}: {hint}");
        }
        if width != 40 {
            assert!(rendered.contains("q quit"), "{width}x{height}");
        }
        assert!(rendered.contains("Enter open plan"), "{width}x{height}");
        assert!(rendered.contains("v full plan"), "{width}x{height}");
        assert!(!rendered.contains("↑↓"), "{width}x{height}");
        assert!(!rendered.contains("←→"), "{width}x{height}");
    }

    press(&mut view, &mut state, KeyCode::Char(' '));
    let expanded = text(&mut view, &state, (80, 24));
    assert!(expanded.contains("[-] terraform_data.server[*]"));
    assert!(expanded.contains("terraform_data.server[0]"));
    assert!(expanded.contains("Space collapse all"));
    assert!(expanded.contains("q quit"));

    press(&mut view, &mut state, KeyCode::Char('j'));
    let child = text(&mut view, &state, (80, 24));
    assert!(child.contains("Space collapse all"));
    press(&mut view, &mut state, KeyCode::Char(' '));
    let collapsed = text(&mut view, &state, (80, 24));
    assert!(collapsed.contains("[+] terraform_data.server[*]"));

    press(&mut view, &mut state, KeyCode::Char('k'));
    assert!(text(&mut view, &state, (80, 24)).contains("Space expand all"));

    press(&mut view, &mut state, KeyCode::Char('/'));
    for character in "server[1]".chars() {
        press(&mut view, &mut state, KeyCode::Char(character));
    }
    press(&mut view, &mut state, KeyCode::Enter);
    let filtered = text(&mut view, &state, (80, 24));
    assert!(filtered.contains("terraform_data.server[1]"));
    assert!(!filtered.contains("terraform_data.server[*]"));
    assert!(!filtered.contains("Space expand all"));
    assert!(!filtered.contains("Space collapse all"));
    press(&mut view, &mut state, KeyCode::Char(' '));
    assert_eq!(text(&mut view, &state, (80, 24)), filtered);
}

#[test]
fn short_terminal_keeps_major_environment_actions_without_movement_hints() {
    let state = session(&["dev", "prod", "stg"]);
    let mut view = EnvironmentView::default();
    let rendered = text(&mut view, &state, (40, 14));

    assert!(rendered.contains("Enter open plan"));
    for hint in ["[/] env", "Enter open plan", "/ filter", "?/q help/quit"] {
        assert!(rendered.contains(hint), "{hint}");
    }
    assert!(!rendered.contains("↑↓"));
    assert!(!rendered.contains("←→"));
    let lines = rendered.lines().collect::<Vec<_>>();
    assert!(
        !lines
            .iter()
            .any(|line| line.trim_start().starts_with("Total"))
    );
    assert!(!rendered.contains(&"─".repeat(40)));
}

#[test]
fn narrow_matrix_keeps_why_visible_with_a_long_selected_environment_name() {
    let state = session(&["production-eu-west-1"]);
    let mut view = EnvironmentView::default();
    let rendered = text(&mut view, &state, (40, 16));
    let header = rendered
        .lines()
        .find(|line| line.contains("Address"))
        .expect("matrix header");

    assert!(!header.contains("> "));
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
    let rendered = text(&mut view, &state, (80, 24));
    insta::assert_snapshot!("twelve_ready_selected_last", rendered.as_str());
    let header = rendered
        .lines()
        .find(|line| line.contains("Address"))
        .expect("matrix header");
    assert!(header.contains("env-11"), "{header}");
    press(&mut view, &mut state, KeyCode::Char('v'));
    assert_eq!(view.selection.raw, Some(11));
    press(&mut view, &mut state, KeyCode::Char('['));
    assert_eq!(view.selection.raw, Some(10));
    press(&mut view, &mut state, KeyCode::Esc);
    assert_eq!(view.selection.column, 11);
}

#[rstest]
#[case::one(1)]
#[case::three(3)]
#[case::ten(10)]
fn matrix_symbol_legend_remains_visible_across_environment_counts(#[case] count: usize) {
    let names = (0..count)
        .map(|index| format!("env-{index:02}"))
        .collect::<Vec<_>>();
    let state = session(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let mut view = EnvironmentView::default();
    let rendered = text(&mut view, &state, (120, 40));

    assert!(rendered.contains("blank: absent"));
    assert!(rendered.contains(".: unchanged"));
    assert!(rendered.contains("?: plan unavailable"));
}

#[test]
fn unavailable_environment_opens_its_state_dialog_and_raw_plan_after_completion() {
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
    let size = Size::new(120, 40);
    press_at(&mut view, &mut state, KeyCode::Down, size);
    press_at(&mut view, &mut state, KeyCode::Enter, size);
    assert_eq!(view.selection.raw, Some(1));
    assert!(
        text(&mut view, &state, (120, 40)).contains("Terraform will perform the following actions")
    );
    press_at(&mut view, &mut state, KeyCode::Esc, size);
    press_at(&mut view, &mut state, KeyCode::Down, size);
    press_at(&mut view, &mut state, KeyCode::Enter, size);
    assert!(text(&mut view, &state, (120, 40)).contains("Pending"));
    assert!(view.selection.raw.is_none());
    press_at(&mut view, &mut state, KeyCode::Esc, size);
    complete(&mut state, Vec::new());
    press_at(&mut view, &mut state, KeyCode::Enter, size);
    let rendered = text(&mut view, &state, (120, 40));
    assert!(
        rendered.contains("Terraform will perform the following actions"),
        "{rendered}"
    );
    assert_eq!(view.selection.raw, Some(2));
}

#[test]
fn space_toggles_all_displayed_resource_groups() {
    let mut state = session(&["dev", "prod"]);
    let changes: Vec<_> = (0..2)
        .flat_map(|group| {
            (0..2).map(move |instance| {
                change(
                    &format!("terraform_data.server_{group}[{instance}]"),
                    ResourceChangeKind::Update,
                )
            })
        })
        .collect();
    complete(&mut state, changes.clone());
    complete(&mut state, changes);
    let mut view = EnvironmentView::default();

    let collapsed = text(&mut view, &state, (120, 40));
    assert!(
        collapsed.contains("[+] terraform_data.server_0[*]"),
        "{collapsed}"
    );
    assert!(
        collapsed.contains("[+] terraform_data.server_1[*]"),
        "{collapsed}"
    );
    press(&mut view, &mut state, KeyCode::Char(' '));
    let expanded = text(&mut view, &state, (120, 40));
    assert!(
        expanded.contains("[-] terraform_data.server_0[*]"),
        "{expanded}"
    );
    assert!(
        expanded.contains("[-] terraform_data.server_1[*]"),
        "{expanded}"
    );
    assert!(expanded.contains("server_0[0]"), "{expanded}");
    assert!(expanded.contains("server_1[1]"), "{expanded}");
    press(&mut view, &mut state, KeyCode::Char(' '));
    let collapsed = text(&mut view, &state, (120, 40));
    assert!(
        collapsed.contains("[+] terraform_data.server_0[*]"),
        "{collapsed}"
    );
    assert!(
        collapsed.contains("[+] terraform_data.server_1[*]"),
        "{collapsed}"
    );
}

#[test]
fn filter_uses_complete_addresses_and_raw_return_preserves_expansion() {
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
    press(&mut view, &mut state, KeyCode::Char('v'));
    assert_eq!(view.selection.raw, Some(1));
    assert_eq!(view.reviews[1].scroll().0, 0);
    press(&mut view, &mut state, KeyCode::Esc);
    assert_eq!(view.selection.column, 1);
    assert_eq!(text(&mut view, &state, (80, 24)), before);
    assert!(
        text(&mut view, &state, (80, 24)).contains("module.long_name.terraform_data.server[198]")
    );
}

#[test]
fn matrix_search_edits_graphemes_and_restores_the_previous_filter_on_cancel() {
    let mut state = session(&["a"]);
    complete(
        &mut state,
        vec![
            change("terraform_data.alpha", ResourceChangeKind::Update),
            change("terraform_data.beta", ResourceChangeKind::Update),
        ],
    );
    let mut view = EnvironmentView::default();
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
    assert!(text(&mut view, &state, (80, 24)).contains("terraform_data.beta"));

    press(&mut view, &mut state, KeyCode::Char('/'));
    for character in "terraform_data.beta".chars() {
        press(&mut view, &mut state, KeyCode::Char(character));
    }
    press(&mut view, &mut state, KeyCode::Enter);
    assert_eq!(view.matrix.filter(), "terraform_data.beta");
    assert!(text(&mut view, &state, (80, 24)).contains("terraform_data.beta"));

    press(&mut view, &mut state, KeyCode::Char('/'));
    press(&mut view, &mut state, KeyCode::Char('x'));
    press(&mut view, &mut state, KeyCode::Esc);
    assert_eq!(view.matrix.filter(), "terraform_data.beta");
    assert!(text(&mut view, &state, (80, 24)).contains("terraform_data.beta"));

    press(&mut view, &mut state, KeyCode::Char('/'));
    press(&mut view, &mut state, KeyCode::Char(']'));
    assert_eq!(view.selection.column, 0);
    assert_eq!(view.matrix.filter(), "terraform_data.beta]");
    press(&mut view, &mut state, KeyCode::Esc);
    assert_eq!(view.matrix.filter(), "terraform_data.beta");
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
    complete(
        &mut state,
        vec![
            change("terraform_data.server[0]", ResourceChangeKind::Replace),
            change("terraform_data.server[1]", ResourceChangeKind::NoOp),
        ],
    );
    let output = text(&mut view, &state, (80, 24));
    assert_eq!(view.selection.column, 2);
    assert!(output.contains("terraform_data.server[0]"), "{output}");
    assert!(output.contains("terracotta ▸ c"), "{output}");
    assert!(output.contains("Ready"));
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

#[test]
fn focus_keys_open_and_close_the_sidebar_without_changing_selection() {
    let mut state = session(&["dev", "prod", "stg"]);
    for _ in 0..3 {
        complete(
            &mut state,
            vec![change("terraform_data.api", ResourceChangeKind::Update)],
        );
    }
    let mut view = EnvironmentView::default();
    let size = Size::new(120, 40);

    press_at(&mut view, &mut state, KeyCode::Char('2'), size);
    assert_eq!(view.focus, EnvironmentPane::Matrix);
    press_at(&mut view, &mut state, KeyCode::Char('1'), size);
    assert_eq!(view.focus, EnvironmentPane::Environments);
    press_at(&mut view, &mut state, KeyCode::Down, size);
    assert_eq!(view.selection.column, 1);
    press_at(&mut view, &mut state, KeyCode::Char('b'), size);
    assert_eq!(view.sidebar, SidebarSetting::Closed);
    assert_eq!(view.focus, EnvironmentPane::Matrix);
    press_at(&mut view, &mut state, KeyCode::Char('b'), size);
    assert_eq!(view.sidebar, SidebarSetting::Open);
    assert_eq!(view.focus, EnvironmentPane::Matrix);
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
    assert!(output.contains("terracotta ▸ prod"));
    assert!(!output.contains("staging · tofu"));
    insta::assert_snapshot!(format!("shared_workspace_{width}x{height}"), output);
    press(&mut view, &mut state, KeyCode::Char('c'));
    let context = text(&mut view, &state, (width, height));
    assert!(context.contains("/synthetic/prod"));
    assert!(context.contains("tofu"));
    press(&mut view, &mut state, KeyCode::Esc);
    let input = view.handle_key(
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        Size::new(width, height),
        &state,
    );
    assert!(matches!(input, Some(EnvironmentInput::Retry(1))));
}
