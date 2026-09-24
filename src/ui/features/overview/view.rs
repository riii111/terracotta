use std::collections::BTreeSet;

use ratatui::layout::Rect;

use crate::app::{
    plan::{PlanAction, ResourceChange, ResourceChangeKind},
    review::PlanReview,
};
use crate::ui::text_input;

use super::OverviewInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverviewOverlay {
    Help,
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OverviewRow {
    pub(crate) group_index: usize,
    pub(crate) member_index: Option<usize>,
    pub(crate) child: bool,
    pub(crate) address: String,
    pub(crate) display_address: String,
    pub(crate) action: String,
    pub(crate) count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OverviewContent {
    pub(crate) rows: Vec<OverviewRow>,
    pub(crate) repeated: usize,
    pub(crate) unsupported: usize,
}

impl OverviewContent {
    pub(crate) fn from_review(
        review: &PlanReview,
        query: &str,
        expanded: &BTreeSet<usize>,
    ) -> Self {
        let grouping = review.plan().grouped_changes(review.provider_schemas());
        let mut rows = Vec::new();
        for (group_index, group) in grouping.groups.iter().enumerate() {
            let matching = group
                .members
                .iter()
                .enumerate()
                .filter(|(_, member)| {
                    member.kind != ResourceChangeKind::NoOp
                        && (query.is_empty() || member.address.contains(query))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                continue;
            }
            let first = matching[0];
            if group.is_repeated() && matching.len() > 1 {
                rows.push(OverviewRow {
                    group_index,
                    member_index: None,
                    child: false,
                    address: group.members[first].address.clone(),
                    display_address: group.display_address.clone(),
                    action: action_text(&group.members[0]),
                    count: matching.len(),
                });
                if expanded.contains(&group_index) {
                    rows.extend(matching.into_iter().map(|member_index| OverviewRow {
                        group_index,
                        member_index: Some(member_index),
                        child: true,
                        address: group.members[member_index].address.clone(),
                        display_address: group.members[member_index].address.clone(),
                        action: action_text(&group.members[member_index]),
                        count: 1,
                    }));
                }
            } else {
                rows.extend(matching.into_iter().map(|member_index| OverviewRow {
                    group_index,
                    member_index: Some(member_index),
                    child: false,
                    address: group.members[member_index].address.clone(),
                    display_address: group.members[member_index].address.clone(),
                    action: action_text(&group.members[member_index]),
                    count: 1,
                }));
            }
        }
        Self {
            rows,
            repeated: grouping.repeated,
            unsupported: review.metadata().nonstandard_changes(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchState {
    query: String,
    cursor: usize,
    previous_query: String,
    previous_vertical: u16,
    previous_selected: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OverviewViewState {
    vertical: u16,
    selected: Option<usize>,
    expanded: BTreeSet<usize>,
    search: Option<SearchState>,
    filter: String,
    overlay: Option<OverviewOverlay>,
    overlay_scroll: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverviewCommand {
    Open(Option<String>),
    ViewPlan,
    Back,
    Copy,
    Quit,
}

impl OverviewViewState {
    pub(crate) fn apply(
        &mut self,
        input: OverviewInput,
        body: Rect,
        max_vertical: u16,
        content: &OverviewContent,
    ) -> Option<OverviewCommand> {
        self.vertical = self.vertical.min(max_vertical);
        if self.search.is_some() {
            return self.apply_search(input, content.rows.len());
        }
        match input {
            OverviewInput::Up => self.move_selection(-1, body, max_vertical, content),
            OverviewInput::Down => self.move_selection(1, body, max_vertical, content),
            OverviewInput::PageUp => {
                self.vertical = self.vertical.saturating_sub(body.height.max(1));
                None
            }
            OverviewInput::PageDown => {
                self.vertical = self
                    .vertical
                    .saturating_add(body.height.max(1))
                    .min(max_vertical);
                None
            }
            OverviewInput::Top => {
                self.vertical = 0;
                self.selected = content.rows.first().map(|_| 0);
                None
            }
            OverviewInput::Bottom => {
                self.vertical = max_vertical;
                self.selected = content.rows.len().checked_sub(1);
                None
            }
            OverviewInput::ToggleExpand => {
                if let Some(group_index) = self.selected_group_index(content)
                    && !self.expanded.remove(&group_index)
                {
                    self.expanded.insert(group_index);
                }
                None
            }
            OverviewInput::Open => Some(OverviewCommand::Open(self.selected_address(content))),
            OverviewInput::ViewPlan => Some(OverviewCommand::ViewPlan),
            OverviewInput::Back => Some(OverviewCommand::Back),
            OverviewInput::SearchStart => {
                let query = self.filter.clone();
                self.search = Some(SearchState {
                    cursor: text_input::last_grapheme_boundary(&query),
                    previous_query: query.clone(),
                    query,
                    previous_vertical: self.vertical,
                    previous_selected: self.selected,
                });
                self.selected = None;
                self.vertical = 0;
                None
            }
            OverviewInput::SearchCancel => {
                self.filter.clear();
                self.selected = None;
                self.vertical = 0;
                None
            }
            OverviewInput::OpenHelp => {
                self.overlay = Some(OverviewOverlay::Help);
                self.overlay_scroll = 0;
                None
            }
            OverviewInput::OpenContext => {
                self.overlay = Some(OverviewOverlay::Context);
                self.overlay_scroll = 0;
                None
            }
            OverviewInput::Copy => Some(OverviewCommand::Copy),
            OverviewInput::Quit => Some(OverviewCommand::Quit),
            OverviewInput::SearchChar(_)
            | OverviewInput::SearchBackspace
            | OverviewInput::Left
            | OverviewInput::Right
            | OverviewInput::SearchLeft
            | OverviewInput::SearchRight
            | OverviewInput::SearchHome
            | OverviewInput::SearchEnd
            | OverviewInput::SearchConfirm => None,
        }
    }

    fn apply_search(&mut self, input: OverviewInput, row_count: usize) -> Option<OverviewCommand> {
        let search = self.search.as_mut()?;
        match input {
            OverviewInput::SearchConfirm => {
                let search = self.search.take().expect("search state should exist");
                self.filter = search.query;
                self.selected = (row_count > 0).then_some(0);
                self.vertical = 0;
            }
            OverviewInput::SearchCancel => {
                let search = self.search.take().expect("search state should exist");
                self.filter = search.previous_query;
                self.vertical = search.previous_vertical;
                self.selected = search.previous_selected;
            }
            OverviewInput::SearchChar(character) => {
                search.query.insert(search.cursor, character);
                search.cursor = text_input::next_grapheme_boundary_at_or_after(
                    &search.query,
                    search.cursor + character.len_utf8(),
                );
                self.filter = search.query.clone();
                self.selected = (row_count > 0).then_some(0);
                self.vertical = 0;
            }
            OverviewInput::SearchBackspace => {
                if search.cursor > 0 {
                    let start =
                        text_input::previous_grapheme_boundary(&search.query, search.cursor);
                    search.query.drain(start..search.cursor);
                    search.cursor = start;
                    self.filter = search.query.clone();
                    self.selected = (row_count > 0).then_some(0);
                    self.vertical = 0;
                }
            }
            OverviewInput::SearchLeft => {
                search.cursor =
                    text_input::previous_grapheme_boundary(&search.query, search.cursor);
            }
            OverviewInput::SearchRight => {
                search.cursor = text_input::next_grapheme_boundary(&search.query, search.cursor);
            }
            OverviewInput::SearchHome => search.cursor = 0,
            OverviewInput::SearchEnd => search.cursor = search.query.len(),
            _ => {}
        }
        None
    }

    fn move_selection(
        &mut self,
        direction: i8,
        body: Rect,
        max_vertical: u16,
        content: &OverviewContent,
    ) -> Option<OverviewCommand> {
        let row_count = content.rows.len();
        if row_count == 0 {
            return None;
        }
        let next = match (self.selected, direction.is_negative()) {
            (Some(selected), false) => selected.saturating_add(1).min(row_count - 1),
            (Some(selected), true) => selected.saturating_sub(1),
            (None, false) => 0,
            (None, true) => row_count - 1,
        };
        self.selected = Some(next);
        let row_line = 1 + usize::from(content.unsupported > 0) + next;
        let bottom = usize::from(self.vertical) + usize::from(body.height.max(1));
        if row_line < usize::from(self.vertical) {
            self.vertical = u16::try_from(row_line).unwrap_or(u16::MAX);
        } else if row_line >= bottom {
            self.vertical = u16::try_from(row_line + 1 - usize::from(body.height.max(1)))
                .unwrap_or(u16::MAX)
                .min(max_vertical);
        }
        None
    }

    fn selected_address(&self, content: &OverviewContent) -> Option<String> {
        self.selected
            .and_then(|index| content.rows.get(index))
            .map(|row| row.address.clone())
            .or_else(|| content.rows.first().map(|row| row.address.clone()))
    }

    pub(crate) fn filter(&self) -> &str {
        &self.filter
    }

    pub(crate) const fn searching(&self) -> bool {
        self.search.is_some()
    }

    pub(crate) fn search_query(&self) -> Option<&str> {
        self.search.as_ref().map(|search| search.query.as_str())
    }

    pub(crate) const fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn selected_group_expanded(&self, content: &OverviewContent) -> Option<bool> {
        let group_index = self.selected_group_index(content)?;
        Some(self.expanded.contains(&group_index))
    }

    fn selected_group_index(&self, content: &OverviewContent) -> Option<usize> {
        self.selected
            .and_then(|index| content.rows.get(index))
            .filter(|row| row.member_index.is_none() && row.count > 1)
            .map(|row| row.group_index)
    }

    pub(crate) const fn scroll(&self) -> u16 {
        self.vertical
    }

    pub(crate) const fn expanded(&self) -> &BTreeSet<usize> {
        &self.expanded
    }

    pub(crate) const fn overlay(&self) -> Option<OverviewOverlay> {
        self.overlay
    }

    pub(crate) const fn overlay_scroll(&self) -> u16 {
        self.overlay_scroll
    }

    pub(crate) const fn scroll_overlay(&mut self, delta: i16) {
        if delta.is_negative() {
            self.overlay_scroll = self.overlay_scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.overlay_scroll = self.overlay_scroll.saturating_add(delta.cast_unsigned());
        }
    }

    pub(crate) const fn overlay_top(&mut self) {
        self.overlay_scroll = 0;
    }

    pub(crate) const fn overlay_bottom(&mut self) {
        self.overlay_scroll = u16::MAX;
    }

    pub(crate) const fn close_overlay(&mut self) {
        self.overlay = None;
    }

    pub(crate) fn reconcile(&mut self, max_vertical: u16, row_count: usize) {
        self.vertical = self.vertical.min(max_vertical);
        if self.selected.is_none() && row_count > 0 {
            self.selected = Some(0);
        }
        if self.selected.is_some_and(|selected| selected >= row_count) {
            self.selected = row_count.checked_sub(1);
        }
    }
}

fn action_text(change: &ResourceChange) -> String {
    let symbol = match change.kind {
        ResourceChangeKind::Create => "+",
        ResourceChangeKind::Update => "~",
        ResourceChangeKind::Delete => "-",
        ResourceChangeKind::Replace => {
            if change
                .actions
                .starts_with(&[PlanAction::Create, PlanAction::Delete])
            {
                "+/-"
            } else {
                "-/+"
            }
        }
        ResourceChangeKind::Read => "read",
        ResourceChangeKind::Move => "move",
        ResourceChangeKind::Import => "import",
        ResourceChangeKind::Unknown
        | ResourceChangeKind::Unsupported
        | ResourceChangeKind::NoOp => "?",
    };
    symbol.to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::app::plan::{Plan, PlanSummary, PlanValue, ResourceMode};
    use crate::app::review::{PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata};

    fn apply_search(view: &mut OverviewViewState, input: OverviewInput, content: &OverviewContent) {
        view.apply(input, Rect::new(0, 0, 40, 5), 0, content);
    }

    fn review() -> PlanReview {
        let mut review = PlanReview::new(
            PathBuf::from("/project"),
            "default".to_owned(),
            PlanDocument::with_blocks_and_line_kinds(
                "plan\n".to_owned(),
                vec![PlanBlock::new(0..1, PlanBlockKind::Common)],
                vec![],
            ),
            PlanMetadata::new(Vec::new(), Vec::new(), 1, 0, 0, true),
            Vec::new(),
        );
        let change = |address: &str| ResourceChange {
            address: address.to_owned(),
            provider: None,
            resource_type: None,
            resource_name: None,
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: Some(PlanValue::Object(BTreeMap::from([(
                "value".to_owned(),
                PlanValue::String("old".to_owned()),
            )]))),
            after: Some(PlanValue::Object(BTreeMap::from([(
                "value".to_owned(),
                PlanValue::String("new".to_owned()),
            )]))),
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
            previous_address: None,
            importing: None,
        };
        let no_op = ResourceChange {
            address: "terraform_data.unchanged".to_owned(),
            provider: None,
            resource_type: None,
            resource_name: None,
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::NoOp],
            kind: ResourceChangeKind::NoOp,
            before: None,
            after: None,
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
            previous_address: None,
            importing: None,
        };
        review = review.with_plan(Plan {
            value_addresses: BTreeSet::new(),
            resource_changes: vec![
                change("aws_instance.web[0]"),
                change("aws_instance.web[1]"),
                no_op,
            ],
            summary: PlanSummary {
                creates: 0,
                updates: 2,
                replaces: 0,
                deletes: 0,
            },
            unsupported_changes: Vec::new(),
            output_changes: Vec::new(),
        });
        review
    }

    #[test]
    fn filters_group_members_without_changing_repeated_count() {
        let content = OverviewContent::from_review(&review(), "[1]", &BTreeSet::new());

        assert_eq!(content.repeated, 2);
        assert_eq!(content.rows.len(), 1);
        assert_eq!(content.rows[0].count, 1);
        assert_eq!(content.rows[0].address, "aws_instance.web[1]");
    }

    #[test]
    fn selection_scroll_accounts_for_overview_header_and_notice() {
        let content = OverviewContent {
            rows: (0..6)
                .map(|index| OverviewRow {
                    group_index: index,
                    member_index: Some(index),
                    child: false,
                    address: format!("resource.{index}"),
                    display_address: format!("resource.{index}"),
                    action: "~".to_owned(),
                    count: 1,
                })
                .collect(),
            repeated: 0,
            unsupported: 1,
        };
        let mut view = OverviewViewState::default();
        let body = Rect::new(0, 0, 40, 5);

        for _ in 0..6 {
            view.apply(OverviewInput::Down, body, 3, &content);
        }

        assert_eq!(view.selected(), Some(5));
        assert_eq!(view.scroll(), 3);
    }

    #[test]
    fn search_cursor_edits_graphemes_and_cancel_restores_the_confirmed_filter() {
        let content = OverviewContent {
            rows: Vec::new(),
            repeated: 0,
            unsupported: 0,
        };
        let mut view = OverviewViewState::default();

        apply_search(&mut view, OverviewInput::SearchStart, &content);
        for character in "aあe\u{301}👩💻".chars() {
            apply_search(&mut view, OverviewInput::SearchChar(character), &content);
        }
        apply_search(&mut view, OverviewInput::SearchHome, &content);
        apply_search(&mut view, OverviewInput::SearchRight, &content);
        apply_search(&mut view, OverviewInput::SearchRight, &content);
        apply_search(&mut view, OverviewInput::SearchBackspace, &content);
        assert_eq!(view.search_query(), Some("ae\u{301}👩💻"));

        apply_search(&mut view, OverviewInput::SearchEnd, &content);
        apply_search(&mut view, OverviewInput::SearchLeft, &content);
        apply_search(&mut view, OverviewInput::SearchChar('\u{200d}'), &content);
        apply_search(&mut view, OverviewInput::SearchChar('x'), &content);
        assert_eq!(view.search_query(), Some("ae\u{301}👩\u{200d}💻x"));

        apply_search(&mut view, OverviewInput::SearchBackspace, &content);
        apply_search(&mut view, OverviewInput::SearchBackspace, &content);
        apply_search(&mut view, OverviewInput::SearchBackspace, &content);
        assert_eq!(view.search_query(), Some("a"));

        apply_search(&mut view, OverviewInput::SearchHome, &content);
        apply_search(&mut view, OverviewInput::SearchChar('X'), &content);
        apply_search(&mut view, OverviewInput::SearchEnd, &content);
        apply_search(&mut view, OverviewInput::SearchChar('Y'), &content);
        apply_search(&mut view, OverviewInput::SearchConfirm, &content);
        assert_eq!(view.filter(), "XaY");
        assert!(!view.searching());

        apply_search(&mut view, OverviewInput::SearchStart, &content);
        apply_search(&mut view, OverviewInput::SearchChar('Z'), &content);
        apply_search(&mut view, OverviewInput::SearchCancel, &content);
        assert_eq!(view.filter(), "XaY");
    }
}
