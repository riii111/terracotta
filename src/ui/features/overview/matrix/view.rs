use std::collections::BTreeSet;

use crate::app::environments::{
    EnvironmentSession,
    comparison::{CellState, ComparisonRow, DifferenceReason, SourceReference},
    overview::{EnvironmentOverview, GroupId, OverviewRow},
};
use crate::ui::features::overview::OverviewInput;
use crate::ui::text_input;

#[derive(Clone)]
pub(crate) struct MatrixCell {
    pub(crate) state: CellState,
    pub(crate) source: Option<SourceReference>,
    pub(crate) members: Vec<String>,
}

pub(super) struct Row {
    pub(super) address: String,
    pub(super) group: Option<GroupId>,
    pub(super) child: bool,
    pub(super) cells: Vec<MatrixCell>,
    pub(super) difference: Option<DifferenceReason>,
}

#[derive(Clone)]
struct Anchor {
    group: Option<GroupId>,
    address: String,
}

struct Search {
    previous: String,
    anchor: Option<Anchor>,
    cursor: usize,
}

#[derive(Default)]
pub(crate) struct MatrixView {
    pub(super) rows: Vec<Row>,
    pub(super) selected: usize,
    pub(super) vertical: usize,
    pub(super) first_column: usize,
    pub(super) expanded: BTreeSet<GroupId>,
    pub(super) filter: String,
    search: Option<Search>,
    revision: Option<u64>,
}

impl MatrixView {
    pub(crate) fn sync(&mut self, state: &EnvironmentSession, environment: usize) {
        if self.revision != Some(state.revision()) {
            let anchor = self.anchor(environment);
            self.rebuild(state.overview(), anchor.as_ref());
            self.revision = Some(state.revision());
        }
    }

    pub(crate) const fn searching(&self) -> bool {
        self.search.is_some()
    }

    pub(crate) fn filter(&self) -> &str {
        &self.filter
    }

    pub(crate) const fn filtered(&self) -> bool {
        !self.filter.is_empty()
    }

    pub(crate) fn selected_group_expanded(&self) -> Option<bool> {
        let group = self.rows.get(self.selected)?.group.as_ref()?;
        Some(self.expanded.contains(group))
    }

    pub(crate) fn cell(&self, environment: usize) -> Option<&MatrixCell> {
        self.rows.get(self.selected)?.cells.get(environment)
    }

    pub(crate) fn selected_address(&self) -> Option<&str> {
        self.rows.get(self.selected).map(|row| row.address.as_str())
    }

    pub(crate) fn selected_is_group(&self) -> bool {
        self.rows
            .get(self.selected)
            .is_some_and(|row| row.group.is_some())
    }

