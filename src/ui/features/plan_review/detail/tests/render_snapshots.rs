#[test]
fn realistic_detail_120x40() {
    let state = realistic_state();

    insta::assert_snapshot!(buffer_text(&render(&state, 120, 40)));
}

#[test]
fn realistic_detail_80x24() {
    let state = realistic_state();

    insta::assert_snapshot!(buffer_text(&render(&state, 80, 24)));
}
