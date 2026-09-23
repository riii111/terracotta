use std::time::Instant;

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::app::{copy::CopyNotice, review::PlanReview, session::OverviewSessionState};
use crate::ui::{
    primitives::{
        atoms::{scrollbar, separator},
        molecules::{help_dialog, terminal_notice},
    },
    shell::{context, footer, header, layout as shell_layout},
    theme,
};

use super::{OverviewContent, OverviewOverlay, OverviewViewState};

const MIN_WIDTH: u16 = 24;
const MIN_HEIGHT: u16 = 6;

pub(crate) struct OverviewLayout {
    shell: shell_layout::ShellLayout,
    status: Rect,
    separator: Rect,
    body: Rect,
    max_vertical: u16,
}

struct PreparedOverview {
    layout: OverviewLayout,
    lines: Vec<Line<'static>>,
}

impl OverviewLayout {
    pub(crate) const fn status(&self) -> Rect {
        self.status
    }

    pub(crate) const fn separator(&self) -> Rect {
        self.separator
    }

    pub(crate) const fn body(&self) -> Rect {
        self.body
    }

    pub(crate) const fn max_vertical(&self) -> u16 {
        self.max_vertical
    }
}

pub(crate) fn layout(
    area: Rect,
    state: &OverviewSessionState,
    view: &OverviewViewState,
) -> OverviewLayout {
    prepare(area, state, view).layout
}

fn prepare(area: Rect, state: &OverviewSessionState, view: &OverviewViewState) -> PreparedOverview {
    let content = OverviewContent::from_review(state.review(), view.filter(), view.expanded());
    let footer_message = state.copy_feedback().notice().map(CopyNotice::message);
    let full_footer = footer::layout_with_notice(
        footer_items(
            view.searching(),
            !view.filter().is_empty(),
            view.selected_group_expanded(&content),
        ),
        area.width,
        footer_message,
    );
    let required_footer = footer::layout_with_notice(
        required_footer_items(view.searching()),
        area.width,
        footer_message,
    );
    let shell = shell_layout::full_width_layout(area, full_footer, required_footer);
    let inner = shell.content_inner();
    let status = Rect::new(inner.x, inner.y, inner.width, 1);
    let separator = Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1);
    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        inner.height.saturating_sub(2),
    );
    let lines = overview_lines(&content, state.review(), view, body.width);
    let line_count = lines.len();
    let max_vertical =
        u16::try_from(line_count.saturating_sub(usize::from(body.height))).unwrap_or(u16::MAX);
    PreparedOverview {
        layout: OverviewLayout {
            shell,
            status,
            separator,
            body,
            max_vertical,
        },
        lines,
    }
}

pub(crate) fn render(
    frame: &mut Frame<'_>,
    state: &OverviewSessionState,
    view: &OverviewViewState,
    now: Instant,
) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        terminal_notice::render_wrapped(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }
    let PreparedOverview {
        layout,
        lines: prepared_lines,
    } = prepare(area, state, view);
    if layout.body().width == 0 || layout.body().height == 0 {
        terminal_notice::render_wrapped(
            frame,
            area,
            "Terminal too small. Resize or press q to quit.",
        );
        return;
    }
    header::render_review(frame, layout.shell.header(), state.review());
    frame.render_widget(
        Block::new().style(theme::body_style()),
        layout.shell.content(),
    );
    frame.render_widget(
        Paragraph::new(status_line(state.review(), view)).style(theme::body_style()),
        layout.status(),
    );
    frame.render_widget(
        separator::render(layout.separator().width),
        layout.separator(),
    );

    let lines = if state.copy_feedback().flash_active(now) {
        copy_flash_lines(prepared_lines)
    } else {
        prepared_lines
    };
    let line_count = lines.len();
    let vertical = view.scroll().min(layout.max_vertical());
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .scroll((vertical, 0)),
        layout.body(),
    );
    if line_count > usize::from(layout.body().height) {
        scrollbar::render_vertical(
            frame,
            layout.body(),
            line_count,
            usize::from(layout.body().height),
            usize::from(vertical),
        );
    }
    let notice = state.copy_feedback().notice_at(now).map(|notice| {
        (
            notice.message(),
            if matches!(notice, CopyNotice::Failed) {
                theme::error_style()
            } else {
                theme::accent_style()
            },
        )
    });
    footer::render(
        frame,
        layout.shell.footer(),
        layout.shell.footer_lines(),
        notice,
    );
    frame.render_widget(
        separator::render(layout.shell.footer_separator().width),
        layout.shell.footer_separator(),
    );
    render_overlay(frame, area, state.review(), view);
}

