mod input;
mod render;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionScroll {
    Up,
    Down,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutionViewState {
    scroll: u16,
    follow: bool,
}

impl Default for ExecutionViewState {
    fn default() -> Self {
        Self {
            scroll: 0,
            follow: true,
        }
    }
}

impl ExecutionViewState {
    pub(crate) fn apply_scroll(
        &mut self,
        action: ExecutionScroll,
        current_offset: u16,
        max_offset: u16,
        page_height: u16,
    ) {
        let page_height = page_height.max(1);
        let offset = match action {
            ExecutionScroll::Up => current_offset.saturating_sub(1),
            ExecutionScroll::Down => current_offset.saturating_add(1).min(max_offset),
            ExecutionScroll::PageUp => current_offset.saturating_sub(page_height),
            ExecutionScroll::PageDown => current_offset.saturating_add(page_height).min(max_offset),
        };
        self.follow = false;
        self.scroll = offset;
    }

    pub(crate) const fn end(&mut self) {
        self.follow = true;
        self.scroll = 0;
    }

    #[must_use]
    pub(crate) const fn follows_latest(self) -> bool {
        self.follow
    }

    #[must_use]
    pub(crate) const fn scroll(self) -> u16 {
        self.scroll
    }
}

pub(crate) use input::{ExecutionInput, execution_key_to_input};
pub(crate) use render::{
    execution_layout, execution_scroll_position_with_view, render_execution_with_view,
};
