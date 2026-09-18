use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::copy::{CopyNotice, CopyTarget};
use crate::app::review::{PlanListState, ResourceNavigation, ReviewDetailState};
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{footer, header, layout as shell_layout};
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
    shell: shell_layout::ShellLayout,
    chunks: Vec<Rect>,
}

impl ResourceDetailLayout {
    #[cfg(test)]
    pub(crate) const fn content(&self) -> Rect {
        self.shell.content()
    }

    pub(crate) fn body(&self) -> Rect {
        self.chunks[1]
    }

    #[cfg(test)]
    pub(crate) const fn footer(&self) -> Rect {
        self.shell.footer()
    }
}

pub(crate) fn resource_detail_layout(
    area: Rect,
    list: &PlanListState,
    detail: &ReviewDetailState,
    copy_notice: Option<CopyNotice>,
    now: Instant,
) -> ResourceDetailLayout {
    let footer_lines = footer_lines(list, detail, now, area.width);
    let required_footer_lines = required_footer_lines(detail, now, area.width);
    let shell = shell_layout::layout(area, footer_lines, required_footer_lines, 1);
    let content_area = shell.content_inner();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::from(detail.is_revealed_at(now))),
            Constraint::Min(1),
            Constraint::Length(u16::from(copy_notice.is_some())),
        ])
        .split(content_area)
        .to_vec();
    ResourceDetailLayout { shell, chunks }
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

    let layout = resource_detail_layout(area, list, detail, copy_notice, now);
    let chunks = &layout.chunks;
    if chunks[1].height == 0 {
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    header::render_review(
        frame,
        layout.shell.header(),
        list.context(),
        list.comparison(),
    );
    let content_area = shell_layout::render_content_block(
        frame,
        layout.shell.content(),
        format!(
            "Resource {}/{} | Filter: {}",
            detail.index() + 1,
            detail.total(),
            list.filter().label()
        ),
    );
    debug_assert_eq!(content_area, layout.shell.content_inner());

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

    let content = super::rows::detail_content(
        list,
        detail,
        view.analysis_info_expanded(),
        chunks[1].width,
        now,
    );
    let scroll = view.scroll().min(super::viewport::max_scroll(
        &content,
        chunks[1].width,
        chunks[1].height,
    ));
    frame.render_widget(
        Paragraph::new(content.lines)
            .style(theme::body_style())
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
    if let Some(notice) = copy_notice {
        frame.render_widget(
            Paragraph::new(notice.message()).style(theme::body_style()),
            chunks[2],
        );
    }
    footer::render(
        frame,
        layout.shell.footer(),
        layout.shell.footer_lines().to_owned(),
    );
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
    items.push(footer::hint(&["s"], "analysis info"));
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
    ]);
    if detail.can_navigate(ResourceNavigation::Previous) {
        items.push(footer::hint(&["[", "←", "Ctrl+B"], "prev"));
    }
    if detail.can_navigate(ResourceNavigation::Next) {
        items.push(footer::hint(&["]", "→", "Ctrl+F"], "next"));
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
    items.push(footer::hint(&["s"], "analysis info"));
    footer::layout(items, width)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::app::review::DetailAction;
    use crate::ui::test_support::{assert_shell_frame_and_footer, buffer_text};

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

        assert_eq!(body.height, 5);
    }

    #[test]
    fn keeps_analysis_info_operation_in_footer_when_body_is_narrow() {
        let state = state();
        let text = buffer_text(&render(&state, 48, 30));

        assert!(text.contains("s analysis info"), "{text}");
    }

    #[test]
    fn common_shell_geometry_survives_supported_sizes() {
        let state = state();
        for (width, height) in [(120, 40), (80, 24), (48, 12)] {
            let layout = resource_detail_layout(
                Rect::new(0, 0, width, height),
                &state.list,
                &state.detail,
                state.copy_notice,
                Instant::now(),
            );
            let buffer = render(&state, width, height);
            assert_shell_frame_and_footer(&buffer, layout.content(), layout.footer(), "q quit");
        }

        let small = buffer_text(&render(&state, 47, 7));
        assert!(small.contains("Terminal too small"), "{small}");
    }

    #[test]
    fn resetting_detail_view_closes_analysis_info() {
        let mut state = state();
        state.toggle_analysis_info(100, 40, Instant::now());
        assert!(state.view.analysis_info_expanded());

        state.view.reset();

        assert!(!state.view.analysis_info_expanded());
        assert_eq!(state.scroll(), 0);
    }
}
