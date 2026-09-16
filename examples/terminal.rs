use std::io::{self, IsTerminal};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::widgets::{Block, Paragraph};

fn main() -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "the terminal example requires an interactive terminal",
        ));
    }

    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| {
                let content =
                    Paragraph::new("Development setup ready. Press q, Esc, or Ctrl-C to quit.")
                        .block(Block::bordered().title("Terracotta"));
                frame.render_widget(content, frame.area());
            })?;

            if let Event::Key(key) = event::read()?
                && key.is_press()
                && (matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)))
            {
                return Ok(());
            }
        }
    })
}
