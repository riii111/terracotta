use super::*;

#[test]
fn plan_list_shows_actions_selection_and_git_evidence() {
    let state = synthetic_state();

    insta::assert_snapshot!(buffer_text(&render_to_buffer(&state, 120, 20)));
}

#[test]
fn narrow_plan_list_wraps_git_evidence() {
    let state = synthetic_state();

    insta::assert_snapshot!(buffer_text(&render_to_buffer(&state, 60, 16)));
}

#[test]
fn empty_plan_list_shows_zero_summary() {
    let state = PlanListState::empty("working tree vs HEAD");

    insta::assert_snapshot!(buffer_text(&render_to_buffer(&state, 80, 12)));
}
