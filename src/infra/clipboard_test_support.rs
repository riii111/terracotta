use std::{env, ffi::OsStr, fs, path::PathBuf};

use crate::app::copy::{CopyEffect, CopyResult};

use super::clipboard::{ClipboardError, ClipboardWriter, SystemClipboard};

struct FileClipboard {
    path: PathBuf,
}

impl ClipboardWriter for FileClipboard {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        fs::write(&self.path, text).map_err(|_| ClipboardError)
    }
}

struct UnavailableClipboard;

impl ClipboardWriter for UnavailableClipboard {
    fn write(&mut self, _text: &str) -> Result<(), ClipboardError> {
        Err(ClipboardError)
    }
}

pub(crate) struct ClipboardExecutor {
    writer: Box<dyn ClipboardWriter>,
}

impl ClipboardExecutor {
    #[must_use]
    pub(crate) fn new() -> Self {
        let writer: Box<dyn ClipboardWriter> = match env::var_os("TERRACOTTA_TEST_CLIPBOARD") {
            Some(path) if path == OsStr::new("unavailable") => Box::new(UnavailableClipboard),
            Some(path) => Box::new(FileClipboard { path: path.into() }),
            None => Box::new(SystemClipboard::new()),
        };
        Self { writer }
    }

    pub(crate) fn execute(&mut self, effect: &CopyEffect) -> CopyResult {
        match self.writer.write(effect.text()) {
            Ok(()) => CopyResult::Written,
            Err(_) => CopyResult::Failed,
        }
    }
}
