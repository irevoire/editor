use std::io;
use termion::raw::IntoRawMode;
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::widgets::{Block, Borders, Widget};
use tui::Terminal;

pub struct Ui<'a> {
    pub title: &'a str,
    pub terminal: Terminal<TermionBackend<termion::raw::RawTerminal<io::Stdout>>>,
}

impl<'a> Ui<'a> {
    pub fn new(title: &'a str) -> Result<Ui<'a>, io::Error> {
        let stdout = io::stdout().into_raw_mode()?;
        let backend = TermionBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        Ok(Ui { title, terminal })
    }

    pub fn draw(&mut self) {
        self.terminal.draw(|mut f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Percentage(10),
                        Constraint::Percentage(80),
                        Constraint::Percentage(10),
                    ]
                    .as_ref(),
                )
                .split(f.size());
            Block::default()
                .title("Block")
                .borders(Borders::ALL)
                .render(&mut f, chunks[0]);
            Block::default()
                .title("Block 2")
                .borders(Borders::ALL)
                .render(&mut f, chunks[2]);
        });
    }

    pub fn on_key(&mut self, c: char) {
        match c {
            _ => {}
        }
    }
}
