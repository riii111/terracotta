use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::{Frame, Terminal};

pub(super) const REPRESENTATIVE_TERMINAL_SIZE: (u16, u16) = (165, 51);
pub(super) const REALISTIC_REPOSITORY_ROOT: &str = "/repo";
pub(super) const REALISTIC_EXECUTION_ROOT: &str = "/repo/environments/development/main";
pub(super) const REALISTIC_DEVELOPMENT_SOURCE: &str =
    "/repo/environments/development/main/service.tf";
pub(super) const REALISTIC_PRODUCTION_SOURCE: &str =
    "/repo/environments/production/main/service.tf";
pub(super) const REALISTIC_COMMON_SOURCE: &str = "/repo/common/main/service.tf";

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

pub(super) fn assert_shell_frame_and_footer(
    buffer: &Buffer,
    content: Rect,
    footer: Rect,
    footer_marker: &str,
) {
    assert_eq!(content.y + content.height, footer.y);
    assert!(content.height >= 2);
    assert_eq!(
        buffer.cell((content.x, content.y)).expect("frame cell").fg,
        Color::Rgb(0x76, 0x7a, 0x84)
    );
    let footer_text = (footer.y..footer.bottom())
        .flat_map(|y| (footer.x..footer.right()).filter_map(move |x| buffer.cell((x, y))))
        .map(Cell::symbol)
        .collect::<String>();
    assert!(footer_text.contains(footer_marker), "{footer_text}");
}
