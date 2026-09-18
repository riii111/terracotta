use std::time::Instant;

use crate::app::copy::CopyNotice;
use crate::app::review::{PlanListState, ReviewDetailState};
use ratatui::layout::Rect;

mod input;
mod render;
mod rows;
mod viewport;

pub(crate) use input::{DetailInput, DetailScroll, key_to_input};
pub(crate) use render::{render_resource_detail, resource_detail_layout};
use rows::detail_content;
use viewport::{apply_scroll, clamp_scroll, ensure_selected_visible};

const MIN_HEIGHT: u16 = 8;
const MIN_WIDTH: u16 = 48;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DetailViewState {
    scroll: u16,
}

impl DetailViewState {
    pub(crate) const fn scroll(&self) -> u16 {
        self.scroll
    }

    pub(crate) const fn reset(&mut self) {
        self.scroll = 0;
    }
}

pub(crate) fn apply_detail_scroll(
    view: &mut DetailViewState,
    scroll: DetailScroll,
    list: &PlanListState,
    detail: &ReviewDetailState,
    copy_notice: Option<CopyNotice>,
    now: Instant,
    area: Rect,
) {
    let layout = resource_detail_layout(area, list, detail, copy_notice, now);
    let content = detail_content(list, detail, now);
    apply_scroll(
        view,
        scroll,
        &content,
        layout.body().width,
        layout.body().height,
    );
}

pub(crate) fn ensure_detail_selection_visible(
    view: &mut DetailViewState,
    list: &PlanListState,
    detail: &ReviewDetailState,
    copy_notice: Option<CopyNotice>,
    now: Instant,
    area: Rect,
) {
    let layout = resource_detail_layout(area, list, detail, copy_notice, now);
    let content = detail_content(list, detail, now);
    ensure_selected_visible(view, &content, layout.body().width, layout.body().height);
}

pub(crate) fn clamp_detail_scroll(
    view: &mut DetailViewState,
    list: &PlanListState,
    detail: &ReviewDetailState,
    copy_notice: Option<CopyNotice>,
    now: Instant,
    area: Rect,
) {
    let layout = resource_detail_layout(area, list, detail, copy_notice, now);
    let content = detail_content(list, detail, now);
    clamp_scroll(view, &content, layout.body().width, layout.body().height);
}

#[cfg(test)]
mod test_support;
