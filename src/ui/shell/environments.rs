use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::app::environments::{EnvironmentPlan, EnvironmentSession, EnvironmentState};
use crate::ui::theme;

#[derive(Default)]
pub(crate) struct EnvironmentSelection {
    pub(crate) column: usize,
    pub(crate) raw: Option<usize>,
}

pub(crate) struct EnvironmentLayout {
    pub(crate) tabs: Rect,
    pub(crate) summary: Rect,
    pub(crate) notice: Rect,
    pub(crate) header_separator: Rect,
    pub(crate) body: Rect,
    pub(crate) ready_on_tabs: bool,
}

pub(crate) fn layout(
    area: Rect,
    state: &EnvironmentSession,
    notice: Option<&str>,
    filter_active: bool,
    show_header_separator: bool,
) -> EnvironmentLayout {
    let tabs = Rect::new(area.x, area.y, area.width, area.height.min(1));
    let summary_height =
        wrapped_height(&summary(state, filter_active), area.width).min(area.height / 3);
    let summary = Rect::new(area.x, tabs.bottom(), area.width, summary_height);
    let remaining = area.bottom().saturating_sub(summary.bottom());
    let notice_height = notice
        .map_or(0, |text| wrapped_height(text, area.width))
        .min(remaining / 3);
    let notice = Rect::new(area.x, summary.bottom(), area.width, notice_height);
    let header_separator = Rect::new(
        area.x,
        notice.bottom(),
        area.width,
        u16::from(show_header_separator),
    );
    let body = Rect::new(
        area.x,
        header_separator.bottom(),
        area.width,
        area.bottom().saturating_sub(header_separator.bottom()),
    );
    EnvironmentLayout {
        tabs,
        summary,
        notice,
        header_separator,
        body,
        ready_on_tabs: false,
    }
}

pub(crate) fn overview_layout(
    area: Rect,
    state: &EnvironmentSession,
    notice: Option<&str>,
    filter_active: bool,
    selection: &EnvironmentSelection,
    visible_environments: &[usize],
) -> EnvironmentLayout {
    let tabs = Rect::new(area.x, area.y, area.width, area.height.min(1));
    let ready = ready_summary(state);
    let tabs_width = overview_tabs_width(state, selection, visible_environments);
    let ready_on_tabs = tabs_width
        .saturating_add(Line::from(ready.as_str()).width())
        .saturating_add(2)
        <= usize::from(area.width);
    let summary_text = overview_summary(state, filter_active, ready_on_tabs);
    let summary_height = if summary_text.is_empty() {
        0
    } else {
        wrapped_height(&summary_text, area.width).min(area.height / 3)
    };
    let summary = Rect::new(area.x, tabs.bottom(), area.width, summary_height);
    let remaining = area.bottom().saturating_sub(summary.bottom());
    let notice_height = notice
        .map_or(0, |text| wrapped_height(text, area.width))
        .min(remaining / 3);
    let notice = Rect::new(area.x, summary.bottom(), area.width, notice_height);
    let body = Rect::new(
        area.x,
        notice.bottom(),
        area.width,
        area.bottom().saturating_sub(notice.bottom()),
    );
    EnvironmentLayout {
        tabs,
        summary,
        notice,
        header_separator: Rect::default(),
        body,
        ready_on_tabs,
    }
}

impl EnvironmentSelection {
    pub(crate) fn active(&self) -> usize {
        self.raw.unwrap_or(self.column)
    }
}

pub(crate) fn name(plan: &EnvironmentPlan) -> String {
    plan.workspace()
        .filter(|name| *name != "default")
        .map_or_else(
            || {
                plan.directory()
                    .file_name()
                    .unwrap_or_else(|| plan.directory().as_os_str())
                    .to_string_lossy()
                    .into_owned()
            },
            str::to_owned,
        )
}

