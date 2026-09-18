use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
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
    let content_area = block.inner(area);
    frame.render_widget(block, area);

    let notice_height = u16::from(state.is_revealed_at(now));
    let copy_notice_height = u16::from(state.copy_notice().is_some());
    let context_height = u16::from(state.context.is_some()) * 2;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(notice_height),
            Constraint::Length(context_height),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(copy_notice_height),
            Constraint::Length(1),
        ])
        .split(content_area);

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
                "! Sensitive value revealed                    {remaining}s remaining"
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
    footer::render(
        frame,
        chunks[6],
        vec![Line::from(footer_line(state, now, chunks[6].width))],
    );
}

fn footer_line(state: &ResourceDetailState, now: Instant, width: u16) -> String {
    let reveal = if state.is_revealed_at(now) {
        "r mask now"
    } else if state.can_reveal_selected() {
        "r reveal sensitive value for 10s"
    } else {
        ""
    };
    let prefix = if reveal.is_empty() {
        String::new()
    } else {
        format!("{reveal}   ")
    };
    let copy_controls = match (state.can_copy_resource, state.can_copy_plan) {
        (true, true) => "y resource / Y plan | ",
        (false, true) => "Y plan | ",
        (true, false) => "y resource | ",
        (false, false) => "",
    };
    let controls = if width < 110 {
        format!(
            "{copy_controls}Up/Down/j/k select | Enter expand | [ / ] prev/next | Esc back | q quit"
        )
    } else {
        format!(
            "{copy_controls}Up/Down/j/k select   Enter expand/collapse   PageUp/PageDown scroll   [ / ] prev/next   Esc back   q quit"
        )
    };
    format!("{prefix}{controls}")
}

#[cfg(test)]
mod tests {
    use crate::app::attribution::{SourceLineChange, SourceRange, SourceSide};
    use crate::app::plan::{AttributeChangeKind, ReplacePathSegment};
    use crate::ui::test_support::buffer_text;

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
        assert!(text.contains("Up/Down/j/k select"), "{text}");
        assert!(text.contains("Esc back"), "{text}");
        assert!(text.contains("Enter expand"), "{text}");
        assert!(text.contains("[ / ] prev/next"), "{text}");
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

        let text = buffer_text(&render_at(&mut state, 48, 8, now));

        assert!(
            text.contains("Copy failed: clipboard unavailable."),
            "{text}"
        );
        assert!(text.contains("r mask now"), "{text}");
    }

    #[test]
    fn reveals_only_selected_known_sensitive_attribute_with_warning_and_expiry() {
        let mut state = sensitive_sibling_state();
        select_attribute(&mut state, "password");
        let now = Instant::now();

        let before_reveal = buffer_text(&render(&state, 100, 40));
        assert!(before_reveal.contains("r reveal sensitive value for 10s"));
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
