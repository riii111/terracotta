use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::app::execution::ExecutionContext;
use crate::ui::{
    primitives::molecules::{dialog_scroll::DialogScroll, terminal_notice},
    shell::{context, footer},
    theme,
};

const MAX_WIDTH: u16 = 96;
const MIN_WIDTH: u16 = 12;
const MIN_HEIGHT: u16 = 4;

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    execution_context: &ExecutionContext,
    scroll: &DialogScroll,
) {
    let width = area.width.saturating_sub(4).min(MAX_WIDTH);
    let body = Paragraph::new(context::context_lines(execution_context)).wrap(Wrap { trim: false });
    let height = u16::try_from(body.line_count(width.saturating_sub(2)))
        .unwrap_or(u16::MAX)
        .saturating_add(3)
        .min(area.height.saturating_sub(2));
    if width < MIN_WIDTH || height < MIN_HEIGHT {
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
        .title("Context");
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);
    let content = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let max_scroll = u16::try_from(
        body.line_count(content.width)
            .saturating_sub(usize::from(content.height)),
    )
    .unwrap_or(u16::MAX);
    frame.render_widget(
        body.style(theme::body_style())
            .scroll((scroll.clamp_for_render(max_scroll), 0)),
        content,
    );
    footer::render(
        frame,
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        &[footer::hint(&["?", "Esc"], "close")],
        None,
    );
}
