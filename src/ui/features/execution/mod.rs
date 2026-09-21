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
enum VerticalScroll {
    Initial,
    FollowLatest,
    Manual(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutionViewState {
    vertical: VerticalScroll,
    horizontal: u16,
    logs_open: bool,
}

impl Default for ExecutionViewState {
    fn default() -> Self {
        Self {
            vertical: VerticalScroll::Initial,
            horizontal: 0,
            logs_open: false,
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
        self.vertical = VerticalScroll::Manual(offset);
    }

    pub(crate) fn apply_horizontal_scroll(
        &mut self,
        action: ExecutionScroll,
        current_offset: u16,
        max_offset: u16,
        current_vertical: u16,
    ) {
        self.vertical = VerticalScroll::Manual(current_vertical);
        self.horizontal = match action {
            ExecutionScroll::Left => current_offset.saturating_sub(1),
            ExecutionScroll::Right => current_offset.saturating_add(1).min(max_offset),
            ExecutionScroll::LeftEdge => 0,
            ExecutionScroll::RightEdge => max_offset,
            _ => current_offset,
        };
    }

    pub(crate) const fn end(&mut self) {
        self.vertical = VerticalScroll::FollowLatest;
    }

    pub(crate) const fn open_logs(&mut self) {
        self.logs_open = true;
        self.vertical = VerticalScroll::FollowLatest;
        self.horizontal = 0;
    }

    pub(crate) const fn close_logs(&mut self) {
        self.logs_open = false;
    }

    #[must_use]
    pub(crate) const fn logs_open(self) -> bool {
        self.logs_open
    }

    #[must_use]
    pub(crate) const fn follows_latest(self) -> bool {
        !matches!(self.vertical, VerticalScroll::Manual(_))
    }

    #[must_use]
    pub(crate) const fn horizontal(self) -> u16 {
        self.horizontal
    }

    #[must_use]
    pub(crate) const fn vertical_offset(self, initial: u16, max: u16) -> u16 {
        match self.vertical {
            VerticalScroll::Initial => {
                if initial < max {
                    initial
                } else {
                    max
                }
            }
            VerticalScroll::FollowLatest => max,
            VerticalScroll::Manual(offset) => {
                if offset < max {
                    offset
                } else {
                    max
                }
            }
        }
    }
}

pub(crate) use input::{ExecutionInput, execution_key_to_input};
pub(crate) use render::{
    execution_horizontal_scroll_position_with_view, execution_layout_with_view,
    execution_scroll_position_with_view, render_execution_with_quit_confirmation,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_scroll_keeps_the_effective_follow_position() {
        let mut view = ExecutionViewState::default();

        view.apply_horizontal_scroll(ExecutionScroll::Right, 0, 5, 42);

        assert_eq!(view.vertical_offset(0, 42), 42);
        assert_eq!(view.horizontal(), 1);
        assert!(!view.follows_latest());
    }

    #[test]
    fn initial_and_follow_latest_use_different_vertical_modes() {
        let mut view = ExecutionViewState::default();

        assert_eq!(view.vertical_offset(2, 90), 2);
        view.end();
        assert_eq!(view.vertical_offset(2, 90), 90);
    }

    #[test]
    fn horizontal_scroll_preserves_manual_vertical_position() {
        let mut view = ExecutionViewState::default();
        view.apply_scroll(ExecutionScroll::Down, 4, 10, 5);

        view.apply_horizontal_scroll(ExecutionScroll::Right, 0, 5, 5);

        assert_eq!(view.vertical_offset(0, 5), 5);
        assert_eq!(view.horizontal(), 1);
    }
}
