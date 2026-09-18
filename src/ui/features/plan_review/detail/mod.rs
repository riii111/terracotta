use std::time::{Duration, Instant};

use crate::app::attribution::SourceFileAnalysis;
#[cfg(test)]
use crate::app::copy::CopyEffect;
use crate::app::copy::{CopyNotice, CopyTarget};
use crate::app::plan::{AttributeDiff, AttributeDiffs, AttributePathSegment};
#[cfg(test)]
use crate::app::review::PlanListAction;
#[cfg(test)]
use crate::app::review::ResourceNavigation;
use crate::app::review::{
    AttributeGroup, DetailAction, DetailRow, PlanListContext, PlanListItem, PlanListState,
    ReviewDetailState, SensitiveReveal,
};

mod input;
mod render;
mod rows;
mod viewport;

pub(crate) use input::{DetailInput, key_to_input};
pub(crate) use render::render_resource_detail;
use rows::{detail_content, detail_rows};
use viewport::{max_scroll, wrapped_selected_line};

const MIN_HEIGHT: u16 = 8;
const MIN_WIDTH: u16 = 48;
const REVEAL_DURATION: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceDetailState {
    context: Option<PlanListContext>,
    comparison: String,
    item: PlanListItem,
    attributes: AttributeDiffs,
    source_files: Vec<SourceFileAnalysis>,
    index: usize,
    total: usize,
    plan_resource_count: usize,
    selected: usize,
    scroll: u16,
    expanded_groups: Vec<AttributeGroup>,
    reveal: Option<SensitiveReveal>,
    resource_copy_text: Option<String>,
    plan_copy_text: Option<String>,
    copy_notice: Option<CopyNotice>,
}

impl ResourceDetailState {
    pub(super) fn from_list(state: &PlanListState) -> Option<Self> {
        let item = state.selected_item()?.clone();
        let resource_copy_text = state
            .copy_effect(CopyTarget::Resource)
            .map(|effect| effect.text().to_owned());
        let plan_copy_text = state
            .copy_effect(CopyTarget::Plan)
            .map(|effect| effect.text().to_owned());
        Some(Self {
            context: state.context().cloned(),
            comparison: state.comparison().to_owned(),
            attributes: item.attribute_diffs(),
            source_files: state.source_files().to_vec(),
            item,
            index: state.selected()?,
            total: state.visible_count(),
            plan_resource_count: state.items().len(),
            selected: 0,
            scroll: 0,
            expanded_groups: Vec::new(),
            reveal: None,
            resource_copy_text,
            plan_copy_text,
            copy_notice: None,
        })
    }

    pub(crate) fn from_session(
        list: &PlanListState,
        detail: &ReviewDetailState,
        copy_notice: Option<CopyNotice>,
        scroll: u16,
    ) -> Option<Self> {
        let mut state = Self::from_list(list)?;
        state.selected = detail.selected();
        state.expanded_groups = detail.expanded_groups().to_vec();
        state.reveal = detail.reveal().cloned();
        state.copy_notice = copy_notice;
        state.scroll = scroll;
        state.attributes = detail.attributes().clone();
        Some(state)
    }

    #[cfg(test)]
    pub(super) fn navigate(&mut self, navigation: ResourceNavigation, list: &mut PlanListState) {
        let target = match navigation {
            ResourceNavigation::Previous => self.index.checked_sub(1),
            ResourceNavigation::Next => self
                .index
                .checked_add(1)
                .filter(|index| *index < self.total),
        };
        let Some(target) = target else {
            return;
        };

        list.apply(PlanListAction::SelectResource(target));
        if let Some(next) = Self::from_list(list) {
            *self = next;
        }
    }

    pub(crate) fn apply_at(
        &mut self,
        action: DetailAction,
        viewport_width: u16,
        viewport_height: u16,
        now: Instant,
    ) {
        self.clear_expired_reveal(now);
        let page = viewport_height.max(1);
        match action {
            DetailAction::SelectPrevious => {
                let previous = self.selected;
                self.selected = self.selected.saturating_sub(1);
                self.clear_reveal_on_selection_change(previous);
                self.ensure_selected_visible(viewport_width, page, now);
            }
            DetailAction::SelectNext => {
                let previous = self.selected;
                if let Some(last) = detail_rows(self).len().checked_sub(1) {
                    self.selected = (self.selected + 1).min(last);
                    self.clear_reveal_on_selection_change(previous);
                    self.ensure_selected_visible(viewport_width, page, now);
                }
            }
            DetailAction::ToggleExpansion => {
                let group = detail_rows(self)
                    .get(self.selected)
                    .and_then(DetailRow::group)
                    .cloned();
                if let Some(group) = group {
                    if let Some(index) = self.expanded_groups.iter().position(|item| *item == group)
                    {
                        self.expanded_groups.remove(index);
                    } else {
                        self.expanded_groups.push(group);
                    }
                    self.ensure_selected_visible(viewport_width, page, now);
                }
            }
            DetailAction::PageUp => self.scroll = self.scroll.saturating_sub(page),
            DetailAction::PageDown => {
                self.scroll = self.scroll.saturating_add(page).min(max_scroll(
                    self,
                    viewport_width,
                    page,
                    now,
                ));
            }
            DetailAction::Reveal => self.toggle_reveal(now),
        }
    }

