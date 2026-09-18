use crate::app::execution::Diagnostic;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ReviewDiagnosticsState {
    diagnostics: Vec<Diagnostic>,
    open: bool,
}

impl ReviewDiagnosticsState {
    pub(crate) const fn new(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            diagnostics,
            open: false,
        }
    }

    #[must_use]
    pub(crate) fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub(crate) const fn count(&self) -> usize {
        self.diagnostics.len()
    }

    #[must_use]
    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) const fn open(&mut self) {
        if !self.diagnostics.is_empty() {
            self.open = true;
        }
    }

    pub(crate) const fn close(&mut self) {
        self.open = false;
    }
}
