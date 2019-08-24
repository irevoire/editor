mod ui;

use std::io::Read;
use termion::event::Key;
use termion::input::TermRead;
use tui::Terminal;

fn main() -> Result<(), std::io::Error> {
    let mut ui = ui::Ui::new("test")?;
    let mut inputs = termion::async_stdin().keys();

    loop {
        ui.draw();

        if let Some(key) = inputs.next() {
            match key.expect("2") {
                Key::Ctrl('c') => break,
                Key::Char(c) => {
                    ui.on_key(c);
                }
                _ => {}
            }
        }
    }

    Ok(())
}
