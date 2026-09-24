use std::collections::BTreeSet;

use crate::app::environments::{
    EnvironmentSession,
    comparison::EnvironmentSelection,
    comparison::{CellState, ComparisonRow, DifferenceReason},
    overview::{
        EnvironmentOverview, GroupId, OverviewRow, OverviewRowId,
        environment_overview_for_selection,
    },
};
use crate::ui::features::overview::OverviewInput;
use crate::ui::text_input;

#[derive(Clone)]
pub(crate) struct MatrixCell {
    pub(crate) state: CellState,
    pub(crate) members: Vec<String>,
}

pub(super) struct Row {
    pub(super) address: String,
    pub(super) group: Option<GroupId>,
    pub(super) id: OverviewRowId,
    pub(super) child: bool,
    pub(super) cells: Vec<MatrixCell>,
    pub(super) difference: Option<DifferenceReason>,
}

struct Search {
    previous: String,
    cursor: usize,
}

#[derive(Default)]
pub(crate) struct MatrixView {
    pub(super) rows: Vec<Row>,
    pub(super) vertical: usize,
    pub(super) first_column: usize,
    pub(super) expanded: BTreeSet<GroupId>,
    pub(super) filter: String,
    pub(super) overview: Option<EnvironmentOverview>,
    pub(super) environments: Vec<usize>,
    search: Option<Search>,
    revision: Option<u64>,
}

impl MatrixView {
    pub(crate) fn sync(&mut self, state: &EnvironmentSession, environments: &[usize]) {
        if self.revision != Some(state.revision()) || self.environments != environments {
            self.overview = Some(if environments.len() == state.plans().len() {
                state.overview().clone()
            } else {
                let selection =
                    EnvironmentSelection::new(Some(environments.to_vec()), state.plans().len())
                        .expect("visible environment indexes form a valid selection");
                environment_overview_for_selection(state.plans(), &selection)
            });
            self.environments = environments.to_vec();
            self.rebuild();
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

    pub(crate) fn groups_expanded(&self) -> Option<bool> {
        let groups: Vec<_> = self
            .rows
            .iter()
            .filter(|row| row.group.is_some())
            .filter_map(|row| match &row.id {
                OverviewRowId::Group(id) => Some(id),
                OverviewRowId::Individual(_) => None,
            })
            .collect();
        (!groups.is_empty()).then(|| groups.iter().all(|group| self.expanded.contains(group)))
    }

    pub(crate) fn apply(&mut self, input: OverviewInput, page_size: usize) {
        if self.searching() {
            self.edit_search(input);
            return;
        }
        match input {
            OverviewInput::Up => self.vertical = self.vertical.saturating_sub(1),
            OverviewInput::Down => self.vertical = self.vertical.saturating_add(1),
            OverviewInput::PageUp => {
                self.vertical = self.vertical.saturating_sub(page_size.max(1));
            }
            OverviewInput::PageDown => {
                self.vertical = self.vertical.saturating_add(page_size.max(1));
            }
            OverviewInput::Top => self.vertical = 0,
            OverviewInput::Bottom => self.vertical = usize::MAX,
            OverviewInput::ToggleExpand => {
                let groups: BTreeSet<_> = self
                    .rows
                    .iter()
                    .filter(|row| row.group.is_some())
                    .filter_map(|row| match &row.id {
                        OverviewRowId::Group(id) => Some(id.clone()),
                        OverviewRowId::Individual(_) => None,
                    })
                    .collect();
                if groups.iter().any(|group| !self.expanded.contains(group)) {
                    self.expanded.extend(groups);
                } else {
                    self.expanded.retain(|group| !groups.contains(group));
                }
                self.rebuild();
            }
            OverviewInput::SearchStart => {
                self.search = Some(Search {
                    previous: self.filter.clone(),
                    cursor: text_input::last_grapheme_boundary(&self.filter),
                });
            }
            OverviewInput::SearchCancel => {
                self.filter.clear();
                self.rebuild();
            }
            _ => {}
        }
    }

    fn rebuild(&mut self) {
        let Some(overview) = &self.overview else {
            self.rows.clear();
            self.vertical = 0;
            return;
        };
        self.rows = rows(overview, &self.filter, &self.expanded);
        if self.rows.is_empty() {
            self.vertical = 0;
        }
    }

    fn edit_search(&mut self, input: OverviewInput) {
        let search = self.search.as_mut().expect("active search");
        match input {
            OverviewInput::SearchConfirm => {
                self.search = None;
                return;
            }
            OverviewInput::SearchCancel => {
                let search = self.search.take().expect("active search");
                self.filter = search.previous;
                self.rebuild();
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
        self.rebuild();
    }
}

fn rows(overview: &EnvironmentOverview, filter: &str, expanded: &BTreeSet<GroupId>) -> Vec<Row> {
    let mut rows = Vec::new();
    for row in &overview.rows {
        match row {
            OverviewRow::Individual(row) if row.address.contains(filter) => {
                rows.push(individual(
                    row,
                    false,
                    OverviewRowId::Individual(row.address.clone()),
                ));
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
                    rows.push(individual(
                        children[0],
                        false,
                        OverviewRowId::Individual(children[0].address.clone()),
                    ));
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
                            members: members.iter().map(|child| child.address.clone()).collect(),
                        }
                    })
                    .collect();
                rows.push(Row {
                    address: group.display_address.clone(),
                    group: Some(group.id.clone()),
                    id: OverviewRowId::Group(group.id.clone()),
                    child: false,
                    cells,
                    difference: None,
                });
                if expanded.contains(&group.id) {
                    rows.extend(children.into_iter().map(|child| {
                        individual(
                            child,
                            true,
                            OverviewRowId::Individual(child.address.clone()),
                        )
                    }));
                }
            }
            OverviewRow::Individual(_) => {}
        }
    }
    rows
}

fn individual(row: &ComparisonRow, child: bool, id: OverviewRowId) -> Row {
    Row {
        address: row.address.clone(),
        group: None,
        id,
        child,
        cells: row
            .cells
            .iter()
            .map(|cell| MatrixCell {
                state: cell.state.clone(),
                members: vec![row.address.clone()],
            })
            .collect(),
        difference: row.difference,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::environments::comparison::ComparisonScope;

    #[test]
    fn individual_row_identity_keeps_the_full_resource_address() {
        let address = "module.application.terraform_data.api[\"primary\"]".to_owned();
        let overview = EnvironmentOverview {
            scope: ComparisonScope::All { compared: vec![0] },
            rows: vec![OverviewRow::Individual(ComparisonRow {
                address: address.clone(),
                cells: Vec::new(),
                difference: None,
                has_unknown: false,
            })],
        };

        let rows = rows(&overview, "", &BTreeSet::new());

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, OverviewRowId::Individual(address));
    }
}
