use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Rect, Size},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

use crate::{
    app::environments::{EnvironmentPlan, EnvironmentState},
    ui::{
        input::normalize_key,
        primitives::{atoms::scrollbar, molecules::terminal_notice},
        shell::environments::{name, status},
        text_input, theme,
    },
};

const MAX_WIDTH: u16 = 76;
const MAX_HEIGHT: u16 = 20;
const MIN_WIDTH: u16 = 24;
const MIN_HEIGHT: u16 = 8;

#[derive(Default)]
pub(crate) struct EnvironmentFilterDialog {
    query: String,
    cursor: usize,
    search: Option<SearchState>,
    selected: BTreeSet<usize>,
    focused: usize,
    vertical: usize,
    notice: Option<&'static str>,
}

#[derive(Default)]
struct SearchState {
    previous_query: String,
    previous_cursor: usize,
}

pub(crate) enum EnvironmentFilterResult {
    Apply(Vec<usize>),
    Cancel,
}

impl EnvironmentFilterDialog {
    pub(crate) fn new(
        plans: &[EnvironmentPlan],
        applied: Option<&[usize]>,
        active: usize,
        size: Size,
    ) -> Self {
        let selected = applied.map_or_else(
            || (0..plans.len()).collect(),
            |indexes| indexes.iter().copied().collect(),
        );
        let visible = visible_candidates(plans, "");
        let mut dialog = Self {
            selected,
            focused: visible
                .iter()
                .position(|index| *index == active)
                .unwrap_or(0),
            ..Self::default()
        };
        dialog.keep_focus_visible(visible.len(), list_viewport(size, dialog.notice, false));
        dialog
    }

    pub(crate) const fn searching(&self) -> bool {
        self.search.is_some()
    }