fn status_line(review: &PlanReview, view: &OverviewViewState) -> Line<'static> {
    let metadata = review.metadata();
    let mut spans = vec![
        Span::styled(
            format!(
                "Overview  +{} add  ~{} update  {} replace  -{} destroy",
                metadata.additions(),
                metadata.changes(),
                metadata.replacements(),
                metadata.deletions()
            ),
            theme::body_style(),
        ),
        Span::styled(
            format!(
                "  Repeated: {}",
                review
                    .plan()
                    .grouped_changes(review.provider_schemas())
                    .repeated
            ),
            theme::secondary_style(),
        ),
    ];
    if view.searching() {
        spans.extend([
            Span::styled("  Filter: /", theme::secondary_style()),
            Span::styled(
                view.search_query().unwrap_or_default().to_owned(),
                theme::body_style(),
            ),
        ]);
    } else if !view.filter().is_empty() {
        spans.extend([
            Span::styled("  Filter: ", theme::secondary_style()),
            Span::styled(view.filter().to_owned(), theme::body_style()),
            Span::styled(" (display only)", theme::secondary_style()),
        ]);
    }
    Line::from(spans)
}

fn overview_lines(
    content: &OverviewContent,
    review: &PlanReview,
    view: &OverviewViewState,
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("  Change ", theme::secondary_style()),
        Span::styled("Address", theme::secondary_style()),
    ])];
    if content.unsupported > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "Other changes: {} output/import/move or unsupported change(s). Press v for the full plan.",
                content.unsupported
            ),
            theme::warning_style(),
        )));
    }
    if content.rows.is_empty() {
        lines.push(Line::from(Span::styled(
            if review.metadata().has_changes() {
                "No matching resource changes. Press v for the full plan."
            } else {
                "No resource changes to summarize. Press v for the full plan."
            },
            theme::secondary_style(),
        )));
        return lines;
    }
    for (index, row) in content.rows.iter().enumerate() {
        let selected = view.selected() == Some(index);
        let marker = if selected { ">" } else { " " };
        let indent = if row.child { "  " } else { "" };
        let expansion = if row.member_index.is_none() && row.count > 1 {
            if view.expanded().contains(&row.group_index) {
                "[-]"
            } else {
                "[+]"
            }
        } else {
            "   "
        };
        let action = if row.count > 1 && row.member_index.is_none() {
            format!("{} x{}", row.action, row.count)
        } else {
            row.action.clone()
        };
        let prefix_width = marker.len() + 1 + indent.len() + expansion.len() + 1 + 6;
        let address = truncate_address(
            &format!("{indent}{expansion} {}", row.display_address),
            width as usize,
            prefix_width,
        );
        let base = if selected {
            theme::search_match_style()
        } else {
            theme::body_style()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), base),
            Span::styled(
                format!("{action:<6} "),
                if selected {
                    base
                } else {
                    action_style(&row.action)
                },
            ),
            Span::styled(address, base),
        ]));
    }
    lines
}

fn action_style(action: &str) -> Style {
    if action.starts_with('+') {
        theme::success_style()
    } else if action.starts_with('-') {
        theme::error_style()
    } else if action.starts_with('~') {
        theme::warning_style()
    } else {
        theme::secondary_style()
    }
}

