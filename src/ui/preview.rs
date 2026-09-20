use std::{env, fs, path::PathBuf};

use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::scrollbar::Set,
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::ui::{
    shell::{footer, header, layout as shell_layout},
    test_support::{buffer_terminal_capture, buffer_text, render_to_buffer},
    theme,
};

const PREVIEW_SIZES: &[(u16, u16)] = &[(80, 24), (120, 40), (160, 60)];
const PREVIEW_MAX_WIDTH: u16 = 120;
const PREVIEW_MAX_HEIGHT: u16 = 40;
const SEARCH_TERM: &str = "terraform_data";
const WARNING: &str = "This plan includes resource deletion and replacement. Review the production impact before applying this saved plan.";

const LONG_PLAN: &[&str] = &[
    "Terraform will perform the following actions:",
    "",
    "  # terraform_data.api will be updated in-place",
    "  ~ resource \"terraform_data\" \"api\" {",
    "      id       = \"api-20260920\"",
    "      ~ input  = \"before\" -> \"after\"",
    "      # (4 unchanged attributes hidden)",
    "    }",
    "",
    "  # terraform_data.worker must be replaced",
    "-/+ resource \"terraform_data\" \"worker\" {",
    "      ~ input = \"worker-before\" -> \"worker-after\" # forces replacement",
    "      - old_checksum = \"sha256:0123456789abcdef0123456789abcdef0123456789abcdef\"",
    "      + new_checksum = (known after apply)",
    "    }",
    "",
    "  # terraform_data.old will be destroyed",
    "  - resource \"terraform_data\" \"old\" {",
    "      id = \"old-20260920\"",
    "    }",
    "",
    "  # terraform_data.new will be created",
    "  + resource \"terraform_data\" \"new\" {",
    "      input = \"new-value\"",
    "      note  = \"A deliberately long synthetic value keeps horizontal scrolling visible\"",
    "    }",
    "",
    "Changes to Outputs:",
    "  + endpoint = (known after apply)",
    "  ~ summary  = \"old summary\" -> \"new summary with a deliberately long value for review\"",
    "",
    "Warning: Value for \"pending\" is not known until apply",
    "",
    "Plan: 2 to add, 2 to change, 1 to destroy.",
    "",
    "Synthetic review text continues below so the viewport and scrollbar remain meaningful.",
    "The same long body is intentionally reused across every preview state and terminal size.",
    "No Terraform process, provider, state file, or cloud credential is used by this preview.",
    "The review surface preserves Terraform order, attributes, output values, and diagnostics.",
    "Long lines remain unwrapped in the plan body; only the confirmation warning is wrapped.",
    "Horizontal movement exposes the hidden suffix of this line and vertical movement exposes later lines.",
    "",
    "End of synthetic plan body.",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewState {
    Normal,
    Search,
    ApplyConfirmation,
    ApplySuccess,
    ApplyFailure,
}

impl PreviewState {
    const ALL: [Self; 5] = [
        Self::Normal,
        Self::Search,
        Self::ApplyConfirmation,
        Self::ApplySuccess,
        Self::ApplyFailure,
    ];

    const fn slug(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Search => "search",
            Self::ApplyConfirmation => "apply-confirmation",
            Self::ApplySuccess => "apply-success",
            Self::ApplyFailure => "apply-failure",
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Normal => "Plan",
            Self::Search => "Plan | Search: terraform_data",
            Self::ApplyConfirmation => "Apply this reviewed plan?",
            Self::ApplySuccess => "Apply complete",
            Self::ApplyFailure => "Apply failed",
        }
    }
}

fn render_preview(frame: &mut Frame<'_>, state: PreviewState) {
    let panel = centered_panel(frame.area());
    let footer_lines = footer::layout(footer_items(state), panel.width);
    let required_footer = footer::layout(vec![footer::hint(&["q"], "quit")], panel.width);
    let shell = shell_layout::layout(panel, footer_lines, required_footer, 1);
    render_header(frame, shell.header(), state);
    let inner = shell_layout::render_content_block(frame, shell.content(), state.title());
    render_content(frame, inner, state);
    footer::render(frame, shell.footer(), shell.footer_lines().to_owned());
}

