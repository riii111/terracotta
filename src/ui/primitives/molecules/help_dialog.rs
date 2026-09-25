use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::ui::{
    primitives::{atoms::scrollbar, molecules::terminal_notice},
    shell::footer,
    theme,
};

const MIN_WIDTH: u16 = 20;
const MIN_HEIGHT: u16 = 6;
const HORIZONTAL_PADDING: u16 = 1;
const MIN_DESCRIPTION_WIDTH: u16 = 28;
const STACKED_DESCRIPTION_INDENT: u16 = 2;

pub(crate) struct HelpAction {
    keys: &'static str,
    description: String,
}

impl HelpAction {
    pub(crate) fn new(keys: &'static str, description: impl Into<String>) -> Self {
        Self {
            keys,
            description: description.into(),
        }
    }
}

pub(crate) struct HelpSection {
    title: &'static str,
    actions: Vec<HelpAction>,
}

impl HelpSection {
    pub(crate) const fn new(title: &'static str, actions: Vec<HelpAction>) -> Self {
        Self { title, actions }
    }
}

#[derive(Clone, Copy)]
struct ScrolledText<'a> {
    line: usize,
    height: usize,
    scroll: usize,
    x: u16,
    width: u16,
    text: &'a str,
    style: Style,
}

#[derive(Clone, Copy)]
struct ActionLayout {
    content_width: u16,
    key_width: u16,
    description_x: u16,
    description_width: u16,
    stacked: bool,
}

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &'static str,
    sections: &[HelpSection],
    scroll: u16,
) {
    let width = dialog_width(area, sections);
    if width < MIN_WIDTH || area.height < MIN_HEIGHT {
        terminal_notice::render_wrapped(frame, area, "Terminal too small. Resize or press Esc.");
        return;
    }

    dim_background(frame, area);

    let action_layout = action_layout(sections, width, area.width);
    let content_height = content_height(sections, action_layout);
    let inner_width = width.saturating_sub(2);
    let footer_width = inner_width.saturating_sub(HORIZONTAL_PADDING.saturating_mul(2));
    let footer_lines = footer::layout(vec![footer::hint(&["?", "Esc"], "close")], footer_width);
    let available_height = area.height.saturating_sub(2);
    let footer_height = u16::try_from(footer_lines.len()).unwrap_or(u16::MAX);
    let height = u16::try_from(content_height)
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .saturating_add(footer_height)
        .min(available_height);
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
        .title(title)
        .title_style(theme::accent_style().add_modifier(Modifier::BOLD));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    let content = Rect::new(
        inner.x.saturating_add(HORIZONTAL_PADDING),
        inner.y,
        action_layout.content_width,
        inner.height.saturating_sub(footer_height),
    );
    let scrollbar_area = Rect::new(
        content.right().saturating_add(1),
        content.y,
        1,
        content.height,
    );
    let footer_area = Rect::new(
        inner.x.saturating_add(HORIZONTAL_PADDING),
        inner.bottom().saturating_sub(footer_height),
        footer_width,
        footer_height,
    );
    let max_scroll = content_height.saturating_sub(usize::from(content.height));
    let scroll = usize::from(scroll).min(max_scroll);
    render_sections(frame, content, sections, action_layout, scroll);
    if content.height > 0 {
        scrollbar::render_vertical(
            frame,
            scrollbar_area,
            content_height,
            usize::from(content.height),
            scroll,
        );
    }
    footer::render(frame, footer_area, &footer_lines, None);
}

fn dialog_width(area: Rect, sections: &[HelpSection]) -> u16 {
    area.width
        .saturating_sub(2)
        .min(required_dialog_width(sections))
}

fn action_layout(sections: &[HelpSection], dialog_width: u16, terminal_width: u16) -> ActionLayout {
    let content_width = dialog_width.saturating_sub(6);
    let key_width = key_column_width(sections);
    let available_content_width = terminal_width.saturating_sub(8);
    let stacked = key_width > 0
        && available_content_width
            < key_width
                .saturating_add(1)
                .saturating_add(MIN_DESCRIPTION_WIDTH);
    let description_x = if stacked {
        STACKED_DESCRIPTION_INDENT
    } else {
        key_width.saturating_add(u16::from(key_width > 0))
    };
    let description_width = content_width.saturating_sub(description_x).max(1);

    ActionLayout {
        content_width,
        key_width,
        description_x,
        description_width,
        stacked,
    }
}

