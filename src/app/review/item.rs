use crate::app::attribution::ResourceAttribution;
use crate::app::copy::{CopyEffect, CopyTarget};
use crate::app::plan::{
    AttributeDiffs, ResourceChange, ResourceChangeKind, diff_resource_attributes,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanListItem {
    change: ResourceChange,
    attribution: ResourceAttribution,
    resource_copy_text: Option<String>,
}

impl PlanListItem {
    pub(super) const fn new(change: ResourceChange, attribution: ResourceAttribution) -> Self {
        Self {
            change,
            attribution,
            resource_copy_text: None,
        }
    }

    pub(super) fn set_resource_copy_text(&mut self, text: String) {
        self.resource_copy_text = Some(text);
    }

    pub(super) const fn change(&self) -> &ResourceChange {
        &self.change
    }

    #[must_use]
    pub(crate) fn address(&self) -> &str {
        &self.change.address
    }

    #[must_use]
    pub(crate) const fn kind(&self) -> ResourceChangeKind {
        self.change.kind
    }

    #[must_use]
    pub(crate) const fn needs_review(&self) -> bool {
        self.attribution.needs_review()
    }

    #[must_use]
    pub(crate) const fn attribution(&self) -> &ResourceAttribution {
        &self.attribution
    }

    #[must_use]
    pub(super) const fn can_copy(&self) -> bool {
        self.resource_copy_text.is_some()
    }

    #[must_use]
    pub(crate) fn attribute_diffs(&self) -> AttributeDiffs {
        diff_resource_attributes(&self.change)
    }

    #[must_use]
    pub(super) fn copy_effect(
        &self,
        target: CopyTarget,
        resource_count: usize,
    ) -> Option<CopyEffect> {
        Some(CopyEffect::new(
            target,
            resource_count,
            self.resource_copy_text.clone()?,
        ))
    }
}
