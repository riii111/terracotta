mod render;
mod sidebar;

use std::ops::ControlFlow;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Size;

use super::{
    overview::{self, OverviewInput, matrix::MatrixView},
    plan_review::{self, PlanReviewInput, PlanReviewViewState},
};
use crate::{
    app::{
        copy::CopyTarget,
        environments::{EnvironmentSession, EnvironmentState},
        session::{Action, ReviewSessionState},
    },
    ui::{
        input::normalize_key,
        shell::environments::{self, EnvironmentPane, EnvironmentSelection},
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SidebarSetting {
    #[default]
    Uninitialized,
    Closed,
    Open,
}

pub(crate) struct EnvironmentView {
    selection: EnvironmentSelection,
    selected_environments: Option<Vec<usize>>,
    matrix: MatrixView,
    confirming_quit: bool,
    reviews: Vec<PlanReviewViewState>,
    notice: Option<String>,
    dialog: Option<EnvironmentDialog>,
    dialog_scroll: u16,
    focus: EnvironmentPane,
    last_right_focus: EnvironmentPane,
    sidebar_enabled: bool,
    sidebar: SidebarSetting,
    sidebar_width: u16,
    maximized: Option<EnvironmentPane>,
}

enum EnvironmentDialog {
    Help,
    Message(String),
}

pub(crate) enum EnvironmentInput {
    Retry(usize),
    Review(usize, Box<Action>),
    Quit,
    Interrupt,
}

impl Default for EnvironmentView {
    fn default() -> Self {
        Self {
            selection: EnvironmentSelection::default(),
            selected_environments: None,
            matrix: MatrixView::default(),
            confirming_quit: false,
            reviews: Vec::new(),
            notice: None,
            dialog: None,
            dialog_scroll: 0,
            focus: EnvironmentPane::Matrix,
            last_right_focus: EnvironmentPane::Matrix,
            sidebar_enabled: false,
            sidebar: SidebarSetting::Uninitialized,
            sidebar_width: 24,
            maximized: None,
        }
    }
}

impl EnvironmentView {
    pub(crate) fn handle_key(
        &mut self,
        key: KeyEvent,
        size: Size,
        state: &EnvironmentSession,
    ) -> Option<EnvironmentInput> {
        self.initialize(size, state);
        self.sync(state);
        let key = normalize_key(key);
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.selection.raw.is_none() && self.matrix.searching() {
                self.matrix.apply(OverviewInput::SearchCancel, 1);
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
            return self.handle_dialog_key(key, state);
        }

        let editing = self.is_editing();
        let clearing_filter =
            self.selection.raw.is_none() && self.matrix.filtered() && key.code == KeyCode::Esc;
        let matrix_page = if !editing && !clearing_filter {
            self.overview_page_size(size, state)
        } else {
            1
        };
        if !editing
            && !clearing_filter
            && let ControlFlow::Break(result) = self.navigation(key, size, state)
        {
            return result;
        }
        if let Some(index) = self.selection.raw {
            return self.handle_review_key(key, size, state, state.plans()[index].review()?);
        }

        if self.active_pane(size.width) == EnvironmentPane::Environments
            && let ControlFlow::Break(result) = self.handle_environment_key(key, state)
        {
            return result;
        }

        let input = overview::key_to_input(key, self.matrix.searching(), self.matrix.filtered())?;
        self.handle_overview_input(input, state, matrix_page)
    }

    fn initialize(&mut self, size: Size, state: &EnvironmentSession) {
        if self.sidebar != SidebarSetting::Uninitialized {
            return;
        }
        self.sidebar_enabled = state.plans().len() > 1;
        self.sidebar_width = environments::sidebar_width(state.plans());
        self.sidebar = if self.sidebar_enabled && size.width >= 120 {
            SidebarSetting::Open
        } else {
            SidebarSetting::Closed
        };
        self.focus = if self.sidebar == SidebarSetting::Open {
            EnvironmentPane::Environments
        } else {
            EnvironmentPane::Matrix
        };
        self.last_right_focus = EnvironmentPane::Matrix;
    }

    fn sync(&mut self, state: &EnvironmentSession) {
        self.reviews
            .resize_with(state.plans().len(), PlanReviewViewState::default);
        let environments = self.compared_environments(state.plans().len());
        self.matrix.sync(state, &environments);
    }

    fn is_editing(&self) -> bool {
        self.selection.raw.map_or_else(
            || self.matrix.searching(),
            |index| self.reviews[index].searching() || self.reviews[index].overlay().is_some(),
        )
    }

    fn navigation(
        &mut self,
        key: KeyEvent,
        size: Size,
        state: &EnvironmentSession,
    ) -> ControlFlow<Option<EnvironmentInput>> {
        if let Some(index) = self.selection.raw {
            return self.raw_navigation(key, index, size, state);
        }

        match key.code {
            KeyCode::Char('1') => {
                if self.sidebar_enabled && size.width >= 90 {
                    self.sidebar = SidebarSetting::Open;
                    self.focus = EnvironmentPane::Environments;
                    self.maximized = None;
                }
                return ControlFlow::Break(None);
            }
            KeyCode::Char('2') => {
                self.focus = EnvironmentPane::Matrix;
                self.last_right_focus = EnvironmentPane::Matrix;
                self.maximized = None;
                return ControlFlow::Break(None);
            }
            KeyCode::Char('3') => return ControlFlow::Break(None),
            KeyCode::Char('b') if self.maximized.is_none() => {
                if self.sidebar_enabled && size.width >= 90 {
                    if self.sidebar == SidebarSetting::Open {
                        self.sidebar = SidebarSetting::Closed;
                        if self.focus == EnvironmentPane::Environments {
                            self.focus = self.last_right_focus;
                        }
                    } else {
                        self.sidebar = SidebarSetting::Open;
                    }
                }
                return ControlFlow::Break(None);
            }
            KeyCode::Char('f') => {
                if self.maximized.is_some() {
                    self.maximized = None;
                } else {
                    self.maximized = Some(self.active_pane(size.width));
                }
                return ControlFlow::Break(None);
            }
            KeyCode::Char('[' | ']') => {
                let delta = if key.code == KeyCode::Char('[') {
                    -1
                } else {
                    1
                };
                let index = adjacent_environment(self.selection.column, delta, state.plans().len());
                self.select_environment(index);
                return ControlFlow::Break(None);
            }
            KeyCode::Char('r') => {
                let index = self.selection.column;
                let retry = state
                    .plans()
                    .get(index)
                    .filter(|plan| matches!(plan.state(), EnvironmentState::Error))
                    .map(|_| EnvironmentInput::Retry(index));
                return ControlFlow::Break(retry);
            }
            KeyCode::Char('0' | 's') => {
                self.selection.raw = None;
                self.notice = None;
                return ControlFlow::Break(None);
            }
            KeyCode::Esc if self.matrix.filtered() => {
                return ControlFlow::Continue(());
            }
            KeyCode::Esc if self.maximized.is_some() => {
                self.maximized = None;
                return ControlFlow::Break(None);
            }
            KeyCode::Left | KeyCode::Right
                if self.active_pane(size.width) == EnvironmentPane::Matrix =>
            {
                let delta = if key.code == KeyCode::Left { -1 } else { 1 };
                let index = adjacent_environment(self.selection.column, delta, state.plans().len());
                self.select_environment(index);
                return ControlFlow::Break(None);
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn raw_navigation(
        &mut self,
        key: KeyEvent,
        index: usize,
        _size: Size,
        state: &EnvironmentSession,
    ) -> ControlFlow<Option<EnvironmentInput>> {
        match key.code {
            KeyCode::Char('[' | ']') => {
                let delta = if key.code == KeyCode::Char('[') {
                    -1
                } else {
                    1
                };
                let next = adjacent_environment(index, delta, state.plans().len());
                return ControlFlow::Break(self.open(state, next));
            }
            KeyCode::Char('0' | 's') => {
                self.selection.raw = None;
                self.notice = None;
                return ControlFlow::Break(None);
            }
            KeyCode::Esc
                if state.plans()[index]
                    .review()
                    .is_some_and(|review| review.review().search_query().is_empty()) =>
            {
                self.selection.raw = None;
                self.notice = None;
                return ControlFlow::Break(None);
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn handle_environment_key(
        &mut self,
        key: KeyEvent,
        state: &EnvironmentSession,
    ) -> ControlFlow<Option<EnvironmentInput>> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_environment(self.selection.column.saturating_sub(1));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let last = state.plans().len().saturating_sub(1);
                self.select_environment(self.selection.column.saturating_add(1).min(last));
            }
            KeyCode::Home => self.select_environment(0),
            KeyCode::End => self.select_environment(state.plans().len().saturating_sub(1)),
            KeyCode::PageUp => self.select_environment(self.selection.column.saturating_sub(5)),
            KeyCode::PageDown => {
                let last = state.plans().len().saturating_sub(1);
                self.select_environment(self.selection.column.saturating_add(5).min(last));
            }
            KeyCode::Char(' ') => self.toggle_comparison(state.plans().len()),
            KeyCode::Char('o') => self.select_only_environment(state.plans().len()),
            KeyCode::Char('a') => self.select_all_environments(),
            KeyCode::Enter | KeyCode::Char('v') => {
                return ControlFlow::Break(self.open(state, self.selection.column));
            }
            KeyCode::Char('c') => {
                if let Some(plan) = state.plans().get(self.selection.column) {
                    self.show_dialog(format!(
                        "Context\n{}\n\nEsc close",
                        environments::context(plan)
                    ));
                }
            }
            KeyCode::Char('y') => {
                return ControlFlow::Break(Some(EnvironmentInput::Review(
                    self.selection.column,
                    Box::new(Action::Copy(CopyTarget::Plan)),
                )));
            }
            KeyCode::Char('?') => self.help(),
            _ => return ControlFlow::Continue(()),
        }
        ControlFlow::Break(None)
    }

    fn handle_overview_input(
        &mut self,
        input: OverviewInput,
        state: &EnvironmentSession,
        matrix_page: usize,
    ) -> Option<EnvironmentInput> {
        match input {
            OverviewInput::Quit => self.quit(state),
            OverviewInput::Open | OverviewInput::ViewPlan => {
                self.open(state, self.selection.column)
            }
            OverviewInput::Copy => Some(EnvironmentInput::Review(
                self.selection.column,
                Box::new(Action::Copy(CopyTarget::Plan)),
            )),
            OverviewInput::OpenContext => {
                if let Some(plan) = state.plans().get(self.selection.column) {
                    self.show_dialog(format!(
                        "Context\n{}\n\nEsc close",
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
                self.matrix.apply(input, matrix_page);
                None
            }
        }
    }

    fn raw_area(size: Size) -> ratatui::layout::Rect {
        ratatui::layout::Rect::new(
            0,
            1.min(size.height),
            size.width,
            size.height.saturating_sub(1),
        )
    }

    fn handle_review_key(
        &mut self,
        key: KeyEvent,
        size: Size,
        state: &EnvironmentSession,
        review: &ReviewSessionState,
    ) -> Option<EnvironmentInput> {
        let index = self.selection.raw?;
        let area = Self::raw_area(size);
        let view = &mut self.reviews[index];
        if view.overlay().is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => view.close_overlay(),
                KeyCode::Up => view.scroll_overlay(-1),
                KeyCode::Down => view.scroll_overlay(1),
                KeyCode::Char('k')
                    if view.overlay() == Some(plan_review::PlanReviewOverlay::Help) =>
                {
                    view.scroll_overlay(-1);
                }
                KeyCode::Char('j')
                    if view.overlay() == Some(plan_review::PlanReviewOverlay::Help) =>
                {
                    view.scroll_overlay(1);
                }
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

    fn active_pane(&self, width: u16) -> EnvironmentPane {
        self.maximized_for_width(width).unwrap_or_else(|| {
            if self.focus == EnvironmentPane::Environments && self.sidebar_visible(width) {
                EnvironmentPane::Environments
            } else {
                EnvironmentPane::Matrix
            }
        })
    }

    const fn sidebar_visible(&self, width: u16) -> bool {
        self.sidebar_enabled && matches!(self.sidebar, SidebarSetting::Open) && width >= 90
    }

    fn maximized_for_width(&self, width: u16) -> Option<EnvironmentPane> {
        self.maximized
            .filter(|pane| *pane != EnvironmentPane::Environments || width >= 90)
    }

    fn compared_environments(&self, count: usize) -> Vec<usize> {
        self.selected_environments
            .clone()
            .unwrap_or_else(|| (0..count).collect())
    }

    fn select_environment(&mut self, index: usize) {
        if self.selection.column != index {
            self.selection.column = index;
            self.notice = None;
        }
    }

    fn toggle_comparison(&mut self, count: usize) {
        let mut selected = self.compared_environments(count);
        if let Some(position) = selected
            .iter()
            .position(|index| *index == self.selection.column)
        {
            if selected.len() == 1 {
                self.notice =
                    Some("At least one environment must stay in the comparison.".to_owned());
                return;
            }
            selected.remove(position);
        } else {
            selected.push(self.selection.column);
            selected.sort_unstable();
        }
        self.set_comparison(selected, count);
    }

    fn select_only_environment(&mut self, count: usize) {
        self.set_comparison(vec![self.selection.column], count);
    }

    fn select_all_environments(&mut self) {
        self.selected_environments = None;
        self.notice = None;
    }

    fn set_comparison(&mut self, selected: Vec<usize>, count: usize) {
        let all = selected.len() == count && selected.iter().copied().eq(0..count);
        self.selected_environments = (!all).then_some(selected);
        self.notice = None;
    }

    fn open(&mut self, state: &EnvironmentSession, index: usize) -> Option<EnvironmentInput> {
        let plan = state.plans().get(index)?;
        if plan.review().is_none() {
            self.selection.raw = None;
            self.select_environment(index);
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
        self.notice = None;
        self.selection.raw = Some(index);
        self.reviews[index].jump_to_line(0, u16::MAX);
        Some(EnvironmentInput::Review(
            index,
            Box::new(Action::ReviewSearchChanged(String::new())),
        ))
    }

    fn handle_dialog_key(
        &mut self,
        key: KeyEvent,
        state: &EnvironmentSession,
    ) -> Option<EnvironmentInput> {
        let is_help = matches!(self.dialog, Some(EnvironmentDialog::Help));
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => self.dialog = None,
            KeyCode::Up | KeyCode::Char('k') if is_help => {
                self.dialog_scroll = self.dialog_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if is_help => {
                self.dialog_scroll = self.dialog_scroll.saturating_add(1);
            }
            KeyCode::Up => self.dialog_scroll = self.dialog_scroll.saturating_sub(1),
            KeyCode::Down => self.dialog_scroll = self.dialog_scroll.saturating_add(1),
            KeyCode::PageUp => {
                self.dialog_scroll = self.dialog_scroll.saturating_sub(4);
            }
            KeyCode::PageDown => {
                self.dialog_scroll = self.dialog_scroll.saturating_add(4);
            }
            KeyCode::Char('q') => return self.quit(state),
            _ => {}
        }
        None
    }

    fn show_dialog(&mut self, text: String) {
        self.dialog = Some(EnvironmentDialog::Message(text));
        self.dialog_scroll = 0;
    }

    fn help(&mut self) {
        self.dialog = Some(EnvironmentDialog::Help);
        self.dialog_scroll = 0;
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

fn adjacent_environment(active: usize, delta: isize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    active.saturating_add_signed(delta).min(count - 1)
}

#[cfg(test)]
mod tests;
