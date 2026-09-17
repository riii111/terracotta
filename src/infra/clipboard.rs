use arboard::Clipboard;

use crate::app::copy::{CopyEffect, CopyResult};

pub(crate) trait ClipboardWriter {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipboardError;

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
        match self.write(effect.text()) {
            Ok(()) => CopyResult::Written,
            Err(_) => CopyResult::Failed,
        }
    }
}

impl ClipboardWriter for SystemClipboard {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.clipboard
            .as_mut()
            .ok_or(ClipboardError)?
            .set_text(text)
            .map_err(|_| ClipboardError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeClipboard {
        text: Option<String>,
        fail: bool,
    }

    impl ClipboardWriter for FakeClipboard {
        fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
            if self.fail {
                return Err(ClipboardError);
            }
            self.text = Some(text.to_owned());
            Ok(())
        }
    }

    #[test]
    fn fake_clipboard_retains_written_text() {
        let mut clipboard = FakeClipboard::default();

        clipboard
            .write("redacted resource")
            .expect("write should succeed");

        assert_eq!(clipboard.text.as_deref(), Some("redacted resource"));
    }

    #[test]
    fn fake_clipboard_reports_backend_failure_without_text() {
        let mut clipboard = FakeClipboard {
            fail: true,
            ..FakeClipboard::default()
        };

        assert_eq!(clipboard.write("secret"), Err(ClipboardError));
        assert_eq!(clipboard.text, None);
    }
}