fn centered_panel(area: Rect) -> Rect {
    let width = area.width.saturating_sub(2).min(PREVIEW_MAX_WIDTH);
    let height = area.height.saturating_sub(2).min(PREVIEW_MAX_HEIGHT);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn render_header(frame: &mut Frame<'_>, area: Rect, state: PreviewState) {
    let workspace = if state == PreviewState::ApplyConfirmation {
        "    workspace: default"
    } else {
        ""
    };
    header::render(
        frame,
        area,
        vec![Line::from(Span::styled(
            format!("Terracotta | infra/prod{workspace}"),
            theme::secondary_style(),
        ))],
    );
}

fn render_content(frame: &mut Frame<'_>, inner: Rect, state: PreviewState) {
    let text_area = Rect::new(
        inner.x,
        inner.y,
        inner.width.saturating_sub(1),
        inner.height.saturating_sub(1),
    );
    let lines = content_lines(state, text_area.width);
    frame.render_widget(
        Paragraph::new(lines.clone())
            .style(theme::body_style())
            .scroll((0, 0)),
        text_area,
    );

    let mut vertical = ScrollbarState::new(lines.len())
        .viewport_content_length(usize::from(text_area.height))
        .position(0);
    let mut horizontal = ScrollbarState::new(max_line_width(&lines))
        .viewport_content_length(usize::from(text_area.width))
        .position(0);
    let active = Style::default().fg(Color::Rgb(0xc0, 0xb8, 0xb0));
    let track = Style::default().fg(Color::Rgb(0x50, 0x52, 0x5e));
    let vertical_scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .symbols(Set {
            track: "│",
            thumb: "█",
            begin: "↑",
            end: "↓",
        })
        .thumb_style(active)
        .track_style(track)
        .begin_style(active)
        .end_style(active);
    let horizontal_scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
        .symbols(Set {
            track: "─",
            thumb: "█",
            begin: "←",
            end: "→",
        })
        .thumb_style(active)
        .track_style(track)
        .begin_style(active)
        .end_style(active);
    let scrollbar_area = inner.inner(Margin {
        vertical: 0,
        horizontal: 0,
    });
    frame.render_stateful_widget(vertical_scrollbar, scrollbar_area, &mut vertical);
    frame.render_stateful_widget(horizontal_scrollbar, scrollbar_area, &mut horizontal);
}

fn content_lines(state: PreviewState, text_width: u16) -> Vec<Line<'static>> {
    let mut lines = match state {
        PreviewState::Normal => Vec::new(),
        PreviewState::Search => vec![Line::from(Span::styled(
            "Search: terraform_data (filtered; plan summary remains global)",
            search_style(),
        ))],
        PreviewState::ApplyConfirmation => vec![
            Line::from("Target: infra/prod"),
            Line::from("Workspace: default"),
            Line::from("Plan: 2 to add, 2 to change, 1 to destroy."),
            Line::default(),
            Line::from(Span::styled(WARNING, theme::warning_style())),
            Line::default(),
            Line::from("Apply this plan? (yes/no): _"),
            Line::default(),
        ],
        PreviewState::ApplySuccess => vec![
            Line::from(Span::styled(
                "Resources: 2 added, 2 changed, 1 destroyed.",
                success_style(),
            )),
            Line::from("────────────────────────────────────────────────"),
            Line::from("Apply finished successfully."),
            Line::default(),
        ],
        PreviewState::ApplyFailure => vec![
            Line::from(Span::styled(
                "Changes may already be applied.",
                theme::warning_style(),
            )),
            Line::from(Span::styled(
                "Error: AccessDenied: synthetic provider rejected the request",
                theme::error_style(),
            )),
            Line::from("Terraform diagnostic: inspect the log before retrying."),
            Line::default(),
        ],
    };

    lines.extend(LONG_PLAN.iter().map(|text| match state {
        PreviewState::Search => highlight_matches(text, SEARCH_TERM),
        _ => Line::from(Span::styled(
            (*text).to_owned(),
            theme::plan_line_style(text),
        )),
    }));
    if state == PreviewState::ApplyConfirmation {
        wrap_warning_line(&mut lines, usize::from(text_width));
    }
    lines
}

fn wrap_warning_line(lines: &mut Vec<Line<'static>>, width: usize) {
    let Some(line) = lines.get(4) else {
        return;
    };
    let warning = line.to_string();
    if Line::from(warning.as_str()).width() <= width {
        return;
    }
    let words = warning.split_whitespace();
    let mut wrapped = Vec::new();
    let mut current = String::new();
    for word in words {
        let next_width = if current.is_empty() {
            word.len()
        } else {
            current.len() + 1 + word.len()
        };
        if next_width > width && !current.is_empty() {
            wrapped.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        wrapped.push(current);
    }
    let replacement = wrapped
        .into_iter()
        .map(|text| Line::from(Span::styled(text, theme::warning_style())))
        .collect::<Vec<_>>();
    lines.splice(4..=4, replacement);
}

fn highlight_matches(text: &str, query: &str) -> Line<'static> {
    let mut line = Line::default();
    let mut remainder = text;
    while let Some(index) = remainder.find(query) {
        let (before, matched_and_after) = remainder.split_at(index);
        if !before.is_empty() {
            line.push_span(Span::styled(
                before.to_owned(),
                theme::plan_line_style(before),
            ));
        }
        let (matched, after) = matched_and_after.split_at(query.len());
        line.push_span(Span::styled(matched.to_owned(), search_match_style()));
        remainder = after;
    }
    if !remainder.is_empty() {
        line.push_span(Span::styled(
            remainder.to_owned(),
            theme::plan_line_style(remainder),
        ));
    }
    line
}

