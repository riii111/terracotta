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
use viewport::{apply_scroll, clamp_scroll, ensure_selected_visible};

const MIN_HEIGHT: u16 = 8;
const MIN_WIDTH: u16 = 48;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DetailViewState {
    scroll: u16,
    analysis_info_expanded: bool,
}

impl DetailViewState {
    pub(crate) const fn scroll(&self) -> u16 {
        self.scroll
    }

    pub(crate) const fn analysis_info_expanded(&self) -> bool {
        self.analysis_info_expanded
    }

    pub(crate) const fn toggle_analysis_info(&mut self) {
        self.analysis_info_expanded = !self.analysis_info_expanded;
    }

    pub(crate) const fn reset(&mut self) {
        self.scroll = 0;
        self.analysis_info_expanded = false;
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
    let content = rows::detail_content(
        list,
        detail,
        view.analysis_info_expanded(),
        layout.body().width,
        now,
    );
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
    let content = rows::detail_content(
        list,
        detail,
        view.analysis_info_expanded(),
        layout.body().width,
        now,
    );
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
    let content = rows::detail_content(
        list,
        detail,
        view.analysis_info_expanded(),
        layout.body().width,
        now,
    );
    clamp_scroll(view, &content, layout.body().width, layout.body().height);
}

#[cfg(test)]
mod test_support;