    pub(crate) fn handle_key(
        &mut self,
        key: KeyEvent,
        size: Size,
        plans: &[EnvironmentPlan],
    ) -> Option<EnvironmentFilterResult> {
        let key = normalize_key(key);
        if self.searching() {
            self.search_key(key, plans);
            let candidates = visible_candidates(plans, &self.query);
            let viewport = list_viewport(size, self.notice, self.searching());
            self.keep_focus_visible(candidates.len(), viewport);
            return None;
        }

        let candidates = visible_candidates(plans, &self.query);
        let viewport = list_viewport(size, self.notice, self.searching());
        self.keep_focus_visible(candidates.len(), viewport);
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => {
                return Some(EnvironmentFilterResult::Cancel);
            }
            (KeyCode::Enter, _) => {
                if self.selected.is_empty() {
                    self.notice = Some("Select at least one environment.");
                } else {
                    return Some(EnvironmentFilterResult::Apply(
                        self.selected.iter().copied().collect(),
                    ));
                }
            }
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                self.focused = self.focused.saturating_sub(1);
            }
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                self.focused = (self.focused + 1).min(candidates.len().saturating_sub(1));
            }
            (KeyCode::PageUp, _) => {
                self.focused = self.focused.saturating_sub(viewport.max(1));
            }
            (KeyCode::PageDown, _) => {
                self.focused =
                    (self.focused + viewport.max(1)).min(candidates.len().saturating_sub(1));
            }
            (KeyCode::Home, _) => self.focused = 0,
            (KeyCode::End, _) => self.focused = candidates.len().saturating_sub(1),
            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                if let Some(index) = candidates.get(self.focused) {
                    if !self.selected.remove(index) {
                        self.selected.insert(*index);
                    }
                    self.notice = None;
                }
            }
            (KeyCode::Char('a'), KeyModifiers::NONE) => {
                self.query.clear();
                self.cursor = 0;
                self.selected = (0..plans.len()).collect();
                self.focused = 0;
                self.vertical = 0;
                self.notice = None;
            }
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.search = Some(SearchState {
                    previous_query: self.query.clone(),
                    previous_cursor: self.cursor,
                });
                self.notice = None;
            }
            _ => {}
        }
        let candidates = visible_candidates(plans, &self.query);
        self.keep_focus_visible(
            candidates.len(),
            list_viewport(size, self.notice, self.searching()),
        );
        None
    }

    fn search_key(&mut self, key: KeyEvent, plans: &[EnvironmentPlan]) {
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => self.search = None,
            (KeyCode::Esc, _) => {
                if let Some(search) = self.search.take() {
                    self.query = search.previous_query;
                    self.cursor = search.previous_cursor;
                    self.reset_visible_rows(plans);
                }
            }
            (KeyCode::Backspace, _) if self.cursor > 0 => {
                let start = text_input::previous_grapheme_boundary(&self.query, self.cursor);
                self.query.drain(start..self.cursor);
                self.cursor = start;
                self.reset_visible_rows(plans);
            }
            (KeyCode::Left, _) => {
                self.cursor = text_input::previous_grapheme_boundary(&self.query, self.cursor);
            }
            (KeyCode::Right, _) => {
                self.cursor = text_input::next_grapheme_boundary(&self.query, self.cursor);
            }
            (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => self.cursor = 0,
            (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                self.cursor = self.query.len();
            }
            (KeyCode::Char(character), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.query.insert(self.cursor, character);
                self.cursor = text_input::next_grapheme_boundary_at_or_after(
                    &self.query,
                    self.cursor + character.len_utf8(),
                );
                self.reset_visible_rows(plans);
            }
            _ => {}
        }
    }

    fn reset_visible_rows(&mut self, plans: &[EnvironmentPlan]) {
        self.focused = 0;
        self.vertical = 0;
        if visible_candidates(plans, &self.query).is_empty() {
            self.notice = Some("No environments match this search.");
        } else {
            self.notice = None;
        }
    }

    fn keep_focus_visible(&mut self, candidate_count: usize, viewport: usize) {
        if candidate_count == 0 || viewport == 0 {
            self.focused = 0;
            self.vertical = 0;
            return;
        }
        self.focused = self.focused.min(candidate_count - 1);
        if self.focused < self.vertical {
            self.vertical = self.focused;
        } else if self.focused >= self.vertical + viewport {
            self.vertical = self.focused + 1 - viewport;
        }
        self.vertical = self.vertical.min(candidate_count.saturating_sub(viewport));
    }

    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, plans: &[EnvironmentPlan]) {
        let dimensions = dialog_dimensions(Size::new(area.width, area.height));
        let width = dimensions.width;
        let height = dimensions.height;
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            terminal_notice::render_wrapped(
                frame,
                area,
                "Terminal too small for environment filter. Resize or press Esc.",
            );
            return;
        }

        let dialog = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, dialog);
        let block = Block::bordered()
            .border_style(theme::frame_style())
            .style(theme::body_style())
            .title("Environment filter")
            .title_style(theme::accent_style().add_modifier(Modifier::BOLD));
        let inner = block.inner(dialog);
        frame.render_widget(block, dialog);

        let search_area = Rect::new(inner.x, inner.y, inner.width, 1);
        frame.render_widget(
            Paragraph::new(search_line(&self.query, self.cursor, self.searching()))
                .style(theme::body_style()),
            search_area,
        );
        let table_header = Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1);
        let (name_width, status_width) = column_widths(inner.width);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    fit_text("Environment", name_width),
                    theme::secondary_style(),
                ),
                Span::raw(" "),
                Span::styled("Plan", theme::secondary_style()),
            ])),
            table_header,
        );

        let list_y = inner.y.saturating_add(2);
        let footer_lines = footer_lines(self.notice, self.searching(), inner.width);
        let footer_height = u16::try_from(footer_lines.len()).unwrap_or(u16::MAX);
        let footer_y = inner.bottom().saturating_sub(footer_height);
        let list_height = footer_y.saturating_sub(list_y);
        let list_area = Rect::new(inner.x, list_y, inner.width, list_height);
        let candidates = visible_candidates(plans, &self.query);
        let viewport = usize::from(list_area.height);
        let mut vertical = self.vertical.min(candidates.len().saturating_sub(viewport));
        if viewport > 0 && self.focused >= vertical + viewport {
            vertical = self.focused + 1 - viewport;
        } else if self.focused < vertical {
            vertical = self.focused;
        }
        vertical = vertical.min(candidates.len().saturating_sub(viewport));
        let text_area = Rect::new(
            list_area.x,
            list_area.y,
            list_area.width.saturating_sub(1),
            list_area.height,
        );
        let lines = candidates
            .iter()
            .enumerate()
            .skip(vertical)
            .take(viewport)
            .map(|(row, index)| {
                candidate_line(
                    *index,
                    row == self.focused,
                    &self.selected,
                    plans,
                    name_width,
                    status_width,
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines).style(theme::body_style()), text_area);
        scrollbar::render_vertical(
            frame,
            Rect::new(
                list_area.right().saturating_sub(1),
                list_area.y,
                1,
                list_area.height,
            ),
            candidates.len(),
            viewport,
            vertical,
        );

        let footer = Rect::new(inner.x, footer_y, inner.width, footer_height);
        frame.render_widget(Paragraph::new(footer_lines), footer);
    }
}

