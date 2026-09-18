use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::ui::primitives::atoms::separator;
use crate::ui::primitives::molecules::terminal_notice;
use crate::ui::shell::{footer, header};
use crate::ui::theme;

use super::{MIN_HEIGHT, MIN_WIDTH, ResourceDetailState};

pub(crate) fn render_resource_detail(frame: &mut Frame<'_>, state: &mut ResourceDetailState) {
    render_resource_detail_at(frame, state, Instant::now());
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
    state: &ResourceDetailState,
    now: Instant,
) -> ResourceDetailLayout {
    let block = Block::new().borders(Borders::ALL);
    let content_area = block.inner(area);
    let mut footer_lines = footer_lines(state, now, content_area.width);
    let required_height = usize::from(u16::from(state.is_revealed_at(now)))
        + usize::from(u16::from(state.context.is_some()) * 2)
        + 1
        + 1
        + usize::from(u16::from(state.copy_notice().is_some()));
    if required_height + footer_lines.len() > usize::from(content_area.height) {
        footer_lines = required_footer_lines(state, now, content_area.width);
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::from(state.is_revealed_at(now))),
            Constraint::Length(u16::from(state.context.is_some()) * 2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(u16::from(state.copy_notice().is_some())),
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
    state: &mut ResourceDetailState,
    now: Instant,
) {
    state.clear_expired_reveal(now);
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        state.mask_reveal();
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    let title = format!(
        "Terracotta / Resource {}/{}",
        state.item_index() + 1,
        state.total_items()
    );
    let block = Block::new().borders(Borders::ALL).title(title);
    frame.render_widget(block, area);

    let layout = resource_detail_layout(area, state, now);
    let chunks = &layout.chunks;

    if chunks[4].height == 0 {
        state.mask_reveal();
        terminal_notice::render(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }

    if state.is_revealed_at(now) {
        let remaining = state
            .reveal
            .as_ref()
            .expect("active reveal should have state")
            .expires_at
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
    if let Some(context) = &state.context {
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
        Paragraph::new(format!("compare {}", state.comparison)),
        chunks[2],
    );
    frame.render_widget(separator::render(chunks[3].width), chunks[3]);

    let content = super::detail_content(state, now);
    let scroll = state.scroll().min(super::max_scroll(
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
    if let Some(notice) = state.copy_notice() {
        frame.render_widget(Paragraph::new(notice.message()), chunks[5]);
    }
    footer::render(frame, chunks[6], layout.footer_lines);
}

fn footer_lines(state: &ResourceDetailState, now: Instant, width: u16) -> Vec<Line<'static>> {
    let mut items = vec![
        Line::from("q quit"),
        Line::from("Esc back"),
        Line::from("PgUp/PgDn scroll"),
    ];
    if state.is_revealed_at(now) {
        items.push(Line::from("r mask now"));
    } else if state.can_reveal_selected() {
        items.push(Line::from("r reveal 10s"));
    }
    match (state.can_copy_resource, state.can_copy_plan) {
        (true, true) => {
            items.push(Line::from("y resource"));
            items.push(Line::from("Y plan"));
        }
        (true, false) => items.push(Line::from("y resource")),
        (false, true) => items.push(Line::from("Y plan")),
        (false, false) => {}
    }
    items.extend([
        Line::from("↑/↓ select"),
        Line::from("Enter expand"),
        Line::from("[/] prev/next"),
    ]);
    footer::layout(items, width)
}

fn required_footer_lines(
    state: &ResourceDetailState,
    now: Instant,
    width: u16,
) -> Vec<Line<'static>> {
    let mut items = vec![
        Line::from("q quit"),
        Line::from("Esc back"),
        Line::from("PgUp/PgDn scroll"),
    ];
    if state.is_revealed_at(now) {
        items.push(Line::from("r mask now"));
    } else if state.can_reveal_selected() {
        items.push(Line::from("r reveal 10s"));
    }
    footer::layout(items, width)
}

#[cfg(test)]
mod tests {
    use crate::app::attribution::{SourceLineChange, SourceRange, SourceSide};
    use crate::app::plan::{AttributeChangeKind, ReplacePathSegment};
    use crate::ui::test_support::buffer_text;
    use ratatui::layout::Rect;

    use super::super::test_support::*;
    use super::super::*;

    #[test]
    fn renders_diff_evidence_masks_special_values_and_omits_future_controls() {
        let state = state();
        let text = buffer_text(&render(&state, 100, 40));

        assert!(text.contains("Terracotta / Resource 1/1"), "{text}");
        assert!(text.contains("Git: no match"), "{text}");
        assert!(
            text.contains("No direct match in analyzed sources."),
            "{text}"
        );
        assert!(text.contains("Analyzed sources:"), "{text}");
        assert!(text.contains("main.tf (after)"), "{text}");
        assert!(text.contains("instance_type"), "{text}");
        assert!(text.contains("t3.small"), "{text}");
        assert!(text.contains("t3.medium"), "{text}");
        assert!(text.contains("private_ip"), "{text}");
        assert!(text.contains("<unknown>"), "{text}");
        assert!(text.contains("password"), "{text}");
        assert!(text.contains("<sensitive>"), "{text}");
        assert!(text.contains("Replacement triggered by:"), "{text}");
        assert!(text.contains("  instance_type"), "{text}");
        assert!(
            text.contains("Replacement reason: replace_because_cannot_update"),
            "{text}"
        );
        assert!(text.contains("↑/↓ select"), "{text}");
        assert!(text.contains("Esc back"), "{text}");
        assert!(text.contains("Enter expand"), "{text}");
        assert!(text.contains("[/] prev/next"), "{text}");
        assert!(!text.contains("reveal"), "{text}");
    }

    #[test]
    fn renders_quoted_replacement_paths_without_exposing_sensitive_values() {
        let mut change = change();
        change.replace_paths = Some(vec![vec![
            ReplacePathSegment::Attribute("tags".to_owned()),
            ReplacePathSegment::Attribute("service.name".to_owned()),
            ReplacePathSegment::Index(u64::MAX),
        ]]);

        let state = state_for_change(change, &[]);
        let text = buffer_text(&render(&state, 100, 40));

        assert!(
            text.contains("tags[\"service.name\"][18446744073709551615]"),
            "{text}"
        );
        assert!(text.contains("<sensitive>"), "{text}");
        assert!(!text.contains("old-secret"), "{text}");
        assert!(!text.contains("new-secret"), "{text}");
    }

    #[test]
    fn copy_notice_gets_its_own_row_while_reveal_is_active() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();
        state.apply_at(DetailAction::Reveal, 96, 40, now);
        state.set_copy_notice(CopyNotice::Failed);

        let text = buffer_text(&render_at(&mut state, 48, 11, now));

        assert!(
            text.contains("Copy failed: clipboard unavailable."),
            "{text}"
        );
        assert!(text.contains("r mask now"), "{text}");
    }

    #[test]
    fn renders_success_and_failure_copy_notices_above_the_footer() {
        let cases = [
            (
                "success",
                CopyNotice::Copied {
                    target: CopyTarget::Resource,
                    resource_count: 1,
                },
                "Copied selected resource (redacted).",
            ),
            (
                "failure",
                CopyNotice::Failed,
                "Copy failed: clipboard unavailable.",
            ),
        ];

        for (name, notice, expected) in cases {
            let mut state = state();
            state.set_copy_notice(notice);
            let text = buffer_text(&render_at(&mut state, 80, 20, Instant::now()));

            assert!(text.contains(expected), "case: {name}\n{text}");
            assert!(text.contains("q quit"), "case: {name}\n{text}");
            assert!(text.contains("Esc back"), "case: {name}\n{text}");
        }
    }

    #[test]
    fn detail_widths_keep_exit_back_and_page_scroll_hints() {
        for width in [48, 60, 80, 120] {
            let state = state();
            let text = buffer_text(&render(&state, width, 20));

            assert!(text.contains("q quit"), "width: {width}\n{text}");
            assert!(text.contains("Esc back"), "width: {width}\n{text}");
            assert!(text.contains("PgUp/PgDn scroll"), "width: {width}\n{text}");
        }
    }

    #[test]
    fn detail_layout_reserves_notice_rows_and_drives_page_scroll() {
        let now = Instant::now();
        let area = Rect::new(0, 0, 48, 12);
        let mut revealed = sensitive_sibling_state();
        select_attribute(&mut revealed, "password");
        revealed.apply_at(DetailAction::Reveal, 46, 4, now);

        let mut cases = [
            ("normal", state(), 4),
            ("reveal", revealed, 3),
            ("copy", state(), 3),
            ("reveal and copy", sensitive_sibling_state(), 2),
        ];
        cases[2].1.set_copy_notice(CopyNotice::Failed);
        select_attribute(&mut cases[3].1, "password");
        cases[3].1.apply_at(DetailAction::Reveal, 46, 4, now);
        cases[3].1.set_copy_notice(CopyNotice::Failed);

        for (name, mut state, expected_height) in cases {
            let body = resource_detail_layout(area, &state, now).body();
            assert_eq!(body.height, expected_height, "case: {name}");

            state.apply_at(DetailAction::PageDown, body.width, body.height, now);
            assert_eq!(state.scroll(), body.height, "case: {name}");
            for _ in 0..100 {
                state.apply_at(DetailAction::PageDown, body.width, body.height, now);
            }
            let max_scroll = state.scroll();
            state.apply_at(DetailAction::PageDown, body.width, body.height, now);
            assert_eq!(state.scroll(), max_scroll, "case: {name}");
            state.apply_at(DetailAction::PageUp, body.width, body.height, now);
            assert_eq!(
                state.scroll(),
                max_scroll.saturating_sub(body.height),
                "case: {name}"
            );
        }

        let small = buffer_text(&render_at(&mut state(), 48, 7, now));
        assert!(small.contains("Terminal too small"), "{small}");
        assert_eq!(
            resource_detail_layout(Rect::new(0, 0, 48, 7), &state(), now)
                .body()
                .height,
            1
        );
    }

    #[test]
    fn reveals_only_selected_known_sensitive_attribute_with_warning_and_expiry() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();

        let before_reveal = buffer_text(&render(&state, 100, 40));
        assert!(before_reveal.contains("r reveal 10s"));
        assert!(!before_reveal.contains("old-secret"));

        state.apply_at(DetailAction::Reveal, 96, 40, now);
        let revealed = buffer_text(&render_at(
            &mut state,
            100,
            40,
            now + Duration::from_secs(2),
        ));
        assert!(revealed.contains("old-secret"), "{revealed}");
        assert!(revealed.contains("new-secret"), "{revealed}");
        assert!(revealed.contains("<sensitive>"), "{revealed}");
        assert!(!revealed.contains("old-token"), "{revealed}");
        assert!(!revealed.contains("new-token"), "{revealed}");
        assert!(revealed.contains("Sensitive value revealed"), "{revealed}");
        assert!(revealed.contains("8s remaining"), "{revealed}");
        assert!(revealed.contains("r mask now"), "{revealed}");

        let expired = buffer_text(&render_at(&mut state, 100, 40, now + REVEAL_DURATION));
        assert!(expired.contains("<sensitive>"), "{expired}");
        assert!(!expired.contains("old-secret"), "{expired}");
        assert!(!expired.contains("Sensitive value revealed"), "{expired}");
        assert!(state.reveal.is_none());
    }

    #[test]
    fn unknown_sensitive_attribute_cannot_start_reveal() {
        let mut state = unknown_sensitive_state();
        select_attribute(&mut state, "future_secret");
        let now = Instant::now();

        state.apply_at(DetailAction::Reveal, 96, 40, now);
        let text = buffer_text(&render_at(&mut state, 100, 40, now));
        assert!(text.contains("<sensitive>"), "{text}");
        assert!(!text.contains("not-known-yet"), "{text}");
        assert!(!text.contains("Sensitive value revealed"), "{text}");
        assert!(state.reveal.is_none());
    }

    #[test]
    fn small_terminal_masks_active_reveal_before_normal_rendering_resumes() {
        let mut state = state();
        select_attribute(&mut state, "password");
        let now = Instant::now();
        state.apply_at(DetailAction::Reveal, 96, 40, now);

        let small = buffer_text(&render_at(&mut state, 47, 7, now + Duration::from_secs(1)));
        assert!(small.contains("Terminal too small"), "{small}");
        assert!(state.reveal.is_none());

        let normal = buffer_text(&render_at(
            &mut state,
            100,
            40,
            now + Duration::from_secs(2),
        ));
        assert!(!normal.contains("old-secret"), "{normal}");
        assert!(normal.contains("<sensitive>"), "{normal}");
    }

    #[test]
    fn narrow_terminal_keeps_resize_notice_unwrapped() {
        let mut state = state();
        let text = buffer_text(&render_at(&mut state, 45, 7, Instant::now()));

        assert!(text.contains("Terminal too small. Resize or press q to"));
        assert!(!text.contains("quit."));
    }

    #[test]
    fn renders_direct_evidence_with_its_source_side_and_range() {
        let state = state_with_changed_lines(&[SourceLineChange::new(
            "main.tf",
            SourceSide::After,
            SourceRange::new(43, 44),
        )]);
        let text = buffer_text(&render(&state, 100, 24));

        assert!(text.contains("Git: direct"), "{text}");
        assert!(text.contains("main.tf:42-46 (after)"), "{text}");
        assert!(
            text.contains("Resource block overlaps changed lines."),
            "{text}"
        );
        assert!(!text.contains("No direct match"), "{text}");
    }

    #[test]
    fn reveals_sensitive_children_when_nested_group_is_selected() {
        let mut state = state_for_change(expansion_change(), &[]);
        let group = AttributeGroup::Nested {
            kind: AttributeChangeKind::Changed,
            path: vec![AttributePathSegment::Key("group_b".to_owned())],
        };
        select_group(&mut state, &group);
        state.apply_at(DetailAction::ToggleExpansion, 96, 40, Instant::now());
        let now = Instant::now();

        assert!(buffer_text(&render_at(&mut state, 100, 60, now)).contains("r reveal"));
        state.apply_at(DetailAction::Reveal, 96, 40, now);
        let text = buffer_text(&render_at(&mut state, 100, 60, now));

        assert!(text.contains("old-secret"), "{text}");
        assert!(text.contains("new-secret"), "{text}");
        assert!(text.contains("Sensitive value revealed"), "{text}");
    }

    #[test]
    fn detail_wraps_long_values_and_paths_instead_of_truncating_them() {
        let state = state();
        let text = buffer_text(&render(&state, 60, 32));

        assert!(
            text.contains("another-value-that-is-long-enough-to-wrap"),
            "{text}"
        );
        assert!(!text.contains("..."), "{text}");
    }
}
