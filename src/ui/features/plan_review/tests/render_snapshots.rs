use crate::ui::test_support::REPRESENTATIVE_TERMINAL_SIZE;

#[test]
fn plan_list_shows_actions_selection_and_git_evidence() {
    let state = synthetic_state();

    insta::assert_snapshot!(buffer_text(&render_to_buffer(
        &state,
        REPRESENTATIVE_TERMINAL_SIZE.0,
        REPRESENTATIVE_TERMINAL_SIZE.1,
    )));
}

#[test]
fn narrow_plan_list_wraps_git_evidence() {
    let state = synthetic_state();

    insta::assert_snapshot!(buffer_text(&render_to_buffer(&state, 60, 16)));
}

#[test]
fn empty_plan_list_shows_zero_summary() {
    let state = empty_state();

    insta::assert_snapshot!(buffer_text(&render_to_buffer(&state, 80, 12)));
}

#[test]
fn realistic_plan_list_120x40() {
    let state = realistic_state();

    insta::assert_snapshot!(buffer_text(&render_to_buffer(&state, 120, 40)));
}

#[test]
fn realistic_plan_list_80x24() {
    let state = realistic_state();

    insta::assert_snapshot!(buffer_text(&render_to_buffer(&state, 80, 24)));
}