    pub(crate) const fn scroll(&self) -> u16 {
        self.scroll
    }

    pub(crate) const fn reset_scroll(&mut self) {
        self.scroll = 0;
    }

    pub(super) const fn item_index(&self) -> usize {
        self.index
    }

    pub(super) const fn total_items(&self) -> usize {
        self.total
    }

    #[must_use]
    #[cfg(test)]
    pub(super) fn copy_effect(&self, target: CopyTarget) -> Option<CopyEffect> {
        let text = match target {
            CopyTarget::Resource => self.resource_copy_text.clone()?,
            CopyTarget::Plan => self.plan_copy_text.clone()?,
            CopyTarget::Diagnostic | CopyTarget::Result => return None,
        };
        let resource_count = match target {
            CopyTarget::Resource => self.total,
            CopyTarget::Plan => self.plan_resource_count,
            CopyTarget::Diagnostic | CopyTarget::Result => return None,
        };
        Some(CopyEffect::new(target, resource_count, text))
    }

    #[must_use]
    pub(super) const fn copy_notice(&self) -> Option<CopyNotice> {
        self.copy_notice
    }

    #[cfg(test)]
    pub(super) const fn set_copy_notice(&mut self, notice: CopyNotice) {
        self.copy_notice = Some(notice);
    }

    #[cfg(test)]
    pub(super) fn viewport_height_at(&self, total_height: u16, now: Instant) -> u16 {
        total_height.saturating_sub(
            5 + u16::from(self.context.is_some()) * 2
                + u16::from(self.is_revealed_at(now))
                + u16::from(self.copy_notice.is_some()),
        )
    }

    fn ensure_selected_visible(&mut self, viewport_width: u16, viewport_height: u16, now: Instant) {
        let content = detail_content(self, now);
        let Some(selected_line) = wrapped_selected_line(&content, viewport_width) else {
            return;
        };
        let selected_line = u16::try_from(selected_line).unwrap_or(u16::MAX);
        if selected_line < self.scroll {
            self.scroll = selected_line;
        } else if selected_line >= self.scroll.saturating_add(viewport_height) {
            self.scroll = selected_line.saturating_sub(viewport_height.saturating_sub(1));
        }
    }

    fn clear_reveal_on_selection_change(&mut self, previous: usize) {
        if self.selected != previous {
            self.reveal = None;
        }
    }

    fn clear_expired_reveal(&mut self, now: Instant) {
        if self
            .reveal
            .as_ref()
            .is_some_and(|reveal| now >= reveal.expires_at)
        {
            self.reveal = None;
        }
    }

    fn toggle_reveal(&mut self, now: Instant) {
        if self.reveal.is_some() {
            self.reveal = None;
            return;
        }

        let Some(path) = self.selected_reveal_path() else {
            return;
        };
        self.reveal = Some(SensitiveReveal {
            path,
            expires_at: now + REVEAL_DURATION,
        });
    }

    fn selected_reveal_path(&self) -> Option<Vec<AttributePathSegment>> {
        let rows = detail_rows(self);
        match rows.get(self.selected)? {
            DetailRow::Attribute(index) => {
                let attribute = self.attributes.attributes.get(*index)?;
                (attribute.before.is_revealable() || attribute.after.is_revealable())
                    .then(|| attribute.path.clone())
            }
            DetailRow::Group {
                group: AttributeGroup::Nested { kind, path },
                ..
            } => self
                .attributes
                .attributes
                .iter()
                .any(|attribute| {
                    attribute.kind == *kind
                        && attribute.path.starts_with(path)
                        && (attribute.before.is_revealable() || attribute.after.is_revealable())
                })
                .then(|| path.clone()),
            DetailRow::Group { .. } => None,
        }
    }

    fn is_revealed_at(&self, now: Instant) -> bool {
        self.reveal
            .as_ref()
            .is_some_and(|reveal| now < reveal.expires_at)
    }

    fn reveals_attribute(&self, attribute: &AttributeDiff, now: Instant) -> bool {
        self.reveal.as_ref().is_some_and(|reveal| {
            now < reveal.expires_at
                && attribute.path.starts_with(&reveal.path)
                && (attribute.before.is_revealable() || attribute.after.is_revealable())
        })
    }

    fn can_reveal_selected(&self) -> bool {
        self.selected_reveal_path().is_some()
    }

    fn mask_reveal(&mut self) {
        self.reveal = None;
    }
}