fn truncate_address(value: &str, width: usize, reserved: usize) -> String {
    let available = width.saturating_sub(reserved).max(1);
    if value.chars().count() <= available {
        return value.to_owned();
    }
    if available <= 3 {
        return value
            .chars()
            .rev()
            .take(available)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
    }
    let suffix = value
        .chars()
        .rev()
        .take(available - 3)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("...{suffix}")
}

fn copy_flash_lines(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| Line::from(Span::styled(line.to_string(), theme::copy_flash_style())))
        .collect()
}

fn footer_items(searching: bool, filtered: bool, expanded: Option<bool>) -> Vec<Line<'static>> {
    if searching {
        vec![
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "cancel"),
        ]
    } else if filtered {
        let mut items = vec![
            footer::hint(&["Esc"], "clear / edit"),
            footer::hint(&["Enter"], "open raw"),
        ];
        if let Some(expanded) = expanded {
            items.push(footer::hint(
                &["Space"],
                if expanded { "collapse" } else { "expand" },
            ));
        }
        items.extend([
            footer::hint(&["v"], "full plan"),
            footer::hint(&["?"], "help"),
            footer::hint(&["q"], "quit"),
        ]);
        items
    } else {
        let mut items = vec![
            footer::hint(&["/"], "filter"),
            footer::hint(&["Enter"], "open raw"),
        ];
        if let Some(expanded) = expanded {
            items.push(footer::hint(
                &["Space"],
                if expanded { "collapse" } else { "expand" },
            ));
        }
        items.extend([
            footer::hint(&["v"], "full plan"),
            footer::hint(&["?"], "help"),
            footer::hint(&["q"], "quit"),
        ]);
        items
    }
}

fn required_footer_items(searching: bool) -> Vec<Line<'static>> {
    if searching {
        vec![
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "cancel"),
        ]
    } else {
        vec![
            footer::hint(&["Enter"], "open raw"),
            footer::hint(&["v"], "full plan"),
            footer::hint(&["?"], "help"),
            footer::hint(&["q"], "quit"),
        ]
    }
}

fn render_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    review: &PlanReview,
    view: &OverviewViewState,
) {
    let Some(overlay) = view.overlay() else {
        return;
    };
    match overlay {
        OverviewOverlay::Help => help_dialog::render(
            frame,
            area,
            overlay_title(overlay),
            &[
                help_dialog::HelpSection::new(
                    "Navigation",
                    vec![
                        help_dialog::HelpAction::new("↑ / ↓ / j / k", "select a row"),
                        help_dialog::HelpAction::new("PgUp / PgDn", "move one page"),
                        help_dialog::HelpAction::new("Home / End", "go to the first or last row"),
                    ],
                ),
                help_dialog::HelpSection::new(
                    "Review",
                    vec![
                        help_dialog::HelpAction::new("Enter", "open the selected raw block"),
                        help_dialog::HelpAction::new("/", "filter full addresses"),
                        help_dialog::HelpAction::new(
                            "Space",
                            "expand or collapse only on [+]/[-] group rows",
                        ),
                        help_dialog::HelpAction::new("v", "show the full plan from the top"),
                    ],
                ),
                help_dialog::HelpSection::new(
                    "Actions",
                    vec![
                        help_dialog::HelpAction::new("y", "copy the full plan"),
                        help_dialog::HelpAction::new("c", "show execution context"),
                    ],
                ),
                help_dialog::HelpSection::new(
                    "Exit",
                    vec![help_dialog::HelpAction::new("q", "quit")],
                ),
            ],
            view.overlay_scroll(),
        ),
        OverviewOverlay::Context => render_dialog(
            frame,
            area,
            overlay_title(overlay),
            context::context_lines(review.context()),
            view.overlay_scroll(),
        ),
    }
}

fn render_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &'static str,
    lines: Vec<Line<'static>>,
    scroll: u16,
) {
    let width = area.width.saturating_sub(4).min(96);
    let height = u16::try_from(
        Paragraph::new(lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(width.saturating_sub(2)),
    )
    .unwrap_or(u16::MAX)
    .saturating_add(3)
    .min(area.height.saturating_sub(2));
    if width < 12 || height < 4 {
        terminal_notice::render_wrapped(frame, area, "Terminal too small. Resize or press Esc.");
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
        .title(title);
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);
    let content = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body_style())
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        content,
    );
    footer::render(
        frame,
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        &[footer::hint(&["?", "Esc"], "close")],
        None,
    );
}

