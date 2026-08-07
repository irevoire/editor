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

pub mod screen_buffer;
pub mod view;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord)]
pub struct ScreenCoord {
    pub line: u16,
    pub column: u16,
}

impl ScreenCoord {
    pub const fn zero() -> Self {
        Self { line: 0, column: 0 }
    }
}

// We implement PartialOrd manually because we want to be 100% sure that the line
// takes priority over the column.
impl PartialOrd for ScreenCoord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.line.partial_cmp(&other.line) {
            Some(core::cmp::Ordering::Equal) => self.column.partial_cmp(&other.column),
            ord => ord,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScreenArea {
    top_left: ScreenCoord,
    bottom_right: ScreenCoord,
}

impl ScreenArea {
    pub fn new(top_left: ScreenCoord, bottom_right: ScreenCoord) -> Self {
        assert!(
            top_left.line < bottom_right.line && top_left.column < bottom_right.column,
            "{top_left:?} >= {bottom_right:?}"
        );

        Self {
            top_left,
            bottom_right,
        }
    }

    pub fn width(&self) -> u16 {
        self.bottom_right.column - self.top_left.column
    }

    pub fn height(&self) -> u16 {
        self.bottom_right.line - self.top_left.line
    }

    pub fn contains(&self, other: Self) -> bool {
        self.top_left.line <= other.top_left.line
            && self.top_left.column <= other.top_left.column
            && self.bottom_right.line >= other.bottom_right.line
            && self.bottom_right.column >= other.bottom_right.column
    }
}

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
        self.view
            .draw(&mut self.buffer.sub_screen_buffer(ScreenArea {
                top_left: ScreenCoord::zero(),
                bottom_right: ScreenCoord {
                    line: self.buffer.height() - 3,
                    column: self.buffer.width() - 1,
                },
            }));
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