fn dialog_dimensions(size: Size) -> Size {
    Size::new(
        size.width.saturating_sub(2).min(MAX_WIDTH),
        size.height.saturating_sub(2).min(MAX_HEIGHT),
    )
}

fn footer_lines(notice: Option<&'static str>, searching: bool, width: u16) -> Vec<Line<'static>> {
    let mut lines = notice
        .map(|notice| wrap_notice(notice, width))
        .unwrap_or_default();
    if searching {
        lines.extend([
            Line::from("Enter keep search"),
            Line::from("Esc restore search"),
        ]);
    } else if width < 29 {
        lines.extend([
            Line::from("Space toggle  a all"),
            Line::from("/ search"),
            Line::from("Enter apply Esc cancel"),
        ]);
    } else {
        lines.extend([
            Line::from("Space toggle  a all  / search"),
            Line::from("Enter apply  Esc cancel"),
        ]);
    }
    lines
}

fn wrap_notice(notice: &'static str, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width.max(1));
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in notice.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(Line::styled(
                std::mem::take(&mut current),
                theme::warning_style(),
            ));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(Line::styled(current, theme::warning_style()));
    }
    lines
}

fn visible_candidates(plans: &[EnvironmentPlan], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    plans
        .iter()
        .enumerate()
        .filter_map(|(index, plan)| name(plan).to_lowercase().contains(&query).then_some(index))
        .collect()
}

fn search_line(query: &str, cursor: usize, searching: bool) -> Line<'static> {
    let mut value = query.to_owned();
    if searching {
        value.insert(cursor.min(value.len()), '|');
    }
    Line::from(format!("Search: {value}"))
}

fn column_widths(width: u16) -> (usize, usize) {
    let content_width = usize::from(width.saturating_sub(1));
    let status_width = 9.min(content_width.saturating_sub(5));
    (content_width.saturating_sub(status_width + 1), status_width)
}

fn candidate_line(
    index: usize,
    focused: bool,
    selected: &BTreeSet<usize>,
    plans: &[EnvironmentPlan],
    name_width: usize,
    status_width: usize,
) -> Line<'static> {
    let plan = &plans[index];
    let checkbox = if selected.contains(&index) {
        "[x]"
    } else {
        "[ ]"
    };
    let name_column = fit_text(&format!("{checkbox} {}", name(plan)), name_width);
    let status_text = if matches!(plan.state(), EnvironmentState::ExcludedHcp) {
        "Excluded"
    } else {
        status(plan)
    };
    let style = if focused {
        theme::search_match_style()
    } else {
        theme::body_style()
    };
    Line::from(vec![
        Span::styled(name_column, style),
        Span::raw(" "),
        Span::styled(
            fit_text(status_text, status_width),
            theme::secondary_style(),
        ),
    ])
}

fn fit_text(text: &str, width: usize) -> String {
    if Line::from(text).width() <= width {
        return text.to_owned();
    }
    let limit = width.saturating_sub(1);
    let mut value = String::new();
    let mut start = 0;
    while start < text.len() {
        let end = text_input::next_grapheme_boundary(text, start);
        let candidate = &text[..end];
        if Line::from(candidate).width() > limit {
            break;
        }
        value.push_str(&text[start..end]);
        start = end;
    }
    if width > 0 {
        value.push('…');
    }
    value
}