pub(crate) fn context(plan: &EnvironmentPlan) -> String {
    format!(
        "{}   ws:{}\nDirectory: {}",
        plan.tool.display_name(),
        plan.workspace().unwrap_or("unavailable"),
        plan.directory().display()
    )
}

pub(crate) const fn status(plan: &EnvironmentPlan) -> &'static str {
    match plan.state() {
        EnvironmentState::Pending => "Pending",
        EnvironmentState::Running => "Running",
        EnvironmentState::Ready { .. } => "Ready",
        EnvironmentState::Error => "Error",
        EnvironmentState::ExcludedHcp => "Excluded: HCP execution",
    }
}

pub(crate) fn summary(state: &EnvironmentSession, filter_active: bool) -> String {
    let ready = state
        .plans()
        .iter()
        .filter(|plan| plan.review().is_some())
        .count();
    let mut parts = vec![ready_summary(state)];
    if filter_active {
        if state
            .plans()
            .iter()
            .any(|plan| matches!(plan.state(), EnvironmentState::Error))
        {
            parts.push("Error present".to_owned());
        }
        parts.push("[Env filter ON]".to_owned());
        return parts.join("   ");
    }
    if ready < state.plans().len() {
        let names: Vec<_> = state
            .plans()
            .iter()
            .filter(|plan| plan.review().is_some())
            .map(name)
            .collect();
        parts.push(format!(
            "Compared: {}",
            if names.is_empty() {
                "none".to_owned()
            } else {
                names.join(", ")
            }
        ));
        for plan in state.plans().iter().filter(|plan| plan.review().is_none()) {
            parts.push(format!("{}: {}", name(plan), status(plan)));
        }
    }
    parts.join("   ")
}

fn ready_summary(state: &EnvironmentSession) -> String {
    let ready = state
        .plans()
        .iter()
        .filter(|plan| plan.review().is_some())
        .count();
    format!("Ready: {ready}/{}", state.plans().len())
}

fn overview_summary(
    state: &EnvironmentSession,
    filter_active: bool,
    ready_on_tabs: bool,
) -> String {
    let summary = summary(state, filter_active);
    if !ready_on_tabs {
        return summary;
    }
    let ready = ready_summary(state);
    summary
        .strip_prefix(&ready)
        .unwrap_or(&summary)
        .trim_start()
        .trim_start_matches("   ")
        .to_owned()
}

fn overview_tabs_width(
    state: &EnvironmentSession,
    selection: &EnvironmentSelection,
    visible_environments: &[usize],
) -> usize {
    let mut width = Line::from("0 Overview  ").width();
    for (position, index) in visible_environments.iter().enumerate() {
        let plan = &state.plans()[*index];
        let production = plan
            .review()
            .is_some_and(|review| review.review().context().is_production() == Some(true));
        width = width.saturating_add(
            Line::from(
                format!(
                    " {} {}{} ",
                    position + 1,
                    name(plan),
                    if production { " [PROD]" } else { "" }
                )
                .as_str(),
            )
            .width(),
        );
    }
    let active = selection.active();
    let visible_position = visible_environments
        .iter()
        .position(|index| *index == active)
        .unwrap_or(0);
    if visible_position > 0 {
        width = width.saturating_add(2);
    }
    if visible_position + 1 < visible_environments.len() {
        width = width.saturating_add(2);
    }
    width
}

