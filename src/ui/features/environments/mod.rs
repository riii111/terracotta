mod render;

use std::ops::ControlFlow;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Rect, Size};

use super::{
    overview::{self, OverviewInput, matrix::MatrixView},
    plan_review::{self, PlanReviewInput, PlanReviewViewState},
};
use crate::{
    app::{
        copy::CopyTarget,
        environments::{EnvironmentSession, EnvironmentState, comparison::CellState},
        session::{Action, ReviewSessionState},
    },
    ui::{
        input::normalize_key,
        shell::environments::{self, EnvironmentSelection},
    },
};

#[derive(Default)]
pub(crate) struct EnvironmentView {
    selection: EnvironmentSelection,
    matrix: MatrixView,
    confirming_quit: bool,
    reviews: Vec<PlanReviewViewState>,
    notice: Option<String>,
    dialog: Option<String>,
    dialog_scroll: u16,
}

pub(crate) enum EnvironmentInput {
    Retry(usize),
    Review(usize, Box<Action>),
    Quit,
    Interrupt,
}

impl EnvironmentView {
    pub(crate) fn handle_key(
        &mut self,
        key: KeyEvent,
        size: Size,
        state: &EnvironmentSession,
    ) -> Option<EnvironmentInput> {
        self.sync(state);
        let key = normalize_key(key);
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.selection.raw.is_none() && self.matrix.searching() {
                self.matrix
                    .apply(OverviewInput::SearchCancel, state, self.selection.column);
                return None;
            }
            return Some(EnvironmentInput::Interrupt);
        }
        if self.confirming_quit {
            return match key.code {
                KeyCode::Enter => Some(EnvironmentInput::Interrupt),
                KeyCode::Esc => {
                    self.confirming_quit = false;
                    None
                }
                _ => None,
            };
        }
        if self.dialog.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => self.dialog = None,
                KeyCode::Up | KeyCode::PageUp => {
                    self.dialog_scroll = self.dialog_scroll.saturating_sub(4);
                }
                KeyCode::Down | KeyCode::PageDown => {
                    self.dialog_scroll = self.dialog_scroll.saturating_add(4);
                }
                KeyCode::Char('q') => return self.quit(state),
                _ => {}
            }
            return None;
        }
        let editing = self.selection.raw.map_or_else(
            || self.matrix.searching(),
            |index| self.reviews[index].searching() || self.reviews[index].overlay().is_some(),
        );
        if !editing && let ControlFlow::Break(result) = self.navigation(key, state) {
            return result;
        }
        if let Some(index) = self.selection.raw {
            return self.handle_review_key(key, size, state, state.plans()[index].review()?);
        }
        let input = overview::key_to_input(key, self.matrix.searching(), self.matrix.filtered())?;
        match input {
            OverviewInput::Quit => self.quit(state),
            OverviewInput::Open => self.open(state, self.selection.column, false),
            OverviewInput::ViewPlan => self.open(state, self.selection.column, true),
            OverviewInput::Copy => Some(EnvironmentInput::Review(
                self.selection.column,
                Box::new(Action::Copy(CopyTarget::Plan)),
            )),
            OverviewInput::OpenContext => {
                if let Some(plan) = state.plans().get(self.selection.column) {
                    self.show_dialog(format!(
                        "Context\n{}\n\n↑↓ scroll   Esc close",
                        environments::context(plan)
                    ));
                }
                None
            }
            OverviewInput::OpenHelp => {
                self.help();
                None
            }
            _ => {
                self.matrix.apply(input, state, self.selection.column);
                None
            }
        }
    }

    fn sync(&mut self, state: &EnvironmentSession) {
        self.reviews
            .resize_with(state.plans().len(), PlanReviewViewState::default);
        self.matrix.sync(state, self.selection.column);
    }

    fn navigation(
        &mut self,
        key: KeyEvent,
        state: &EnvironmentSession,
    ) -> ControlFlow<Option<EnvironmentInput>> {
        match key.code {
            KeyCode::Char('0' | 's') => {
                self.selection.raw = None;
                self.notice = None;
            }
            KeyCode::Esc
                if self.selection.raw.is_some_and(|index| {
                    state.plans()[index]
                        .review()
                        .is_some_and(|review| review.review().search_query().is_empty())
                }) =>
            {
                self.selection.raw = None;
                self.notice = None;
            }
            KeyCode::Char('1'..='9') => {
                let KeyCode::Char(digit) = key.code else {
                    unreachable!()
                };
                let index = digit as usize - '1' as usize;
                if index < state.plans().len() {
                    return ControlFlow::Break(self.open(state, index, false));
                }
            }
            KeyCode::Char('[' | ']') | KeyCode::Left | KeyCode::Right
                if self.selection.raw.is_none() || matches!(key.code, KeyCode::Char('[' | ']')) =>
            {
                let delta = if matches!(key.code, KeyCode::Left | KeyCode::Char('[')) {
                    -1
                } else {
                    1
                };
                let index = self.selection.adjacent(delta, state.plans().len());
                if self.selection.raw.is_some() {
                    return ControlFlow::Break(self.open(state, index, false));
                }
                self.selection.column = index;
                self.notice = None;
            }
            KeyCode::Char('r') => {
                let index = self.selection.active();
                return ControlFlow::Break(
                    state
                        .plans()
                        .get(index)
                        .filter(|plan| matches!(plan.state(), EnvironmentState::Error))
                        .map(|_| EnvironmentInput::Retry(index)),
                );
            }
            KeyCode::Char('?') => self.help(),
            _ => return ControlFlow::Continue(()),
        }
        ControlFlow::Break(None)
    }

    fn open(
        &mut self,
        state: &EnvironmentSession,
        index: usize,
        full: bool,
    ) -> Option<EnvironmentInput> {
        let plan = state.plans().get(index)?;
        if plan.review().is_none() {
            self.selection.raw = None;
            self.selection.column = index;
            self.show_dialog(format!(
                "{}: {}\n{}\n{}\n\nEsc close   r retries Error after closing",
                environments::name(plan),
                environments::status(plan),
                environments::context(plan),
                if matches!(plan.state(), EnvironmentState::Error) {
                    plan.diagnostic().text().to_owned()
                } else {
                    "Only Ready environments have a reviewable plan.".to_owned()
                }
            ));
            return None;
        }
        let cell = (!full).then(|| self.matrix.cell(index)).flatten();
        if cell.is_some_and(|cell| {
            cell.state == CellState::Missing
                || (matches!(cell.state, CellState::Change { .. }) && cell.members.is_empty())
        }) {
            self.notice = Some("This resource does not exist in this environment.".to_owned());
            return None;
        }
        let unchanged = cell.is_some_and(|cell| cell.state == CellState::NoOp);
        let line = if unchanged {
            0
        } else {
            cell.and_then(|cell| cell.source.as_ref())
                .and_then(|source| source.line)
                .unwrap_or(0)
        };
        self.notice = if unchanged {
            Some("This resource has no changes. Showing the full plan from the top.".to_owned())
        } else if !full
            && cell.is_some_and(|cell| {
                cell.source
                    .as_ref()
                    .is_none_or(|source| source.line.is_none())
            })
        {
            Some("No matching raw block. Showing the full plan from the top.".to_owned())
        } else {
            None
        };
        self.selection.raw = Some(index);
        let offset = if full || unchanged {
            0
        } else {
            plan_review::source_offset(plan.review()?, line)
        };
        self.reviews[index].jump_to_line(offset, u16::MAX);
        Some(EnvironmentInput::Review(
            index,
            Box::new(Action::ReviewSearchChanged(String::new())),
        ))
    }

    fn handle_review_key(
        &mut self,
        key: KeyEvent,
        size: Size,
        state: &EnvironmentSession,
        review: &ReviewSessionState,
    ) -> Option<EnvironmentInput> {
        let index = self.selection.raw?;
        let view = &mut self.reviews[index];
        if view.overlay().is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => view.close_overlay(),
                KeyCode::Up => view.scroll_overlay(-1),
                KeyCode::Down => view.scroll_overlay(1),
                KeyCode::PageUp => view.scroll_overlay(-8),
                KeyCode::PageDown => view.scroll_overlay(8),
                _ => {}
            }
            return None;
        }
        let input = plan_review::key_to_input(
            key,
            view.searching(),
            !review.review().search_query().is_empty(),
        )?;
        match input {
            PlanReviewInput::Quit => return self.quit(state),
            PlanReviewInput::Copy => {
                return Some(EnvironmentInput::Review(
                    index,
                    Box::new(Action::Copy(CopyTarget::Plan)),
                ));
            }
            PlanReviewInput::Apply | PlanReviewInput::OpenOverview => return None,
            _ => {}
        }
        let area = environments::layout(
            Rect::new(0, 0, size.width, size.height),
            state,
            self.notice.as_deref(),
            false,
        )
        .body;
        let layout = plan_review::environment_layout(area, view.searching(), review);
        view.apply_with_matches(
            input,
            layout.body(),
            layout.max_vertical(),
            layout.max_horizontal(),
            review.review().search_query(),
            layout.matches(),
        )
        .map(|query| EnvironmentInput::Review(index, Box::new(Action::ReviewSearchChanged(query))))
    }

    fn show_dialog(&mut self, text: String) {
        self.dialog = Some(text);
        self.dialog_scroll = 0;
    }

    fn help(&mut self) {
        self.show_dialog("Help\n↑↓ / j k select row   ←→ select environment\nEnter open selected resource   Space expand/collapse group\n1-9 open resource in that environment   [ ] previous/next environment\n0 / s Overview   Esc return to the same row and column\n/ filter full addresses   v full plan from the top\nr retry selected Error environment\ny copy selected environment's full plan   c full environment context\nSame change compares patterns; unknown values remain unknown.\nCompared lists only Ready environments. Excluded environments are not retried.\nq quit (confirmation while acquiring)\n\n↑↓ scroll   ? / Esc close".to_owned());
    }

    fn quit(&mut self, state: &EnvironmentSession) -> Option<EnvironmentInput> {
        if state.acquiring() {
            self.confirming_quit = true;
            None
        } else {
            Some(EnvironmentInput::Quit)
        }
    }
}

#[cfg(test)]
mod tests;