fn required_content_width(sections: &[HelpSection]) -> u16 {
    let key_width = key_column_width(sections);
    let description_width = max_description_width(sections);
    let column_width = key_width
        .saturating_add(u16::from(key_width > 0))
        .saturating_add(description_width);
    sections
        .iter()
        .flat_map(|section| &section.actions)
        .map(|action| line_width(&action.description))
        .fold(column_width, u16::max)
}

fn key_column_width(sections: &[HelpSection]) -> u16 {
    sections
        .iter()
        .flat_map(|section| &section.actions)
        .filter(|action| !action.keys.is_empty())
        .map(|action| line_width(action.keys))
        .max()
        .unwrap_or_default()
}

fn max_description_width(sections: &[HelpSection]) -> u16 {
    sections
        .iter()
        .flat_map(|section| &section.actions)
        .map(|action| line_width(&action.description))
        .max()
        .unwrap_or_default()
}

fn line_width(text: &str) -> u16 {
    u16::try_from(Line::from(text).width()).unwrap_or(u16::MAX)
}

fn dim_background(frame: &mut Frame<'_>, area: Rect) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                cell.set_style(cell.style().add_modifier(Modifier::DIM));
            }
        }
    }
}

fn required_dialog_width(sections: &[HelpSection]) -> u16 {
    required_content_width(sections)
        .saturating_add(6)
        .max(MIN_WIDTH)
}

fn content_height(sections: &[HelpSection], layout: ActionLayout) -> usize {
    let mut height = 0;
    for (section_index, section) in sections.iter().enumerate() {
        if section_index > 0 {
            height += 1;
        }
        height += 1;
        for action in &section.actions {
            let row_height = row_height(action, layout);
            height += row_height;
        }
    }
    height
}

fn row_height(action: &HelpAction, layout: ActionLayout) -> usize {
    if action.keys.is_empty() {
        return wrapped_lines(&action.description, layout.content_width);
    }
    if layout.stacked {
        return wrapped_lines(action.keys, layout.content_width)
            .saturating_add(wrapped_lines(&action.description, layout.description_width));
    }
    wrapped_lines(action.keys, layout.key_width)
        .max(wrapped_lines(&action.description, layout.description_width))
}

fn wrapped_lines(text: &str, width: u16) -> usize {
    Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
        .max(1)
}

fn render_sections(
    frame: &mut Frame<'_>,
    viewport: Rect,
    sections: &[HelpSection],
    layout: ActionLayout,
    scroll: usize,
) {
    let mut line = 0;
    for (section_index, section) in sections.iter().enumerate() {
        if section_index > 0 {
            line += 1;
        }
        render_scrolled_text(
            frame,
            viewport,
            ScrolledText {
                line,
                height: 1,
                scroll,
                x: viewport.x,
                width: viewport.width,
                text: section.title,
                style: theme::accent_style().add_modifier(Modifier::BOLD),
            },
        );
        line += 1;

        for action in &section.actions {
            let height = row_height(action, layout);
            if action.keys.is_empty() {
                render_scrolled_text(
                    frame,
                    viewport,
                    ScrolledText {
                        line,
                        height,
                        scroll,
                        x: viewport.x,
                        width: layout.content_width,
                        text: &action.description,
                        style: theme::body_style(),
                    },
                );
            } else if layout.stacked {
                let key_height = wrapped_lines(action.keys, layout.content_width);
                render_scrolled_text(
                    frame,
                    viewport,
                    ScrolledText {
                        line,
                        height: key_height,
                        scroll,
                        x: viewport.x,
                        width: layout.content_width,
                        text: action.keys,
                        style: theme::accent_style(),
                    },
                );
                render_scrolled_text(
                    frame,
                    viewport,
                    ScrolledText {
                        line: line.saturating_add(key_height),
                        height: height.saturating_sub(key_height),
                        scroll,
                        x: viewport.x.saturating_add(layout.description_x),
                        width: layout.description_width,
                        text: &action.description,
                        style: theme::body_style(),
                    },
                );
            } else {
                render_scrolled_text(
                    frame,
                    viewport,
                    ScrolledText {
                        line,
                        height,
                        scroll,
                        x: viewport.x,
                        width: layout.key_width,
                        text: action.keys,
                        style: theme::accent_style(),
                    },
                );
                render_scrolled_text(
                    frame,
                    viewport,
                    ScrolledText {
                        line,
                        height,
                        scroll,
                        x: viewport.x.saturating_add(layout.description_x),
                        width: layout.description_width,
                        text: &action.description,
                        style: theme::body_style(),
                    },
                );
            }
            line += height;
        }
    }
}

