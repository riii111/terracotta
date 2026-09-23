mod filter;
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
        copy::{self, CopyTarget},
        environments::{EnvironmentSession, EnvironmentState},
        session::{Action, ReviewSessionState},
    },
    ui::{
        input::normalize_key,
        shell::environments::{self, EnvironmentSelection},
    },
};
use filter::{EnvironmentFilterDialog, EnvironmentFilterResult};

#[derive(Default)]
pub(crate) struct EnvironmentView {
    selection: EnvironmentSelection,
    selected_environments: Option<Vec<usize>>,
    matrix: MatrixView,
    preview_open: bool,
    preview_vertical: usize,
    preview_horizontal: usize,
    confirming_quit: bool,
    reviews: Vec<PlanReviewViewState>,
    notice: Option<String>,
    dialog: Option<EnvironmentDialog>,
    dialog_scroll: u16,
    filter_dialog: Option<EnvironmentFilterDialog>,
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

pub(crate) struct PlanPreview {
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) is_raw: bool,
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
            if self.filter_dialog.is_some() {
                return None;
            }
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
        if self.filter_dialog.is_some() {
            return self.handle_filter_dialog_key(key, size, state);
        }
        if self.dialog.is_some() {
            let is_help = matches!(self.dialog, Some(EnvironmentDialog::Help));
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => self.dialog = None,
                KeyCode::Up | KeyCode::Char('k') if is_help => {
                    self.dialog_scroll = self.dialog_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') if is_help => {
                    self.dialog_scroll = self.dialog_scroll.saturating_add(1);
                }
                KeyCode::PageUp if is_help => {
                    self.dialog_scroll = self.dialog_scroll.saturating_sub(4);
                }
                KeyCode::PageDown if is_help => {
                    self.dialog_scroll = self.dialog_scroll.saturating_add(4);
                }
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
        let clearing_filter = self.selection.raw.is_none()
            && self.matrix.filtered()
            && !self.preview_open
            && key.code == KeyCode::Esc;
        let (matrix_page, preview_visible, preview_page) = if !editing && !clearing_filter {
            self.overview_page_sizes(
                size,
                state,
                matches!(key.code, KeyCode::PageUp | KeyCode::PageDown),
            )
        } else {
            (1, false, 0)
        };
        if !editing
            && !clearing_filter
            && let ControlFlow::Break(result) =
                self.navigation(key, state, preview_visible, preview_page)
        {
            return result;
        }
        if let Some(index) = self.selection.raw {
            return self.handle_review_key(key, size, state, state.plans()[index].review()?);
        }
        let input = overview::key_to_input(key, self.matrix.searching(), self.matrix.filtered())?;
        self.handle_overview_input(input, size, state, matrix_page)
    }

    fn handle_overview_input(
        &mut self,
        input: OverviewInput,
        size: Size,
        state: &EnvironmentSession,
        matrix_page: usize,
    ) -> Option<EnvironmentInput> {
        match input {
            OverviewInput::Quit => self.quit(state),
            OverviewInput::Open => {
                self.preview_open = true;
                None
            }
            OverviewInput::ViewPlan => self.open(state, self.selection.column),
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
            OverviewInput::OpenEnvironmentFilter => {
                self.filter_dialog = Some(EnvironmentFilterDialog::new(
                    state.plans(),
                    self.selected_environments.as_deref(),
                    self.selection.column,
                    size,
                ));
                None
            }
            _ => {
                self.matrix.apply(input, matrix_page);
                None
            }
        }
    }

    fn sync(&mut self, state: &EnvironmentSession) {
        self.reviews
            .resize_with(state.plans().len(), PlanReviewViewState::default);
        let indexes = self.visible_environments(state.plans().len());
        self.matrix.sync(state, &indexes);
    }

    fn handle_filter_dialog_key(
        &mut self,
        key: KeyEvent,
        size: Size,
        state: &EnvironmentSession,
    ) -> Option<EnvironmentInput> {
        let result = self
            .filter_dialog
            .as_mut()?
            .handle_key(key, size, state.plans());
        match result {
            Some(EnvironmentFilterResult::Apply(selected)) => {
                self.filter_dialog = None;
                let all = selected.len() == state.plans().len()
                    && selected.iter().copied().eq(0..state.plans().len());
                self.selected_environments = (!all).then_some(selected);
                if self
                    .selected_environments
                    .as_ref()
                    .is_some_and(|indexes| !indexes.contains(&self.selection.column))
                {
                    let column = self
                        .selected_environments
                        .as_ref()
                        .and_then(|indexes| indexes.first().copied())
                        .unwrap_or(0);
                    self.select_environment(column);
                }
                self.notice = None;
                self.sync(state);
            }
            Some(EnvironmentFilterResult::Cancel) => self.filter_dialog = None,
            None => {}
        }
        None
    }

