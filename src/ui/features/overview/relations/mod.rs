#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "GR03 introduces the renderer before GR06 and GR07 connect it"
    )
)]

mod render;

#[expect(
    unused_imports,
    reason = "GR06 and GR07 are the planned consumers of the shared Relations renderer"
)]
pub(crate) use render::{RelationGraphScroll, RelationGraphView, render};
