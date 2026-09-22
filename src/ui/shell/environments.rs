use ratatui::{
    Frame,
    layout::Rect,
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
    pub(crate) body: Rect,
}

pub(crate) fn layout(
    area: Rect,
    state: &EnvironmentSession,
    notice: Option<&str>,
) -> EnvironmentLayout {
    let tabs = Rect::new(area.x, area.y, area.width, area.height.min(1));
    let summary_height = wrapped_height(&summary(state), area.width).min(area.height / 3);
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
        body,
    }
}

impl EnvironmentSelection {
    pub(crate) fn active(&self) -> usize {
        self.raw.unwrap_or(self.column)
    }

    pub(crate) fn adjacent(&self, delta: isize, count: usize) -> usize {
        self.active()
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1))
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

pub(crate) fn summary(state: &EnvironmentSession) -> String {
    let ready = state
        .plans()
        .iter()
        .filter(|plan| plan.review().is_some())
        .count();
    let mut parts = vec![format!("Ready: {ready}/{}", state.plans().len())];
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

pub(crate) fn render_tabs(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &EnvironmentSession,
    selection: &EnvironmentSelection,
) {
    let active = selection.active();
    let labels: Vec<_> = state
        .plans()
        .iter()
        .enumerate()
        .map(|(index, plan)| {
            let production = plan
                .review()
                .is_some_and(|review| review.review().context().is_production() == Some(true));
            format!(
                " {} {}{} ",
                index + 1,
                name(plan),
                if production { " [PROD]" } else { "" }
            )
        })
        .collect();
    let mut first = 0;
    let available = usize::from(area.width.saturating_sub(16));
    while first < active
        && labels[first..=active]
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
    for (index, label) in labels.iter().enumerate().skip(first) {
        let width = Line::from(label.as_str()).width();
        if used + width > usize::from(area.width) && index > active {
            spans.push(Span::raw(" ›"));
            break;
        }
        spans.push(Span::styled(
            label.clone(),
            if index == active {
                theme::search_match_style()
            } else {
                theme::secondary_style()
            },
        ));
        used += width;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn wrapped_height(text: &str, width: u16) -> u16 {
    u16::try_from(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .line_count(width.max(1)),
    )
    .unwrap_or(u16::MAX)
}
