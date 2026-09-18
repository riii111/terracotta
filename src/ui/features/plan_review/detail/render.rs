use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::copy::{CopyNotice, CopyTarget};
use crate::app::review::{PlanListState, ReviewDetailState};
use crate::ui::primitives::atoms::separator;
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{footer, header};
use crate::ui::theme;

use super::{DetailViewState, MIN_HEIGHT, MIN_WIDTH};

pub(crate) fn render_resource_detail(
    frame: &mut Frame<'_>,
    list: &PlanListState,
    detail: &ReviewDetailState,
    copy_notice: Option<CopyNotice>,
    view: &DetailViewState,
) {
    render_resource_detail_at(frame, list, detail, copy_notice, view, Instant::now());
}

pub(crate) struct ResourceDetailLayout {
    chunks: Vec<Rect>,
    footer_lines: Vec<Line<'static>>,
}

impl ResourceDetailLayout {
    pub(crate) fn body(&self) -> Rect {
        self.chunks[4]
    }
}

pub(crate) fn resource_detail_layout(
    area: Rect,
    list: &PlanListState,
    detail: &ReviewDetailState,
    copy_notice: Option<CopyNotice>,
    now: Instant,
) -> ResourceDetailLayout {
    let block = Block::new().borders(Borders::ALL);
    let content_area = block.inner(area);
    let mut footer_lines = footer_lines(list, detail, now, content_area.width);
    let required_height = usize::from(u16::from(detail.is_revealed_at(now)))
        + usize::from(u16::from(list.context().is_some()) * 2)
        + 1
        + 1
        + usize::from(u16::from(copy_notice.is_some()));
    if required_height + footer_lines.len() > usize::from(content_area.height) {
        footer_lines = required_footer_lines(detail, now, content_area.width);
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::from(detail.is_revealed_at(now))),
            Constraint::Length(u16::from(list.context().is_some()) * 2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(u16::from(copy_notice.is_some())),
            Constraint::Length(u16::try_from(footer_lines.len()).unwrap_or(u16::MAX).max(1)),
        ])
        .split(content_area)
        .to_vec();
    ResourceDetailLayout {
        chunks,
        footer_lines,
    }
}

pub(super) fn render_resource_detail_at(
    frame: &mut Frame<'_>,
    list: &PlanListState,
    detail: &ReviewDetailState,
    copy_notice: Option<CopyNotice>,
    view: &DetailViewState,
    now: Instant,
) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    let title = format!(
        "Terracotta / Resource {}/{}",
        detail.index() + 1,
        detail.total()
    );
    let block = Block::new().borders(Borders::ALL).title(title);
    frame.render_widget(block, area);

    let layout = resource_detail_layout(area, list, detail, copy_notice, now);
    let chunks = &layout.chunks;
    if chunks[4].height == 0 {
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    if detail.is_revealed_at(now) {
        let remaining = detail
            .reveal()
            .expect("active reveal should have state")
            .expires_at()
            .saturating_duration_since(now)
            .as_secs()
            .max(1);
        frame.render_widget(
            Paragraph::new(format!(
                "! Sensitive value revealed ({remaining}s remaining)"
            ))
            .style(theme::warning_style()),
            chunks[0],
        );
    }
    if let Some(context) = list.context() {
        header::render(
            frame,
            chunks[1],
            vec![
                Line::from(format!("cwd {}", context.root().display())),
                Line::from(format!(
                    "workspace {}   git {}",
                    context.workspace(),
                    context.git()
                )),
            ],
        );
    }
    frame.render_widget(
        Paragraph::new(format!("compare {}", list.comparison())),
        chunks[2],
    );
    frame.render_widget(separator::render(chunks[3].width), chunks[3]);

    let content = super::detail_content(list, detail, view.sources_expanded(), now);
    let scroll = view.scroll().min(super::viewport::max_scroll(
        &content,
        chunks[4].width,
        chunks[4].height,
    ));
    frame.render_widget(
        Paragraph::new(content.lines)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[4],
    );
    if let Some(notice) = copy_notice {
        frame.render_widget(Paragraph::new(notice.message()), chunks[5]);
    }
    footer::render(frame, chunks[6], layout.footer_lines);
}

