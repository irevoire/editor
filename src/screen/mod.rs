use crossterm::{
    cursor::SetCursorStyle,
    style::{ContentStyle, StyledContent},
    ExecutableCommand, QueueableCommand,
};
use std::io;

use crate::{
    action::{Anchor, DeleteDirection, Direction},
    screen::{screen_buffer::ScreenBuffer, view::buffer_view::BufferView},
    server::ServerHandle,
    ActionResult, Selection, SelectionMode,
};

mod geo;
pub mod screen_buffer;
pub mod view;

pub use geo::*;

pub struct Screen {
    stdout: io::Stdout,
    buffer: ScreenBuffer,
    server: ServerHandle,
    view: BufferView,
}

impl Screen {
    pub fn new(server: ServerHandle, stdout: io::Stdout) -> Screen {
        let (col, row) = crossterm::terminal::size().unwrap();
        log::warn!("Opened a terminal with {col} columns and {row} rows");

        let (_buffer_id, buffer) = server.current_buffer();

        Screen {
            stdout,
            server,
            view: BufferView {
                width: col as usize,
                height: row as usize,
                top_line: 0,
                selection: Selection::default(),
                active: true,
                buffer,
            },
            buffer: ScreenBuffer::new(row, col),
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
        let mut sub_screen = self.buffer.as_sub_screen();
        self.view.draw(&mut sub_screen.sub_screen(ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: sub_screen.height() - 3,
                column: sub_screen.width() - 1,
            },
        )));
        self.buffer.display_on_screen(&mut self.stdout).unwrap();
        ActionResult::Nothing
    }

    pub fn move_anchor(
        &mut self,
        anchor: Anchor,
        direction: Direction,
        mode: SelectionMode,
    ) -> ActionResult {
        self.view.move_anchor(anchor, direction, mode)
    }

    pub fn insert(&mut self, c: char) -> ActionResult {
        self.view.insert(c)
    }

    pub fn delete(&mut self, delete_direction: DeleteDirection) -> ActionResult {
        self.view.delete(delete_direction)
    }

    pub fn change_mode(&mut self, mode: crate::Mode) -> io::Result<ActionResult> {
        let cursor_shape = match mode {
            crate::Mode::Normal => SetCursorStyle::DefaultUserShape,
            crate::Mode::Insert => SetCursorStyle::BlinkingBar,
        };
        self.stdout.queue(cursor_shape)?;
        let s = format!("{mode:?}");
        let middle = self.buffer.width() / 2;
        let start_writing_at = middle - s.len() as u16 / 2;
        let last_line = self.buffer.height() - 1;

        for (i, c) in s.chars().enumerate() {
            let coord = ScreenCoord {
                line: last_line,
                column: start_writing_at + i as u16,
            };
            self.buffer[coord] = StyledContent::new(ContentStyle::default(), c.to_string());
        }
        Ok(ActionResult::Redraw)
    }
}