pub(crate) fn render_tabs(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &EnvironmentSession,
    selection: &EnvironmentSelection,
    visible_environments: &[usize],
) {
    let active = selection.active();
    let labels: Vec<_> = visible_environments
        .iter()
        .enumerate()
        .map(|(position, index)| {
            let plan = &state.plans()[*index];
            let production = plan
                .review()
                .is_some_and(|review| review.review().context().is_production() == Some(true));
            format!(
                " {} {}{} ",
                position + 1,
                name(plan),
                if production { " [PROD]" } else { "" }
            )
        })
        .collect();
    let mut first = 0;
    let active_position = visible_environments
        .iter()
        .position(|index| *index == active)
        .unwrap_or(0);
    let available = usize::from(area.width.saturating_sub(16));
    while first < active_position
        && labels[first..=active_position]
            .iter()
            .map(|label| Line::from(label.as_str()).width())
            .sum::<usize>()
            > available
    {
        first += 1;
    }
    let mut spans = vec![Span::styled(
        "0 Overview  ",
        if selection.raw.is_none() {
            theme::search_match_style()
        } else {
            theme::body_style()
        },
    )];
    if first > 0 {
        spans.push(Span::raw("‹ "));
    }
    let mut used = spans.iter().map(Span::width).sum::<usize>();
    for (position, label) in labels.iter().enumerate().skip(first) {
        let width = Line::from(label.as_str()).width();
        if used + width > usize::from(area.width) && position > active_position {
            spans.push(Span::raw(" ›"));
            break;
        }
        spans.push(Span::styled(
            label.clone(),
            if selection.raw.is_some() && visible_environments[position] == active {
                theme::search_match_style()
            } else {
                theme::secondary_style()
            },
        ));
        used += width;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub(crate) fn render_overview_header(
    frame: &mut Frame<'_>,
    layout: &EnvironmentLayout,
    state: &EnvironmentSession,
    selection: &EnvironmentSelection,
    visible_environments: &[usize],
    filter_active: bool,
) {
    let labels: Vec<_> = visible_environments
        .iter()
        .enumerate()
        .map(|(position, index)| {
            let plan = &state.plans()[*index];
            let production = plan
                .review()
                .is_some_and(|review| review.review().context().is_production() == Some(true));
            format!(
                " {} {}{} ",
                position + 1,
                name(plan),
                if production { " [PROD]" } else { "" }
            )
        })
        .collect::<Vec<_>>();
    let active_position = visible_environments
        .iter()
        .position(|index| *index == selection.active())
        .unwrap_or(0);
    let available = usize::from(layout.tabs.width.saturating_sub(16));
    let mut first = 0;
    while first < active_position
        && labels[first..=active_position]
            .iter()
            .map(|label| Line::from(label.as_str()).width())
            .sum::<usize>()
            > available
    {
        first += 1;
    }
    let mut spans = vec![Span::styled(
        "0 Overview  ",
        theme::overview_header_accent_style().add_modifier(Modifier::BOLD),
    )];
    if first > 0 {
        spans.push(Span::styled("‹ ", theme::overview_header_muted_style()));
    }
    let mut used = spans.iter().map(Span::width).sum::<usize>();
    for (position, label) in labels.iter().enumerate().skip(first) {
        let width = Line::from(label.as_str()).width();
        if used + width > usize::from(layout.tabs.width) && position > active_position {
            spans.push(Span::styled(" ›", theme::overview_header_muted_style()));
            break;
        }
        spans.push(Span::styled(
            label.clone(),
            theme::overview_header_muted_style(),
        ));
        used += width;
    }
    if layout.ready_on_tabs {
        let ready = ready_summary(state);
        let padding = usize::from(layout.tabs.width)
            .saturating_sub(used.saturating_add(Line::from(ready.as_str()).width()));
        spans.push(Span::styled(
            " ".repeat(padding),
            theme::overview_header_style(),
        ));
        spans.push(Span::styled(ready, theme::overview_header_muted_style()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::overview_header_style()),
        layout.tabs,
    );

    let summary = overview_summary(state, filter_active, layout.ready_on_tabs);
    if !summary.is_empty() {
        frame.render_widget(
            Paragraph::new(summary)
                .wrap(Wrap { trim: false })
                .style(theme::overview_header_muted_style()),
            layout.summary,
        );
    }
}

fn wrapped_height(text: &str, width: u16) -> u16 {
    u16::try_from(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .line_count(width.max(1)),
    )
    .unwrap_or(u16::MAX)
}