    fn visible_environments(&self, count: usize) -> Vec<usize> {
        self.selected_environments
            .clone()
            .unwrap_or_else(|| (0..count).collect())
    }

    fn selected_plan_preview(&self, state: &EnvironmentSession) -> PlanPreview {
        let Some(selected_plan) = state.plans().get(self.selection.column) else {
            return PlanPreview {
                title: "No environment selected".to_owned(),
                text: "Select an environment to preview its plan.".to_owned(),
                is_raw: false,
            };
        };
        let environment = environments::name(selected_plan);
        let title = format!("{environment} · Plan preview");
        if let Some(review) = selected_plan.review() {
            let review = review.review();
            PlanPreview {
                title,
                text: copy::sanitize_text(
                    review.document().text(),
                    review.metadata().sensitive_values(),
                ),
                is_raw: true,
            }
        } else {
            let text = match selected_plan.state() {
                EnvironmentState::Pending => "The plan has not been acquired yet.".to_owned(),
                EnvironmentState::Running => "Plan acquisition is still in progress.".to_owned(),
                EnvironmentState::Error => selected_plan.diagnostic().text().to_owned(),
                EnvironmentState::ExcludedHcp => {
                    "This environment is excluded because it uses HCP execution.".to_owned()
                }
                EnvironmentState::Ready { .. } => {
                    "No reviewable plan is available for this environment.".to_owned()
                }
            };
            PlanPreview {
                title,
                text,
                is_raw: false,
            }
        }
    }

    fn navigation(
        &mut self,
        key: KeyEvent,
        state: &EnvironmentSession,
        preview_visible: bool,
        preview_page: usize,
    ) -> ControlFlow<Option<EnvironmentInput>> {
        let visible = self.visible_environments(state.plans().len());
        if let Some(index) = self.selection.raw {
            if let Some(delta) = tab_delta(key) {
                let index = adjacent_environment(index, delta, &visible);
                return ControlFlow::Break(self.open(state, index));
            }
        } else if self.navigate_preview(key, preview_visible, preview_page) {
            return ControlFlow::Break(None);
        }
        self.navigate_selection(key, state, &visible)
    }

    fn navigate_preview(&mut self, key: KeyEvent, visible: bool, page_size: usize) -> bool {
        if self.preview_open && key.code == KeyCode::Esc {
            self.preview_open = false;
            self.notice = None;
            return true;
        }
        if !visible {
            return false;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.preview_vertical = self.preview_vertical.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.preview_vertical = self.preview_vertical.saturating_add(1);
            }
            KeyCode::PageUp => {
                self.preview_vertical = self.preview_vertical.saturating_sub(page_size);
            }
            KeyCode::PageDown => {
                self.preview_vertical = self.preview_vertical.saturating_add(page_size);
            }
            KeyCode::Home => self.preview_vertical = 0,
            KeyCode::End => self.preview_vertical = usize::MAX,
            KeyCode::Left => {
                self.preview_horizontal = self.preview_horizontal.saturating_sub(1);
            }
            KeyCode::Right => {
                self.preview_horizontal = self.preview_horizontal.saturating_add(1);
            }
            _ => return false,
        }
        true
    }

    fn navigate_selection(
        &mut self,
        key: KeyEvent,
        state: &EnvironmentSession,
        visible: &[usize],
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
                let visible_index = digit as usize - '1' as usize;
                if let Some(index) = visible.get(visible_index).copied() {
                    return ControlFlow::Break(self.open(state, index));
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
                let index = adjacent_environment(self.selection.active(), delta, visible);
                if self.selection.raw.is_some() {
                    return ControlFlow::Break(self.open(state, index));
                }
                self.select_environment(index);
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
            KeyCode::Char('?') if self.selection.raw.is_none() => self.help(),
            _ => return ControlFlow::Continue(()),
        }
        ControlFlow::Break(None)
    }

    const fn select_environment(&mut self, index: usize) {
        if self.selection.column != index {
            self.selection.column = index;
            self.preview_vertical = 0;
            self.preview_horizontal = 0;
        }
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
        let area = environments::layout(
            Rect::new(0, 0, size.width, size.height),
            state,
            self.notice.as_deref(),
            self.selected_environments.is_some(),
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

const fn tab_delta(key: KeyEvent) -> Option<isize> {
    match (key.code, key.modifiers) {
        (KeyCode::Tab, KeyModifiers::NONE) => Some(1),
        (KeyCode::BackTab, KeyModifiers::NONE | KeyModifiers::SHIFT)
        | (KeyCode::Tab, KeyModifiers::SHIFT) => Some(-1),
        _ => None,
    }
}

fn adjacent_environment(active: usize, delta: isize, visible: &[usize]) -> usize {
    let position = visible
        .iter()
        .position(|index| *index == active)
        .unwrap_or(0);
    visible
        .get(
            position
                .saturating_add_signed(delta)
                .min(visible.len().saturating_sub(1)),
        )
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
