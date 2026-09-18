use std::time::{Duration, Instant};

use crate::app::plan::{AttributeChangeKind, AttributeDiff, AttributeDiffs, AttributePathSegment};

use super::PlanListState;

const REVEAL_DURATION: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailAction {
    SelectPrevious,
    SelectNext,
    ToggleExpansion,
    PageUp,
    PageDown,
    Reveal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceNavigation {
    Previous,
    Next,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttributeGroup {
    Unchanged,
    Nested {
        kind: AttributeChangeKind,
        path: Vec<AttributePathSegment>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DetailRow {
    Attribute(usize),
    Group { group: AttributeGroup, count: usize },
}

impl DetailRow {
    #[must_use]
    pub(crate) const fn group(&self) -> Option<&AttributeGroup> {
        match self {
            Self::Attribute(_) => None,
            Self::Group { group, .. } => Some(group),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SensitiveReveal {
    pub(crate) path: Vec<AttributePathSegment>,
    pub(crate) expires_at: Instant,
}

impl SensitiveReveal {
    #[must_use]
    pub(crate) const fn expires_at(&self) -> Instant {
        self.expires_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewDetailState {
    index: usize,
    total: usize,
    attributes: AttributeDiffs,
    selected: usize,
    expanded_groups: Vec<AttributeGroup>,
    reveal: Option<SensitiveReveal>,
}

impl ReviewDetailState {
    #[must_use]
    pub(crate) fn from_list(list: &PlanListState) -> Option<Self> {
        Some(Self {
            index: list.selected()?,
            total: list.visible_count(),
            attributes: list.selected_item()?.attribute_diffs(),
            selected: 0,
            expanded_groups: Vec::new(),
            reveal: None,
        })
    }

    pub(crate) fn navigate(&mut self, navigation: ResourceNavigation, list: &mut PlanListState) {
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

        list.select_resource(target);
        if let Some(next) = Self::from_list(list) {
            *self = next;
        }
    }

    pub(crate) fn apply(&mut self, action: DetailAction, now: Instant) {
        self.clear_expired_reveal(now);
        match action {
            DetailAction::SelectPrevious => {
                let previous = self.selected;
                self.selected = self.selected.saturating_sub(1);
                self.clear_reveal_on_selection_change(previous);
            }
            DetailAction::SelectNext => {
                let previous = self.selected;
                if let Some(last) = self.rows().len().checked_sub(1) {
                    self.selected = (self.selected + 1).min(last);
                    self.clear_reveal_on_selection_change(previous);
                }
            }
            DetailAction::ToggleExpansion => {
                let group = self
                    .rows()
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
                }
            }
            DetailAction::Reveal => self.toggle_reveal(now),
            DetailAction::PageUp | DetailAction::PageDown => {}
        }
    }

    pub(crate) fn mask(&mut self) {
        self.reveal = None;
    }

    pub(crate) fn clear_expired(&mut self, now: Instant) {
        self.clear_expired_reveal(now);
    }

    #[must_use]
    pub(crate) const fn index(&self) -> usize {
        self.index
    }

    #[must_use]
    pub(crate) const fn total(&self) -> usize {
        self.total
    }

    #[must_use]
    pub(crate) const fn selected(&self) -> usize {
        self.selected
    }

    #[must_use]
    pub(crate) const fn attributes(&self) -> &AttributeDiffs {
        &self.attributes
    }

    #[must_use]
    pub(crate) fn rows(&self) -> Vec<DetailRow> {
        detail_rows(&self.attributes, &self.expanded_groups)
    }

    #[must_use]
    pub(crate) fn expanded_groups(&self) -> &[AttributeGroup] {
        &self.expanded_groups
    }

    #[must_use]
    pub(crate) const fn reveal(&self) -> Option<&SensitiveReveal> {
        self.reveal.as_ref()
    }

    #[must_use]
    pub(crate) fn is_revealed_at(&self, now: Instant) -> bool {
        self.reveal
            .as_ref()
            .is_some_and(|reveal| now < reveal.expires_at)
    }

    #[must_use]
    pub(crate) fn reveals_attribute(&self, attribute: &AttributeDiff, now: Instant) -> bool {
        self.reveal.as_ref().is_some_and(|reveal| {
            now < reveal.expires_at
                && attribute.path.starts_with(&reveal.path)
                && (attribute.before.is_revealable() || attribute.after.is_revealable())
        })
    }

    #[must_use]
    pub(crate) fn can_reveal_selected(&self) -> bool {
        self.selected_reveal_path().is_some()
    }

    fn clear_reveal_on_selection_change(&mut self, previous: usize) {
        if self.selected != previous {
            self.mask();
        }
    }

    fn clear_expired_reveal(&mut self, now: Instant) {
        if self
            .reveal
            .as_ref()
            .is_some_and(|reveal| now >= reveal.expires_at)
        {
            self.mask();
        }
    }

    fn toggle_reveal(&mut self, now: Instant) {
        if self.reveal.is_some() {
            self.mask();
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
        match self.rows().get(self.selected)? {
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
}

fn detail_rows(attributes: &AttributeDiffs, expanded_groups: &[AttributeGroup]) -> Vec<DetailRow> {
    let mut rows = Vec::new();
    append_attribute_rows(
        &mut rows,
        &attributes.attributes,
        AttributeChangeKind::Changed,
        &[],
        expanded_groups,
    );

    if attributes.unchanged_count > 0 {
        let group = AttributeGroup::Unchanged;
        rows.push(DetailRow::Group {
            group: group.clone(),
            count: attributes.unchanged_count,
        });
        if expanded_groups.contains(&group) {
            append_attribute_rows(
                &mut rows,
                &attributes.attributes,
                AttributeChangeKind::Unchanged,
                &[],
                expanded_groups,
            );
        }
    }
    rows
}

fn append_attribute_rows(
    rows: &mut Vec<DetailRow>,
    attributes: &[AttributeDiff],
    kind: AttributeChangeKind,
    parent: &[AttributePathSegment],
    expanded_groups: &[AttributeGroup],
) {
    let mut items = Vec::new();
    for (index, attribute) in attributes.iter().enumerate() {
        if attribute.kind != kind
            || attribute.path.len() <= parent.len()
            || !attribute.path.starts_with(parent)
        {
            continue;
        }

        let segment = &attribute.path[parent.len()];
        let item = if attribute.path.len() == parent.len() + 1 {
            AttributeItem::Attribute(index)
        } else {
            let mut path = parent.to_vec();
            path.push(segment.clone());
            AttributeItem::Group(path)
        };
        if !items.contains(&item) {
            items.push(item);
        }
    }

    for item in items {
        match item {
            AttributeItem::Attribute(index) => rows.push(DetailRow::Attribute(index)),
            AttributeItem::Group(path) => {
                let group = AttributeGroup::Nested {
                    kind,
                    path: path.clone(),
                };
                let count = attributes
                    .iter()
                    .filter(|attribute| attribute.kind == kind && attribute.path.starts_with(&path))
                    .count();
                rows.push(DetailRow::Group {
                    group: group.clone(),
                    count,
                });
                if expanded_groups.contains(&group) {
                    append_attribute_rows(rows, attributes, kind, &path, expanded_groups);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AttributeItem {
    Attribute(usize),
    Group(Vec<AttributePathSegment>),
}