const fn overlay_title(overlay: OverviewOverlay) -> &'static str {
    match overlay {
        OverviewOverlay::Help => "Help",
        OverviewOverlay::Context => "Context",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
    };

    use super::*;
    use crate::ui::features::overview::OverviewInput;
    use crate::{
        app::{
            plan::{
                Plan, PlanAction, PlanSummary, PlanValue, ResourceChange, ResourceChangeKind,
                ResourceMode,
            },
            review::{PlanBlock, PlanBlockKind, PlanDocument, PlanMetadata},
        },
        ui::test_support::{buffer_text, render_to_buffer},
    };

    fn review() -> PlanReview {
        let addresses = vec![
            "terraform_data.server[\"one\"]".to_owned(),
            "terraform_data.server[\"two\"]".to_owned(),
        ];
        let document = PlanDocument::with_blocks_and_line_kinds(
            "Terraform will perform actions.\n\nserver blocks\n".to_owned(),
            vec![
                PlanBlock::new(0..2, PlanBlockKind::Common),
                PlanBlock::with_addresses(2..3, PlanBlockKind::Resource, addresses.clone()),
            ],
            Vec::new(),
        );
        let change = |address: String| ResourceChange {
            address,
            provider: None,
            resource_type: Some("terraform_data".to_owned()),
            resource_name: Some("server".to_owned()),
            mode: ResourceMode::Managed,
            actions: vec![PlanAction::Update],
            kind: ResourceChangeKind::Update,
            before: Some(PlanValue::Object(BTreeMap::from([(
                "input".to_owned(),
                PlanValue::String("old".to_owned()),
            )]))),
            after: Some(PlanValue::Object(BTreeMap::from([(
                "input".to_owned(),
                PlanValue::String("new".to_owned()),
            )]))),
            before_sensitive: None,
            after_sensitive: None,
            after_unknown: None,
            replace_paths: None,
            action_reason: None,
            previous_address: None,
            importing: None,
        };
        PlanReview::new(
            PathBuf::from("/repo/infra"),
            "default".to_owned(),
            document,
            PlanMetadata::new(Vec::new(), vec!["endpoint".to_owned()], 0, 2, 0, true)
                .with_nonstandard_changes(1),
            Vec::new(),
        )
        .with_plan(Plan {
            value_addresses: BTreeSet::new(),
            resource_changes: addresses.into_iter().map(change).collect(),
            summary: PlanSummary {
                creates: 0,
                updates: 2,
                replaces: 0,
                deletes: 0,
            },
            unsupported_changes: Vec::new(),
            output_changes: Vec::new(),
        })
    }

    #[test]
    fn renders_grouped_overview_with_fixed_counts_and_unsupported_notice() {
        let state = OverviewSessionState::new(review());
        let view = OverviewViewState::default();
        let buffer = render_to_buffer((100, 24), |frame| {
            render(frame, &state, &view, Instant::now());
        });

        let text = buffer_text(&buffer);
        assert!(!text.contains("Space expand"));
        insta::assert_snapshot!(text);
    }

    #[test]
    fn selected_group_footer_tracks_expansion_and_filtered_members() {
        let state = OverviewSessionState::new(review());
        let mut view = OverviewViewState::default();
        let body = Rect::new(0, 0, 80, 12);
        let content = OverviewContent::from_review(state.review(), "", view.expanded());

        view.apply(OverviewInput::Down, body, 0, &content);
        let collapsed = render_to_buffer((100, 24), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let collapsed_text = buffer_text(&collapsed);
        assert!(collapsed_text.contains("[+] terraform_data.server[*]"));
        assert!(collapsed_text.contains("Space expand"));
        let narrow_collapsed = render_to_buffer((40, 16), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        assert!(buffer_text(&narrow_collapsed).contains("Space expand"));

        view.apply(OverviewInput::ToggleExpand, body, 0, &content);
        let expanded_content = OverviewContent::from_review(state.review(), "", view.expanded());
        let expanded = render_to_buffer((100, 24), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let expanded_text = buffer_text(&expanded);
        assert!(expanded_text.contains("[-] terraform_data.server[*]"));
        assert!(expanded_text.contains("terraform_data.server[\"one\"]"));
        assert!(expanded_text.contains("Space collapse"));
        let narrow_expanded = render_to_buffer((40, 16), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        assert!(buffer_text(&narrow_expanded).contains("Space collapse"));

        view.apply(OverviewInput::Down, body, 0, &expanded_content);
        assert_eq!(view.selected_group_expanded(&expanded_content), None);
        let child = render_to_buffer((100, 24), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let child_text = buffer_text(&child);
        assert!(!child_text.contains("Space expand"));
        assert!(!child_text.contains("Space collapse"));
        let expanded_groups = view.expanded().clone();
        view.apply(OverviewInput::ToggleExpand, body, 0, &expanded_content);
        assert_eq!(view.expanded(), &expanded_groups);

        view.apply(OverviewInput::SearchStart, body, 0, &expanded_content);
        for character in "one".chars() {
            view.apply(
                OverviewInput::SearchChar(character),
                body,
                0,
                &expanded_content,
            );
        }
        view.apply(OverviewInput::SearchConfirm, body, 0, &expanded_content);
        let filtered_content =
            OverviewContent::from_review(state.review(), view.filter(), view.expanded());
        assert_eq!(filtered_content.rows.len(), 1);
        assert_eq!(view.selected_group_expanded(&filtered_content), None);
        let filtered = render_to_buffer((40, 16), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let filtered_text = buffer_text(&filtered);
        assert!(filtered_text.contains("server[\"one\"]"));
        assert!(!filtered_text.contains("terraform_data.server[*]"));
        assert!(!filtered_text.contains("Space expand"));
    }

    #[test]
    fn renders_help_as_a_grouped_modal_that_scrolls_on_small_terminals() {
        let state = OverviewSessionState::new(review());
        let mut view = OverviewViewState::default();
        let content = OverviewContent::from_review(state.review(), "", view.expanded());
        view.apply(
            OverviewInput::OpenHelp,
            Rect::new(0, 0, 80, 24),
            0,
            &content,
        );

        for (width, height) in [(40, 16), (40, 24), (80, 24), (120, 40), (160, 60)] {
            let buffer = render_to_buffer((width, height), |frame| {
                render(frame, &state, &view, Instant::now());
            });
            let text = buffer_text(&buffer);
            assert!(text.contains("Help"), "{width}x{height}: {text}");
            assert!(text.contains("Navigation"), "{width}x{height}: {text}");
            if width >= 80 {
                assert!(
                    text.contains("open the selected raw block"),
                    "{width}x{height}: {text}"
                );
                assert!(
                    text.contains("only on [+]/[-] group rows"),
                    "{width}x{height}: {text}"
                );
            }
            assert_eq!(text.matches("close").count(), 1, "{width}x{height}: {text}");
            if (width, height) == (80, 24) {
                assert!(
                    buffer
                        .cell((0, 0))
                        .expect("dimmed background")
                        .modifier
                        .contains(ratatui::style::Modifier::DIM)
                );
            }
            insta::assert_snapshot!(format!("overview_help_{width}x{height}"), text);
        }

        view.overlay_bottom();
        let bottom = render_to_buffer((40, 16), |frame| {
            render(frame, &state, &view, Instant::now());
        });
        let bottom_text = buffer_text(&bottom);
        assert!(bottom_text.contains("Exit"));
        assert!(bottom_text.contains("quit"));
        assert_eq!(bottom_text.matches("close").count(), 1);
        insta::assert_snapshot!("overview_help_40x16_bottom", bottom_text);
    }
}
