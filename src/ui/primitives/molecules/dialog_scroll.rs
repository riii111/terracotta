use std::cell::Cell;

// The limit comes from the last render because only the dialog layout knows its wrapped height.
// Relative moves start from the clamped offset, so End (u16::MAX) is followed by visible movement.
// Before a render reports a limit, moves are not clamped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DialogScroll {
    offset: u16,
    max: Cell<Option<u16>>,
}

impl DialogScroll {
    pub(crate) fn scroll_by(&mut self, delta: i16) {
        let current = self.clamped(self.offset);
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta.cast_unsigned())
        };
        self.offset = self.clamped(next);
    }

    pub(crate) const fn top(&mut self) {
        self.offset = 0;
    }

    pub(crate) const fn bottom(&mut self) {
        self.offset = u16::MAX;
    }

    pub(crate) fn reset(&mut self) {
        self.offset = 0;
        self.max.set(None);
    }

    pub(crate) fn clamp_for_render(&self, max: u16) -> u16 {
        self.max.set(Some(max));
        self.offset.min(max)
    }

    fn clamped(&self, offset: u16) -> u16 {
        self.max.get().map_or(offset, |max| offset.min(max))
    }
}

#[cfg(test)]
mod test_support {
    use super::DialogScroll;

    // Key-mapping tests observe the raw offset without a render; renders read it only
    // through the clamped value.
    impl DialogScroll {
        pub(crate) const fn offset_for_test(&self) -> u16 {
            self.offset
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DialogScroll;

    #[test]
    fn moves_start_from_the_rendered_limit_and_stop_at_it() {
        let mut scroll = DialogScroll::default();
        scroll.bottom();
        assert_eq!(scroll.clamp_for_render(10), 10);

        scroll.scroll_by(-1);
        assert_eq!(scroll.offset, 9);

        scroll.scroll_by(8);
        assert_eq!(scroll.offset, 10);
    }

    #[test]
    fn reset_forgets_the_limit_of_the_previous_dialog() {
        let mut scroll = DialogScroll::default();
        scroll.clamp_for_render(3);

        scroll.reset();
        scroll.scroll_by(20);

        assert_eq!(scroll.offset, 20);
    }
}
