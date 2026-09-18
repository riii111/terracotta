use arboard::Clipboard;

use crate::app::copy::{CopyEffect, CopyResult};

pub(crate) struct SystemClipboard {
    clipboard: Option<Clipboard>,
}

impl SystemClipboard {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            clipboard: Clipboard::new().ok(),
        }
    }

    pub(crate) fn execute(&mut self, effect: &CopyEffect) -> CopyResult {
        if self.write(effect.text()) {
            CopyResult::Written
        } else {
            CopyResult::Failed
        }
    }

    fn write(&mut self, text: &str) -> bool {
        self.clipboard
            .as_mut()
            .is_some_and(|clipboard| clipboard.set_text(text).is_ok())
    }
}
