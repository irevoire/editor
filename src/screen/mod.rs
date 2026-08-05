use crossterm::{
    style::{ContentStyle, StyledContent},
    ExecutableCommand,
};
use std::io;

use crate::{
    action::{Anchor, DeleteDirection, Direction},
    screen::{screen_buffer::ScreenBuffer, view::buffer_view::BufferView},
    server::ServerHandle,
    ActionResult, Selection,
};

pub mod screen_buffer;
pub mod view;

pub struct Screen {
    stdout: io::Stdout,
    buffer: ScreenBuffer,
    server: ServerHandle,
    view: BufferView,
}

impl Screen {
    pub fn new(server: ServerHandle, stdout: io::Stdout) -> Screen {
        let (col, row) = crossterm::terminal::size().unwrap();
        let (col, row) = (col as usize, row as usize);
        log::warn!("Opened a terminal with {col} columns and {row} rows");

        let (_buffer_id, buffer) = server.current_buffer();

        Screen {
            stdout,
            server,
            view: BufferView {
                width: col,
                height: row,
                top_line: 0,
                selection: Selection::default(),
                buffer,
            },
            buffer: ScreenBuffer::new(col, row),
        }
    }

    pub fn focus_gained(&mut self) -> ActionResult {
        self.stdout
            .execute(crossterm::cursor::SetCursorStyle::BlinkingBlock)
            .unwrap();
        ActionResult::Nothing
    }

    pub fn focus_lost(&mut self) -> ActionResult {
        self.stdout
            .execute(crossterm::cursor::SetCursorStyle::SteadyBlock)
            .unwrap();
        ActionResult::Nothing
    }

    pub fn redraw(&mut self) -> ActionResult {
        self.view.redraw(
            &mut self
                .buffer
                .sub_screen_buffer((0, 0), (self.buffer.height() - 3, self.buffer.width() - 1)),
        );
        self.buffer.display_on_screen(&mut self.stdout).unwrap();
        ActionResult::Nothing
    }

    pub fn move_anchor(&mut self, anchor: Anchor, direction: Direction) -> ActionResult {
        let anchor = match anchor {
            Anchor::Tail => &mut self.view.selection.tail,
            Anchor::Head => &mut self.view.selection.head,
        };
        let rope = self.view.buffer.rope.blocking_read();
        match direction {
            Direction::Up => {
                if anchor.line == 0 {
                    return ActionResult::Nothing;
                } else if anchor.line == self.view.top_line {
                    self.view.top_line -= 1;
                } else {
                    anchor.line -= 1;
                }
            }
            Direction::Down => {
                if anchor.line == rope.len_lines() {
                    return ActionResult::Nothing;
                }
                // we've reached the bottom of the screen. We move all the text but not the anchor
                if anchor.line == self.view.top_line + self.view.height {
                    self.view.top_line += 1;
                } else {
                    anchor.line += 1;
                }
            }
            Direction::Right => {
                anchor.column = anchor
                    .column
                    .saturating_add(1)
                    .min(rope.line(anchor.line).len_chars())
            }
            Direction::Left => anchor.column = anchor.column.saturating_sub(1),
            Direction::StartOfLine => anchor.column = 0,
            Direction::EndOfLine => anchor.column = rope.line(anchor.line).len_chars(),
            Direction::StartOfFile => {
                self.view.top_line = 0;
                anchor.line = 0;
                anchor.column = 0;
            }
            Direction::EndOfFile => todo!(),
            Direction::PageUp => todo!(),
            Direction::PageDown => todo!(),
        }
        ActionResult::Redraw
    }

    pub fn insert(&mut self, c: char) -> ActionResult {
        self.view.insert(c)
    }

    pub fn delete(&mut self, delete_direction: DeleteDirection) -> ActionResult {
        self.view.delete(delete_direction)
    }

    pub fn change_mode(&mut self, mode: crate::Mode) -> ActionResult {
        let s = format!("{mode:?}");
        let middle = self.buffer.width() / 2;
        let start_writing_at = middle - s.len() / 2;
        let last_line = self.buffer.height() - 1;

        for (i, c) in s.chars().enumerate() {
            self.buffer[(last_line, start_writing_at + i)] =
                StyledContent::new(ContentStyle::default(), c);
        }
        ActionResult::Redraw
    }
}