fn list_viewport(size: Size, notice: Option<&'static str>, searching: bool) -> usize {
    let dimensions = dialog_dimensions(size);
    if dimensions.width < MIN_WIDTH || dimensions.height < MIN_HEIGHT {
        return 0;
    }
    let inner_width = dimensions.width.saturating_sub(2);
    let inner_height = dimensions.height.saturating_sub(2);
    let footer_height =
        u16::try_from(footer_lines(notice, searching, inner_width).len()).unwrap_or(u16::MAX);
    usize::from(inner_height.saturating_sub(2).saturating_sub(footer_height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::environments::{
            Environment, EnvironmentAvailability, EnvironmentIdentity, EnvironmentSession,
        },
        app::execution::Tool,
        ui::test_support::{buffer_text, render_to_buffer},
    };

    fn session(names: &[String]) -> EnvironmentSession {
        EnvironmentSession::new(
            names
                .iter()
                .map(|name| Environment {
                    tool: Tool::Terraform,
                    availability: EnvironmentAvailability::Available(EnvironmentIdentity {
                        directory: std::path::PathBuf::from(format!("/synthetic/{name}")),
                        workspace: "default".to_owned(),
                    }),
                })
                .collect(),
            false,
        )
    }

    fn press(
        dialog: &mut EnvironmentFilterDialog,
        code: KeyCode,
        size: Size,
        plans: &[EnvironmentPlan],
    ) -> Option<EnvironmentFilterResult> {
        dialog.handle_key(KeyEvent::new(code, KeyModifiers::NONE), size, plans)
    }

    fn press_char(
        dialog: &mut EnvironmentFilterDialog,
        character: char,
        size: Size,
        plans: &[EnvironmentPlan],
    ) -> Option<EnvironmentFilterResult> {
        dialog.handle_key(
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
            size,
            plans,
        )
    }

    #[test]
    fn search_changes_candidates_without_changing_hidden_selections() {
        let state = session(&["dev", "prod-eu", "prod-us", "stg"].map(str::to_owned));
        let plans = state.plans();
        let size = Size::new(80, 24);
        let mut dialog = EnvironmentFilterDialog::new(plans, None, 0, size);

        assert!(press(&mut dialog, KeyCode::Char(' '), size, plans).is_none());
        assert!(press(&mut dialog, KeyCode::Char('/'), size, plans).is_none());
        for character in "prod".chars() {
            assert!(press_char(&mut dialog, character, size, plans).is_none());
        }
        assert!(press(&mut dialog, KeyCode::Enter, size, plans).is_none());
        assert_eq!(visible_candidates(plans, &dialog.query), vec![1, 2]);
        assert!(press(&mut dialog, KeyCode::Char(' '), size, plans).is_none());

        assert_eq!(dialog.selected, BTreeSet::from([2, 3]));
        assert_eq!(dialog.query, "prod");
    }

    #[test]
    fn a_clears_search_and_selects_every_environment() {
        let state = session(&["dev", "prod"].map(str::to_owned));
        let plans = state.plans();
        let size = Size::new(80, 24);
        let mut dialog = EnvironmentFilterDialog::new(plans, Some(&[0]), 0, size);
        assert!(press(&mut dialog, KeyCode::Char('/'), size, plans).is_none());
        assert!(press_char(&mut dialog, 'p', size, plans).is_none());
        assert!(press(&mut dialog, KeyCode::Enter, size, plans).is_none());
        assert!(press(&mut dialog, KeyCode::Char('a'), size, plans).is_none());

        assert!(dialog.query.is_empty());
        assert_eq!(dialog.selected, BTreeSet::from([0, 1]));
    }

    #[test]
    fn escape_cancels_search_before_closing_the_filter_dialog() {
        let state = session(&["dev", "prod"].map(str::to_owned));
        let plans = state.plans();
        let size = Size::new(80, 24);
        let mut dialog = EnvironmentFilterDialog::new(plans, Some(&[1]), 1, size);
        assert!(press(&mut dialog, KeyCode::Char('/'), size, plans).is_none());
        assert!(press_char(&mut dialog, 'd', size, plans).is_none());
        assert!(press(&mut dialog, KeyCode::Enter, size, plans).is_none());
        assert_eq!(dialog.query, "d");
        assert!(press(&mut dialog, KeyCode::Char('/'), size, plans).is_none());
        assert!(press_char(&mut dialog, 'e', size, plans).is_none());
        assert!(press(&mut dialog, KeyCode::Esc, size, plans).is_none());

        assert!(!dialog.searching());
        assert_eq!(dialog.query, "d");
        assert!(matches!(
            press(&mut dialog, KeyCode::Esc, size, plans),
            Some(EnvironmentFilterResult::Cancel)
        ));
    }

    #[test]
    fn cancelling_a_no_match_search_restores_the_previous_candidates() {
        let state = session(&["dev", "prod"].map(str::to_owned));
        let plans = state.plans();
        let size = Size::new(80, 24);
        let mut dialog = EnvironmentFilterDialog::new(plans, None, 0, size);

        assert!(press(&mut dialog, KeyCode::Char('/'), size, plans).is_none());
        for character in "dev".chars() {
            assert!(press_char(&mut dialog, character, size, plans).is_none());
        }
        assert!(press(&mut dialog, KeyCode::Enter, size, plans).is_none());
        assert_eq!(visible_candidates(plans, &dialog.query), vec![0]);

        assert!(press(&mut dialog, KeyCode::Char('/'), size, plans).is_none());
        assert!(press_char(&mut dialog, 'x', size, plans).is_none());
        assert_eq!(dialog.notice, Some("No environments match this search."));

        assert!(press(&mut dialog, KeyCode::Esc, size, plans).is_none());

        assert_eq!(dialog.query, "dev");
        assert_eq!(dialog.notice, None);
        assert_eq!(visible_candidates(plans, &dialog.query), vec![0]);
    }

    #[test]
    fn empty_selection_stays_in_the_dialog_until_an_environment_is_selected() {
        let state = session(&["dev", "prod", "stg"].map(str::to_owned));
        let plans = state.plans();
        let size = Size::new(80, 24);
        let mut dialog = EnvironmentFilterDialog::new(plans, None, 0, size);

        for index in 0..plans.len() {
            assert!(press(&mut dialog, KeyCode::Char(' '), size, plans).is_none());
            if index + 1 < plans.len() {
                assert!(press(&mut dialog, KeyCode::Down, size, plans).is_none());
            }
        }
        assert!(press(&mut dialog, KeyCode::Enter, size, plans).is_none());

        assert!(dialog.selected.is_empty());
        assert_eq!(dialog.notice, Some("Select at least one environment."));
    }

    #[test]
    fn page_down_tracks_a_focused_environment_beyond_the_visible_list() {
        let names = (0..12)
            .map(|index| format!("env-{index:02}"))
            .collect::<Vec<_>>();
        let state = session(&names);
        let plans = state.plans();
        let size = Size::new(40, 16);
        let mut dialog = EnvironmentFilterDialog::new(plans, None, 0, size);

        assert_eq!(list_viewport(size, None, false), 8);
        assert_eq!(list_viewport(Size::new(120, 40), None, false), 14);
        assert!(press(&mut dialog, KeyCode::PageDown, size, plans).is_none());
        assert_eq!(dialog.focused, 8);
        assert_eq!(dialog.vertical, 1);
        assert!(press(&mut dialog, KeyCode::PageDown, size, plans).is_none());

        assert_eq!(dialog.focused, 11);
        assert_eq!(dialog.vertical, 4);
    }

    #[test]
    fn resizing_keeps_the_focused_environment_in_the_visible_list() {
        let names = (0..20)
            .map(|index| format!("env-{index:02}"))
            .collect::<Vec<_>>();
        let state = session(&names);
        let plans = state.plans();
        let mut dialog = EnvironmentFilterDialog::new(plans, None, 0, Size::new(160, 60));
        assert!(press(&mut dialog, KeyCode::PageDown, Size::new(160, 60), plans).is_none());
        let text = buffer_text(&render_to_buffer((40, 16), |frame| {
            dialog.render(frame, frame.area(), plans);
        }));

        assert!(text.contains("env-13"), "{text}");
    }

    #[test]
    fn one_environment_rejects_an_empty_filter_and_a_restores_it() {
        let state = session(&["dev".to_owned()]);
        let plans = state.plans();
        let size = Size::new(40, 16);
        let mut dialog = EnvironmentFilterDialog::new(plans, None, 0, size);

        assert!(press(&mut dialog, KeyCode::Char(' '), size, plans).is_none());
        assert!(press(&mut dialog, KeyCode::Enter, size, plans).is_none());
        assert_eq!(dialog.notice, Some("Select at least one environment."));
        assert!(press(&mut dialog, KeyCode::Char('a'), size, plans).is_none());
        assert!(matches!(
            press(&mut dialog, KeyCode::Enter, size, plans),
            Some(EnvironmentFilterResult::Apply(indexes)) if indexes == [0]
        ));
    }

    #[test]
    fn narrow_filter_dialog_shows_search_controls_and_a_scrollbar() {
        let names = (0..12)
            .map(|index| format!("env-{index:02}"))
            .collect::<Vec<_>>();
        let state = session(&names);
        let plans = state.plans();
        let dialog = EnvironmentFilterDialog::new(plans, None, 0, Size::new(40, 16));
        let text = buffer_text(&render_to_buffer((40, 16), |frame| {
            dialog.render(frame, frame.area(), plans);
        }));

        for marker in [
            "Environment filter",
            "Search:",
            "Space toggle",
            "/ search",
            "Enter apply",
            "Esc cancel",
            "┃",
        ] {
            assert!(text.contains(marker), "missing {marker:?} in:\n{text}");
        }
    }

    #[test]
    fn filter_dialog_fits_supported_terminal_sizes_with_large_candidate_lists() {
        let names = (0..20)
            .map(|index| format!("env-{index:02}"))
            .collect::<Vec<_>>();
        let state = session(&names);
        let plans = state.plans();
        for size in [(40, 16), (80, 24), (120, 40), (160, 60)] {
            let dialog = EnvironmentFilterDialog::new(plans, None, 0, Size::new(size.0, size.1));
            let text = buffer_text(&render_to_buffer(size, |frame| {
                dialog.render(frame, frame.area(), plans);
            }));
            for marker in [
                "Environment filter",
                "Search:",
                "Space toggle",
                "/ search",
                "Enter apply",
                "Esc cancel",
                "┃",
            ] {
                assert!(
                    text.contains(marker),
                    "{size:?} missing {marker:?}:\n{text}"
                );
            }
        }
    }

    #[test]
    fn filter_search_footer_explains_search_confirmation_and_cancel() {
        let state = session(&["dev".to_owned(), "prod".to_owned()]);
        let plans = state.plans();
        let size = Size::new(40, 16);
        let mut dialog = EnvironmentFilterDialog::new(plans, None, 0, size);
        assert!(press(&mut dialog, KeyCode::Char('/'), size, plans).is_none());

        let text = buffer_text(&render_to_buffer((40, 16), |frame| {
            dialog.render(frame, frame.area(), plans);
        }));

        assert!(text.contains("Enter keep search"), "{text}");
        assert!(text.contains("Esc restore search"), "{text}");
        assert!(!text.contains("Enter apply"), "{text}");
        assert!(!text.contains("↑↓"), "{text}");
        assert!(!text.contains("PgUp/PgDn"), "{text}");
    }

    #[test]
    fn narrow_search_footer_keeps_notice_and_both_actions_visible() {
        let state = session(&["dev".to_owned(), "prod".to_owned()]);
        let plans = state.plans();
        let size = Size::new(26, 16);
        let mut dialog = EnvironmentFilterDialog::new(plans, None, 0, size);

        let normal_footer = buffer_text(&render_to_buffer((26, 16), |frame| {
            dialog.render(frame, frame.area(), plans);
        }));
        for marker in ["Space toggle", "/ search", "Enter apply Esc cancel"] {
            assert!(
                normal_footer.contains(marker),
                "missing {marker:?}:\n{normal_footer}"
            );
        }

        assert!(press(&mut dialog, KeyCode::Char('/'), size, plans).is_none());
        assert!(press_char(&mut dialog, 'x', size, plans).is_none());

        let text = buffer_text(&render_to_buffer((26, 16), |frame| {
            dialog.render(frame, frame.area(), plans);
        }));

        for marker in [
            "No environments match",
            "this search.",
            "Enter keep search",
            "Esc restore search",
        ] {
            assert!(text.contains(marker), "missing {marker:?}:\n{text}");
        }
        assert_eq!(dialog.notice, Some("No environments match this search."));
        assert_eq!(list_viewport(size, dialog.notice, dialog.searching()), 6);
    }
}
