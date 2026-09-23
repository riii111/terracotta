use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::ui::{primitives::molecules::terminal_notice, shell::footer, theme};

const MAX_WIDTH: u16 = 76;
const MIN_WIDTH: u16 = 20;
const MIN_HEIGHT: u16 = 6;

pub(crate) struct HelpAction {
    keys: &'static str,
    description: &'static str,
}

impl HelpAction {
    pub(crate) const fn new(keys: &'static str, description: &'static str) -> Self {
        Self { keys, description }
    }

    pub(crate) const fn note(description: &'static str) -> Self {
        Self {
            keys: "",
            description,
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
struct ScrolledText {
    line: usize,
    height: usize,
    scroll: usize,
    x: u16,
    width: u16,
    text: &'static str,
    style: Style,
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

    let key_width = key_column_width(sections, width.saturating_sub(2));
    let description_x = key_width.saturating_add(u16::from(key_width > 0));
    let description_width = width.saturating_sub(2).saturating_sub(description_x).max(1);
    let content_height = content_height(sections, key_width, description_width);
    let height = u16::try_from(content_height)
        .unwrap_or(u16::MAX)
        .saturating_add(3)
        .min(area.height.saturating_sub(2));
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
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let footer_area = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(1),
        inner.width,
        u16::from(inner.height > 0),
    );
    let max_scroll = content_height.saturating_sub(usize::from(content.height));
    let scroll = usize::from(scroll).min(max_scroll);
    render_sections(
        frame,
        content,
        sections,
        key_width,
        description_x,
        description_width,
        scroll,
    );
    footer::render(
        frame,
        footer_area,
        &[footer::hint(&["?", "Esc"], "close")],
        None,
    );
}

fn dialog_width(area: Rect, sections: &[HelpSection]) -> u16 {
    let max_key_width = sections
        .iter()
        .flat_map(|section| &section.actions)
        .map(|action| Line::from(action.keys).width())
        .max()
        .unwrap_or_default();
    let max_description_width = sections
        .iter()
        .flat_map(|section| &section.actions)
        .map(|action| Line::from(action.description).width())
        .max()
        .unwrap_or_default();
    let content_width = max_key_width
        .saturating_add(usize::from(max_key_width > 0))
        .saturating_add(max_description_width);
    let natural_width = u16::try_from(content_width.saturating_add(6))
        .unwrap_or(u16::MAX)
        .max(MIN_WIDTH);
    area.width
        .saturating_sub(4)
        .min(MAX_WIDTH)
        .min(natural_width)
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

fn key_column_width(sections: &[HelpSection], inner_width: u16) -> u16 {
    let max_key_width = sections
        .iter()
        .flat_map(|section| &section.actions)
        .filter(|action| !action.keys.is_empty())
        .map(|action| Line::from(action.keys).width())
        .max()
        .unwrap_or_default();
    let key_limit = inner_width / 3;
    u16::try_from(max_key_width)
        .unwrap_or(u16::MAX)
        .min(key_limit)
}

fn content_height(sections: &[HelpSection], key_width: u16, description_width: u16) -> usize {
    let mut height = 0;
    for (section_index, section) in sections.iter().enumerate() {
        if section_index > 0 {
            height += 1;
        }
        height += 1;
        for (action_index, action) in section.actions.iter().enumerate() {
            let row_height = row_height(action, key_width, description_width);
            height += row_height;
            if action_index + 1 < section.actions.len() {
                height += 1;
            }
        }
    }
    height
}

fn row_height(action: &HelpAction, key_width: u16, description_width: u16) -> usize {
    if action.keys.is_empty() {
        return wrapped_lines(action.description, description_width);
    }
    wrapped_lines(action.keys, key_width).max(wrapped_lines(action.description, description_width))
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
    key_width: u16,
    description_x: u16,
    description_width: u16,
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

        for (action_index, action) in section.actions.iter().enumerate() {
            let height = row_height(action, key_width, description_width);
            if action.keys.is_empty() {
                render_scrolled_text(
                    frame,
                    viewport,
                    ScrolledText {
                        line,
                        height,
                        scroll,
                        x: viewport.x,
                        width: viewport.width,
                        text: action.description,
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
                        width: key_width,
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
                        x: viewport.x.saturating_add(description_x),
                        width: description_width,
                        text: action.description,
                        style: theme::body_style(),
                    },
                );
            }
            line += height;
            if action_index + 1 < section.actions.len() {
                line += 1;
            }
        }
    }
}

fn render_scrolled_text(frame: &mut Frame<'_>, viewport: Rect, text: ScrolledText) {
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