    pub(crate) fn apply(
        &mut self,
        input: OverviewInput,
        state: &EnvironmentSession,
        environment: usize,
    ) {
        if self.searching() {
            self.edit_search(input, state);
            return;
        }
        match input {
            OverviewInput::Up => self.selected = self.selected.saturating_sub(1),
            OverviewInput::Down => {
                self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1));
            }
            OverviewInput::PageUp => self.selected = self.selected.saturating_sub(10),
            OverviewInput::PageDown => {
                self.selected = (self.selected + 10).min(self.rows.len().saturating_sub(1));
            }
            OverviewInput::Top => self.selected = 0,
            OverviewInput::Bottom => self.selected = self.rows.len().saturating_sub(1),
            OverviewInput::ToggleExpand => {
                let anchor = self.anchor(environment);
                if let Some(group) = self
                    .rows
                    .get(self.selected)
                    .and_then(|row| row.group.clone())
                {
                    if !self.expanded.remove(&group) {
                        self.expanded.insert(group);
                    }
                    self.rebuild(state.overview(), anchor.as_ref());
                }
            }
            OverviewInput::SearchStart => {
                self.search = Some(Search {
                    previous: self.filter.clone(),
                    anchor: self.anchor(environment),
                    cursor: text_input::last_grapheme_boundary(&self.filter),
                });
            }
            OverviewInput::SearchCancel => {
                self.filter.clear();
                self.rebuild(state.overview(), self.anchor(environment).as_ref());
            }
            _ => {}
        }
    }

    fn anchor(&self, environment: usize) -> Option<Anchor> {
        let row = self.rows.get(self.selected)?;
        let address = row
            .cells
            .get(environment)
            .and_then(|cell| cell.members.first())
            .or_else(|| row.cells.iter().find_map(|cell| cell.members.first()))
            .unwrap_or(&row.address)
            .clone();
        Some(Anchor {
            group: row.group.clone(),
            address,
        })
    }

    fn rebuild(&mut self, overview: &EnvironmentOverview, anchor: Option<&Anchor>) {
        self.rows = rows(overview, &self.filter, &self.expanded);
        let selected = anchor.and_then(|anchor| {
            if let Some(group) = &anchor.group
                && let Some(index) = self
                    .rows
                    .iter()
                    .position(|row| row.group.as_ref() == Some(group))
            {
                return Some(index);
            }
            if let Some(index) = self
                .rows
                .iter()
                .position(|row| row.group.is_none() && row.address == anchor.address)
            {
                return Some(index);
            }
            let containing = self.rows.iter().find(|row| {
                row.cells
                    .iter()
                    .any(|cell| cell.members.contains(&anchor.address))
            })?;
            if let Some(group) = containing.group.clone() {
                self.expanded.insert(group);
                self.rows = rows(overview, &self.filter, &self.expanded);
            }
            self.rows
                .iter()
                .position(|row| row.address == anchor.address)
        });
        self.selected = selected.unwrap_or(0);
        if selected.is_none() {
            self.vertical = 0;
        }
    }

    fn edit_search(&mut self, input: OverviewInput, state: &EnvironmentSession) {
        let search = self.search.as_mut().expect("active search");
        match input {
            OverviewInput::SearchConfirm => {
                self.search = None;
                return;
            }
            OverviewInput::SearchCancel => {
                let search = self.search.take().expect("active search");
                self.filter = search.previous;
                self.rebuild(state.overview(), search.anchor.as_ref());
                return;
            }
            OverviewInput::SearchChar(character) => {
                self.filter.insert(search.cursor, character);
                search.cursor = text_input::next_grapheme_boundary_at_or_after(
                    &self.filter,
                    search.cursor + character.len_utf8(),
                );
            }
            OverviewInput::SearchBackspace if search.cursor > 0 => {
                let previous = text_input::previous_grapheme_boundary(&self.filter, search.cursor);
                self.filter.drain(previous..search.cursor);
                search.cursor = previous;
            }
            OverviewInput::SearchLeft => {
                search.cursor = text_input::previous_grapheme_boundary(&self.filter, search.cursor);
            }
            OverviewInput::SearchRight => {
                search.cursor = text_input::next_grapheme_boundary(&self.filter, search.cursor);
            }
            OverviewInput::SearchHome => search.cursor = 0,
            OverviewInput::SearchEnd => search.cursor = self.filter.len(),
            _ => return,
        }
        self.rebuild(state.overview(), None);
    }
}

fn rows(overview: &EnvironmentOverview, filter: &str, expanded: &BTreeSet<GroupId>) -> Vec<Row> {
    let mut rows = Vec::new();
    for row in &overview.rows {
        match row {
            OverviewRow::Individual(row) if row.address.contains(filter) => {
                rows.push(individual(row, false));
            }
            OverviewRow::Group(group) => {
                let children: Vec<_> = group
                    .children
                    .iter()
                    .filter(|child| child.address.contains(filter))
                    .collect();
                if children.is_empty() {
                    continue;
                }
                if children.len() == 1 {
                    rows.push(individual(children[0], false));
                    continue;
                }
                let cells = group
                    .cells
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| {
                        let members: Vec<_> = children
                            .iter()
                            .filter(|child| {
                                matches!(child.cells[index].state, CellState::Change { .. })
                            })
                            .collect();
                        MatrixCell {
                            state: cell.state.clone(),
                            source: members
                                .first()
                                .and_then(|child| child.cells[index].source.clone()),
                            members: members.iter().map(|child| child.address.clone()).collect(),
                        }
                    })
                    .collect();
                rows.push(Row {
                    address: group.display_address.clone(),
                    group: Some(group.id.clone()),
                    child: false,
                    cells,
                    difference: None,
                });
                if expanded.contains(&group.id) {
                    rows.extend(children.into_iter().map(|child| individual(child, true)));
                }
            }
            OverviewRow::Individual(_) => {}
        }
    }
    rows
}

fn individual(row: &ComparisonRow, child: bool) -> Row {
    Row {
        address: row.address.clone(),
        group: None,
        child,
        cells: row
            .cells
            .iter()
            .map(|cell| MatrixCell {
                state: cell.state.clone(),
                source: cell.source.clone(),
                members: vec![row.address.clone()],
            })
            .collect(),
        difference: row.difference,
    }
}
