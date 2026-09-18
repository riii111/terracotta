#[test]
fn realistic_diagnostics_120x40() {
    let state = realistic_diagnostics();
    let list = realistic_list();

    insta::assert_snapshot!(buffer_text(&render_to_buffer((120, 40), |frame| {
        render_diagnostics(frame, &list, &state, DiagnosticsViewState::default());
    })));
}

#[test]
fn realistic_diagnostics_80x24() {
    let state = realistic_diagnostics();
    let list = realistic_list();

    insta::assert_snapshot!(buffer_text(&render_to_buffer((80, 24), |frame| {
        render_diagnostics(frame, &list, &state, DiagnosticsViewState::default());
    })));
}
