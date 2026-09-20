mod input;
mod render;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionScroll {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Top,
    LeftEdge,
    RightEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutionViewState {
    scroll: u16,
    horizontal: u16,
    follow: bool,
}

impl Default for ExecutionViewState {
    fn default() -> Self {
        Self {
            scroll: 0,
            horizontal: 0,
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
            ExecutionScroll::Top => 0,
            ExecutionScroll::Left
            | ExecutionScroll::Right
            | ExecutionScroll::LeftEdge
            | ExecutionScroll::RightEdge => current_offset,
        };
        self.follow = false;
        self.scroll = offset;
    }

    pub(crate) fn apply_horizontal_scroll(
        &mut self,
        action: ExecutionScroll,
        current_offset: u16,
        max_offset: u16,
    ) {
        self.horizontal = match action {
            ExecutionScroll::Left => current_offset.saturating_sub(1),
            ExecutionScroll::Right => current_offset.saturating_add(1).min(max_offset),
            ExecutionScroll::LeftEdge => 0,
            ExecutionScroll::RightEdge => max_offset,
            _ => current_offset,
        };
        self.follow = false;
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

    #[must_use]
    pub(crate) const fn horizontal(self) -> u16 {
        self.horizontal
    }
}

pub(crate) use input::{ExecutionInput, execution_key_to_input};
pub(crate) use render::{
    execution_horizontal_scroll_position_with_view, execution_layout,
    execution_scroll_position_with_view, render_execution_with_view,
};
