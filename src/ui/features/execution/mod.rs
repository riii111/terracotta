mod input;
mod render;

use crate::app::execution::ExecutionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionScroll {
    Up,
    Down,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ExecutionViewState {
    scroll: u16,
    follow: bool,
}

impl ExecutionViewState {
    #[must_use]
    pub(crate) const fn from_state(_state: &ExecutionState) -> Self {
        Self {
            scroll: 0,
            follow: true,
        }
    }

    pub(crate) fn apply_scroll(
        &mut self,
        action: ExecutionScroll,
        current_offset: u16,
        max_offset: u16,
    ) {
        let offset = match action {
            ExecutionScroll::Up => current_offset.saturating_sub(1),
            ExecutionScroll::Down => current_offset.saturating_add(1).min(max_offset),
            ExecutionScroll::PageUp => current_offset.saturating_sub(8),
            ExecutionScroll::PageDown => current_offset.saturating_add(8).min(max_offset),
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
    execution_chunks, execution_scroll_position_with_view, render_execution_with_view,
};
