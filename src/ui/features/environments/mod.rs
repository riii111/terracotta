use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect, Size},
    text::Line,
    widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use super::plan_review::{self, PlanReviewInput, PlanReviewViewState};
use crate::{
    app::{
        copy::CopyTarget,
        environments::{EnvironmentSession, EnvironmentState},
        session::{Action, ReviewSessionState},
    },
    ui::{input::normalize_key, theme},
};

#[derive(Default)]
pub(crate) struct EnvironmentView {
    selected: usize,
    raw: bool,
    confirming_quit: bool,
    list: ListState,
    diagnostic_scroll: u16,
    reviews: Vec<PlanReviewViewState>,
}

pub(crate) enum EnvironmentInput {
    Retry(usize),
    Review(usize, Box<Action>),
    Quit,
    Interrupt,
}

impl EnvironmentView {
    pub(crate) fn render(&mut self, frame: &mut Frame<'_>, state: &EnvironmentSession) {
        self.reviews
            .resize_with(state.plans().len(), PlanReviewViewState::default);
        let area = frame.area();
        let Some(selected) = state.plans().get(self.selected) else {
            return;
        };
        if self.raw
            && let Some(review) = selected.review()
        {
            plan_review::render_environment(
                frame,
                review,
                &self.reviews[self.selected],
                std::time::Instant::now(),
            );
        } else {
            self.raw = false;
            self.render_environments(frame, state);
        }
        if self.confirming_quit {
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(
                    "Stop acquiring environment plans?\nEnter stop and quit   Esc continue",
                )
                .wrap(Wrap { trim: false }),
                area,
            );
        }
    }

    pub(crate) fn handle_key(
        &mut self,
        key: KeyEvent,
        size: Size,
        state: &EnvironmentSession,
    ) -> Option<EnvironmentInput> {
        let key = normalize_key(key);
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
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
        if self.raw
            && let Some(review) = state
                .plans()
                .get(self.selected)
                .and_then(|plan| plan.review())
        {
            return self.handle_review_key(key, size, state, review);
        }
        match key.code {
            KeyCode::Char('q') => self.quit(state),
            KeyCode::Up | KeyCode::Char('k' | '[') => {
                self.selected = self.selected.saturating_sub(1);
                self.diagnostic_scroll = 0;
                None
            }
            KeyCode::Down | KeyCode::Char('j' | ']') => {
                self.selected = (self.selected + 1).min(state.plans().len().saturating_sub(1));
                self.diagnostic_scroll = 0;
                None
            }
            KeyCode::Enter => {
                self.raw = state
                    .plans()
                    .get(self.selected)
                    .is_some_and(|plan| plan.review().is_some());
                None
            }
            KeyCode::PageDown => {
                self.diagnostic_scroll = self.diagnostic_scroll.saturating_add(4);
                None
            }
            KeyCode::PageUp => {
                self.diagnostic_scroll = self.diagnostic_scroll.saturating_sub(4);
                None
            }
            KeyCode::Char('r') => Some(EnvironmentInput::Retry(self.selected)),
            _ => None,
        }
    }

    fn render_environments(&mut self, frame: &mut Frame<'_>, state: &EnvironmentSession) {
        let area = frame.area();
        let selected = &state.plans()[self.selected];
        let areas = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(area);
        let ready = state
            .plans()
            .iter()
            .filter(|plan| matches!(plan.state(), EnvironmentState::Ready { .. }))
            .count();
        frame.render_widget(
            Paragraph::new(format!(
                "Environment plans   Ready: {ready}/{}",
                state.plans().len()
            ))
            .style(theme::accent_style()),
            areas[0],
        );
        let items: Vec<_> = state
            .plans()
            .iter()
            .map(|plan| {
                let status = match plan.state() {
                    EnvironmentState::Pending => "Pending",
                    EnvironmentState::Running => "Running",
                    EnvironmentState::Ready { .. } => "Ready",
                    EnvironmentState::Error => "Error",
                    EnvironmentState::ExcludedHcp => "Excluded: HCP execution",
                };
                let name = plan
                    .directory()
                    .file_name()
                    .unwrap_or_else(|| plan.directory().as_os_str())
                    .to_string_lossy();
                ListItem::new(Line::from(format!("{status:<10} {name}")))
            })
            .collect();
        self.list.select(Some(self.selected));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("> ")
                .highlight_style(theme::accent_style()),
            areas[1],
            &mut self.list,
        );
        let detail = match selected.state() {
            EnvironmentState::Error => selected.diagnostic().text().to_owned(),
            EnvironmentState::ExcludedHcp => {
                "Excluded: HCP execution. This environment cannot run locally.".to_owned()
            }
            _ => format!(
                "{}\n{}   ws:{}",
                selected.directory().display(),
                selected.tool.display_name(),
                selected.workspace().unwrap_or("unavailable")
            ),
        };
        frame.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: false })
                .scroll((self.diagnostic_scroll, 0)),
            areas[2],
        );
        frame.render_widget(
            Paragraph::new("↑↓ select   Enter plan   r retry Error   q quit"),
            areas[3],
        );
    }

    fn handle_review_key(
        &mut self,
        key: KeyEvent,
        size: Size,
        state: &EnvironmentSession,
        review: &ReviewSessionState,
    ) -> Option<EnvironmentInput> {
        let view = &mut self.reviews[self.selected];
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
        if !view.searching()
            && review.review().search_query().is_empty()
            && matches!(key.code, KeyCode::Esc | KeyCode::Char('s'))
        {
            self.raw = false;
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
                    self.selected,
                    Box::new(Action::Copy(CopyTarget::Plan)),
                ));
            }
            PlanReviewInput::Apply | PlanReviewInput::OpenOverview => return None,
            _ => {}
        }
        let layout = plan_review::environment_layout(
            Rect::new(0, 0, size.width, size.height),
            view.searching(),
            review,
        );
        view.apply_with_matches(
            input,
            layout.body(),
            layout.max_vertical(),
            layout.max_horizontal(),
            review.review().search_query(),
            layout.matches(),
        )
        .map(|query| {
            EnvironmentInput::Review(self.selected, Box::new(Action::ReviewSearchChanged(query)))
        })
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