fn footer_lines(
    list: &PlanListState,
    detail: &ReviewDetailState,
    now: Instant,
    width: u16,
) -> Vec<Line<'static>> {
    let mut items = vec![
        footer::hint(&["q"], "quit"),
        footer::hint(&["Esc"], "back"),
        footer::hint(&["PgUp", "PgDn"], "scroll"),
    ];
    if detail.is_revealed_at(now) {
        items.push(footer::hint(&["r"], "mask now"));
    } else if detail.can_reveal_selected() {
        items.push(footer::hint(&["r"], "reveal 10s"));
    }
    match (
        list.can_copy(CopyTarget::Resource),
        list.can_copy(CopyTarget::Plan),
    ) {
        (true, true) => {
            items.push(footer::hint(&["y"], "resource"));
            items.push(footer::hint(&["Y"], "plan"));
        }
        (true, false) => items.push(footer::hint(&["y"], "resource")),
        (false, true) => items.push(footer::hint(&["Y"], "plan")),
        (false, false) => {}
    }
    items.extend([
        footer::hint(&["↑", "↓"], "select"),
        footer::hint(&["Enter"], "expand"),
        footer::hint(&["[", "]"], "prev/next"),
    ]);
    if !list.source_files().is_empty() {
        items.push(footer::hint(&["s"], "sources"));
    }
    footer::layout(items, width)
}

fn required_footer_lines(
    detail: &ReviewDetailState,
    now: Instant,
    width: u16,
) -> Vec<Line<'static>> {
    let mut items = vec![
        footer::hint(&["q"], "quit"),
        footer::hint(&["Esc"], "back"),
        footer::hint(&["PgUp", "PgDn"], "scroll"),
    ];
    if detail.is_revealed_at(now) {
        items.push(footer::hint(&["r"], "mask now"));
    } else if detail.can_reveal_selected() {
        items.push(footer::hint(&["r"], "reveal 10s"));
    }
    footer::layout(items, width)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::app::review::DetailAction;
    use crate::ui::test_support::buffer_text;

    use super::super::test_support::*;
    use super::*;

    #[test]
    fn renders_app_owned_detail_content_and_keeps_render_read_only() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();
        state.detail.apply(DetailAction::Reveal, now);
        let detail_before = state.detail.clone();
        let view_before = state.view.clone();

        let text = buffer_text(&render_at(&state, 100, 40, now + Duration::from_secs(2)));

        assert!(text.contains("old-secret"), "{text}");
        assert!(text.contains("new-secret"), "{text}");
        assert!(text.contains("Sensitive value revealed"), "{text}");
        assert_eq!(state.detail, detail_before);
        assert_eq!(state.view, view_before);
    }

    #[test]
    fn render_does_not_clear_reveal_for_small_or_expired_area() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();
        state.detail.apply(DetailAction::Reveal, now);

        let small = buffer_text(&render_at(&state, 47, 7, now + Duration::from_secs(1)));
        assert!(small.contains("Terminal too small"), "{small}");
        assert!(state.detail.reveal().is_some());

        let expired = buffer_text(&render_at(&state, 100, 40, now + Duration::from_secs(10)));
        assert!(!expired.contains("old-secret"), "{expired}");
        assert!(!expired.contains("Sensitive value revealed"), "{expired}");
        assert!(state.detail.reveal().is_some());
    }

    #[test]
    fn unknown_sensitive_value_stays_masked_in_rendering() {
        let mut state = unknown_sensitive_state();
        select_attribute(&mut state, "future_secret");
        state.detail.apply(DetailAction::Reveal, Instant::now());

        let text = buffer_text(&render(&state, 100, 40));

        assert!(text.contains("<sensitive>"), "{text}");
        assert!(!text.contains("not-known-yet"), "{text}");
        assert!(state.detail.reveal().is_none());
    }

    #[test]
    fn layout_reserves_copy_notice_above_footer() {
        let mut state = state();
        state.set_copy_notice(CopyNotice::Failed);
        let area = Rect::new(0, 0, 48, 12);
        let body = resource_detail_layout(
            area,
            &state.list,
            &state.detail,
            state.copy_notice,
            Instant::now(),
        )
        .body();

        assert_eq!(body.height, 3);
    }

    #[test]
    fn keeps_source_toggle_operation_in_body_when_footer_is_narrow() {
        let state = state();
        let text = buffer_text(&render(&state, 48, 30));

        assert!(text.contains("Analyzed sources: 1 (s show)"), "{text}");
        assert!(text.contains("s sources"), "{text}");
    }

    #[test]
    fn resetting_detail_view_closes_sources() {
        let mut state = state();
        state.toggle_sources(100, 40, Instant::now());
        assert!(state.view.sources_expanded());

        state.view.reset();

        assert!(!state.view.sources_expanded());
        assert_eq!(state.scroll(), 0);
    }
}
