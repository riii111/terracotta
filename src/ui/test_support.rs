use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::{Frame, Terminal};

pub(super) const REPRESENTATIVE_TERMINAL_SIZE: (u16, u16) = (165, 51);

pub(super) fn render_to_buffer(
    (width, height): (u16, u16),
    render: impl FnOnce(&mut Frame<'_>),
) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal should be created");
    terminal.draw(render).expect("test frame should render");
    terminal.backend().buffer().clone()
}

pub(super) fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area();
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .filter_map(|x| buffer.cell((x, y)))
                .map(Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