fn render_scrolled_text(frame: &mut Frame<'_>, viewport: Rect, text: ScrolledText<'_>) {
    if text.width == 0 || text.height == 0 {
        return;
    }
    let start = text.line.max(text.scroll);
    let end = text
        .line
        .saturating_add(text.height)
        .min(text.scroll.saturating_add(usize::from(viewport.height)));
    if start >= end {
        return;
    }
    let area = Rect::new(
        text.x,
        viewport
            .y
            .saturating_add(u16::try_from(start - text.scroll).unwrap_or(u16::MAX)),
        text.width,
        u16::try_from(end - start).unwrap_or(u16::MAX),
    );
    let skipped = u16::try_from(start - text.line).unwrap_or(u16::MAX);
    frame.render_widget(
        Paragraph::new(text.text)
            .style(text.style)
            .wrap(Wrap { trim: false })
            .scroll((skipped, 0)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use crate::ui::test_support::{buffer_text, render_to_buffer};

    use super::{
        ActionLayout, HelpAction, HelpSection, action_layout, content_height, dialog_width, render,
        required_dialog_width,
    };

    #[test]
    fn operation_rows_are_compact_and_sections_remain_separated() {
        let sections = [
            HelpSection::new(
                "First",
                vec![HelpAction::new("a", "one"), HelpAction::new("b", "two")],
            ),
            HelpSection::new("Second", vec![HelpAction::new("c", "three")]),
        ];

        let layout = ActionLayout {
            content_width: 10,
            key_width: 1,
            description_x: 2,
            description_width: 8,
            stacked: false,
        };
        assert_eq!(content_height(&sections, layout), 6);
    }

    #[test]
    fn wide_dialogs_expand_to_fit_their_content_without_a_fixed_cap() {
        let sections = [HelpSection::new(
            "Comparison",
            vec![HelpAction::new(
                "Same changes",
                "no differences detected between Ready plans; unknown values may differ",
            )],
        )];

        let width = dialog_width(Rect::new(0, 0, 160, 60), &sections);
        let layout = action_layout(&sections, width, 160);

        assert!(width > 76);
        assert_eq!(width, required_dialog_width(&sections));
        assert!(!layout.stacked);
    }

    #[test]
    fn help_rows_stack_when_narrow_and_return_to_columns_after_resize() {
        let sections = [HelpSection::new(
            "Overview",
            vec![HelpAction::new("Space", "toggle a selected ▸/▾ group row")],
        )];

        let narrow = buffer_text(&render_to_buffer((40, 24), |frame| {
            render(frame, frame.area(), "Help", &sections, 0);
        }));
        let key_line = narrow
            .lines()
            .position(|line| line.contains("Space"))
            .expect("key row should be visible");
        let description_line = narrow
            .lines()
            .position(|line| line.contains("toggle a selected"))
            .expect("description row should be visible");
        assert!(description_line > key_line);

        let wide = buffer_text(&render_to_buffer((120, 40), |frame| {
            render(frame, frame.area(), "Help", &sections, 0);
        }));
        assert!(
            wide.lines()
                .any(|line| { line.contains("Space") && line.contains("toggle a selected") })
        );
    }
}
