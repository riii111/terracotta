#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyTarget {
    Resource,
    Plan,
    Diagnostic,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyNotice {
    Copied {
        target: CopyTarget,
        resource_count: usize,
    },
    Failed,
}

impl CopyNotice {
    #[must_use]
    pub(crate) fn message(self) -> String {
        match self {
            Self::Copied {
                target: CopyTarget::Resource,
                ..
            } => "Copied selected resource (redacted).".to_owned(),
            Self::Copied {
                target: CopyTarget::Plan,
                resource_count,
            } => format!("Copied {resource_count} resources (redacted)."),
            Self::Copied {
                target: CopyTarget::Diagnostic,
                ..
            } => "Copied diagnostic (redacted).".to_owned(),
            Self::Copied {
                target: CopyTarget::Result,
                ..
            } => "Copied result (redacted).".to_owned(),
            Self::Failed => "Copy failed: clipboard unavailable.".to_owned(),
        }
    }
}

pub(crate) struct CopyEffect {
    target: CopyTarget,
    resource_count: usize,
    text: String,
}

impl CopyEffect {
    #[must_use]
    pub(crate) const fn new(target: CopyTarget, resource_count: usize, text: String) -> Self {
        Self {
            target,
            resource_count,
            text,
        }
    }

    #[must_use]
    pub(crate) const fn target(&self) -> CopyTarget {
        self.target
    }

    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub(crate) const fn success_notice(&self) -> CopyNotice {
        CopyNotice::Copied {
            target: self.target,
            resource_count: self.resource_count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyResult {
    Written,
    Failed,
}
