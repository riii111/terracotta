#[test]
fn realistic_detail_120x40() {
    let mut state = realistic_state();
    state.toggle_analysis_info(120, 40, Instant::now());

    insta::assert_snapshot!(buffer_text(&render(&state, 120, 40)));
}

#[test]
fn realistic_detail_80x24() {
    let mut state = realistic_state();
    state.toggle_analysis_info(80, 24, Instant::now());

    insta::assert_snapshot!(buffer_text(&render(&state, 80, 24)));
}

#[test]
fn realistic_analysis_info_contains_all_source_paths() {
    let mut state = realistic_state();
    state.toggle_analysis_info(120, 40, Instant::now());
    let content = detail_content(&state.list, &state.detail, true, Instant::now());
    let text = content
        .lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    for path in [
        "environments/development/main/service.tf",
        "environments/production/main/service.tf",
        "common/main/service.tf",
    ] {
        assert!(text.contains(path), "{path} missing from:\n{text}");
    }
}