fn max_line_width(lines: &[Line<'static>]) -> usize {
    lines.iter().map(Line::width).max().unwrap_or(0)
}

fn footer_items(state: PreviewState) -> Vec<Line<'static>> {
    match state {
        PreviewState::Normal => vec![
            footer::hint(&["↑", "↓", "←", "→"], "scroll"),
            footer::hint(&["/"], "search"),
            footer::hint(&["a"], "apply"),
            footer::hint(&["y"], "yank"),
            footer::hint(&["q"], "quit"),
        ],
        PreviewState::Search => vec![
            footer::hint(&["/"], "terraform_data"),
            footer::hint(&["Enter"], "confirm"),
            footer::hint(&["Esc"], "cancel"),
            footer::hint(&["y"], "yank"),
            footer::hint(&["q"], "quit"),
        ],
        PreviewState::ApplyConfirmation => vec![
            footer::hint(&["Enter"], "yes"),
            footer::hint(&["n"], "no"),
            footer::hint(&["Esc"], "back"),
            footer::hint(&["q"], "quit"),
        ],
        PreviewState::ApplySuccess | PreviewState::ApplyFailure => vec![
            footer::hint(&["↑", "↓", "←", "→"], "scroll"),
            footer::hint(&["y"], "yank"),
            footer::hint(&["q"], "quit"),
        ],
    }
}

fn search_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0x88, 0xc0, 0xd0))
        .add_modifier(Modifier::BOLD)
}

fn search_match_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0x11, 0x14, 0x19))
        .bg(Color::Rgb(0xf4, 0x9e, 0x4c))
        .add_modifier(Modifier::BOLD)
}

fn success_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xa3, 0xbe, 0x8c))
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_states_at_the_three_review_sizes() {
        let capture_directory = env::var_os("TERRACOTTA_PREVIEW_CAPTURE_DIR").map(PathBuf::from);
        for &(width, height) in PREVIEW_SIZES {
            for state in PreviewState::ALL {
                let buffer = render_to_buffer((width, height), |frame| {
                    render_preview(frame, state);
                });
                let name = format!("preview_{width}x{height}_{}", state.slug());
                insta::assert_snapshot!(name.clone(), buffer_text(&buffer));
                if let Some(directory) = &capture_directory {
                    fs::create_dir_all(directory).expect("capture directory should be writable");
                    fs::write(
                        directory.join(format!("{name}.ansi")),
                        buffer_terminal_capture(&buffer),
                    )
                    .expect("ANSI capture should be writable");
                    fs::write(directory.join(format!("{name}.txt")), buffer_text(&buffer))
                        .expect("text capture should be writable");
                }
            }
        }
    }

    #[test]
    fn centers_the_maximum_panel_and_keeps_footer_outside_the_frame() {
        let buffer = render_to_buffer((160, 60), |frame| {
            render_preview(frame, PreviewState::Normal);
        });
        let panel = centered_panel(Rect::new(0, 0, 160, 60));
        let footer_lines = footer::layout(footer_items(PreviewState::Normal), panel.width);
        let shell = shell_layout::layout(
            panel,
            footer_lines,
            footer::layout(vec![footer::hint(&["q"], "quit")], panel.width),
            1,
        );
        assert_eq!(shell.content().x, 20);
        assert_eq!(shell.content().y, 12);
        assert_eq!(
            buffer
                .cell((shell.content().x, shell.content().y))
                .expect("top-left frame")
                .symbol(),
            "┌"
        );
        assert!(buffer_text(&buffer).contains("q quit"));
        assert_eq!(
            buffer
                .cell((shell.content().x, shell.footer().y))
                .expect("footer cell")
                .symbol(),
            "↑"
        );
        assert_eq!(
            buffer
                .cell((shell.content().x, shell.footer().y.saturating_sub(1)))
                .expect("frame bottom")
                .symbol(),
            "└"
        );
    }

    #[test]
    fn uses_review_colors_for_frame_warning_and_scrollbars() {
        let buffer = render_to_buffer((80, 24), |frame| {
            render_preview(frame, PreviewState::ApplyConfirmation);
        });
        assert_eq!(
            buffer.cell((1, 3)).expect("frame cell").fg,
            Color::Rgb(0x76, 0x7a, 0x84)
        );
        assert_eq!(
            buffer
                .content()
                .iter()
                .find(|cell| cell.symbol() == "↑")
                .expect("vertical arrow")
                .fg,
            Color::Rgb(0xc0, 0xb8, 0xb0)
        );
        assert!(buffer.content().iter().any(|cell| {
            cell.fg == Color::Rgb(0xeb, 0xcb, 0x8b) && cell.modifier.contains(Modifier::BOLD)
        }));
    }
}
