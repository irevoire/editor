use std::{collections::VecDeque, io::Write, ops};

use crossterm::{
    cursor::MoveTo,
    style::{Print, PrintStyledContent, Stylize},
    terminal::{Clear, ClearType},
    ExecutableCommand, QueueableCommand,
};
use std::io;

use crate::{
    action::{Anchor, Direction},
    screen::screen_buffer::ScreenBuffer,
    server::ServerHandle,
    BufferView, Selection,
};

mod screen_buffer;

pub struct Screen {
    stdout: io::Stdout,
    width: usize,
    height: usize,
    buffer: ScreenBuffer,
    server: ServerHandle,
    view: BufferView,
}

impl Screen {
    pub fn new(server: ServerHandle, stdout: io::Stdout) -> Screen {
        let (width, height) = crossterm::terminal::size().unwrap();
        let (width, height) = (width as usize, height as usize);

        let buffer = server.current_buffer();
        let (content, nb_line) = server.get_lines(buffer, 0, height);
        let content = VecDeque::from(content);

        Screen {
            stdout,
            width: width,
            height: height,
            server,
            view: BufferView {
                width: width,
                height: height,
                top_line: 0,
                buffer,
                selection: Selection::default(),
                nb_line,
                content,
            },
            buffer: ScreenBuffer::new(width, height),
        }
    }

    pub fn focus_gained(&mut self) {
        self.stdout
            .execute(crossterm::cursor::SetCursorStyle::BlinkingBlock)
            .unwrap();
    }

    pub fn focus_lost(&mut self) {
        self.stdout
            .execute(crossterm::cursor::SetCursorStyle::SteadyBlock)
            .unwrap();
    }

    pub fn redraw(&mut self) {
        let start = &self.view.selection.tail;
        let start = (start.line, start.column);

        let end = &self.view.selection.head;
        let end = (end.line, end.column);

        for (row, line) in self.view.content.iter().enumerate() {
            self.stdout
                .queue(MoveTo(row as u16, 0))
                .unwrap()
                .queue(Clear(ClearType::CurrentLine))
                .unwrap();
            for (col, c) in line.chars().enumerate() {
                if (row, col) == start {
                    // That's the cursor, should be do anything?
                    self.stdout.queue(Print(c.black().on_white())).unwrap();
                } else if (row, col) == end {
                    self.stdout.queue(Print(c.on_blue())).unwrap();
                } else if (row, col) > start && (row, col) < end {
                    self.stdout.queue(Print(c.on_dark_blue())).unwrap();
                }
            }
        }
        self.stdout.flush();
    }

    pub fn move_anchor(&mut self, anchor: Anchor, direction: Direction) {
        let anchor = match anchor {
            Anchor::Start => &mut self.view.selection.tail,
            Anchor::End => &mut self.view.selection.head,
        };
        match direction {
            Direction::Up => {
                if anchor.line == 0 {
                    return;
                };
                // we have to get a new line from the buffer
                if anchor.line == self.view.top_line {
                    let (mut line, _nb_lines) =
                        self.server
                            .get_lines(self.view.buffer, self.view.top_line - 1, 1);
                    self.view.content.push_back(line.pop().unwrap());
                    self.view.content.pop_front();
                }

                anchor.line -= 1;
            }
            Direction::Down => {
                if anchor.line == self.view.nb_line {
                    return;
                }
                // we've reached the bottom of the screen
                if anchor.line == self.view.top_line + self.view.content.len() {
                    let (mut line, _nb_lines) = self.server.get_lines(
                        self.view.buffer,
                        self.view.top_line + self.view.content.len(),
                        1,
                    );
                    self.view.content.push_front(line.pop().unwrap());
                    self.view.content.pop_back();
                }
                anchor.line += 1;
            }
            Direction::Right => {
                anchor.column = anchor.column.saturating_add(1).min(
                    self.view.content[self.view.top_line - anchor.line]
                        .chars()
                        .count(),
                )
            }
            Direction::Left => anchor.column = anchor.column.saturating_sub(1),
            Direction::StartOfLine => anchor.column = 0,
            Direction::EndOfLine => {
                anchor.column = self.view.content[self.view.top_line - anchor.line]
                    .chars()
                    .count()
            }
            Direction::StartOfFile => {
                anchor.line = 0;
                anchor.column = 0;
            }
            Direction::EndOfFile => todo!(),
            Direction::PageUp => todo!(),
            Direction::PageDown => todo!(),
        }
    }

    pub fn insert(&mut self, c: char) {
        self.view.insert(&self.server, c);
    }
}
