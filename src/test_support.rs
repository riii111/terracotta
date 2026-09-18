#[allow(
    clippy::redundant_pub_crate,
    reason = "the runtime is the only crate-internal test-support caller"
)]
pub(crate) fn panic_after_draw_requested() -> bool {
    std::env::var_os("TERRACOTTA_TEST_PANIC_AFTER_DRAW").is_some()
}
