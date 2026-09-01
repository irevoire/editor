use crossterm::{cursor::SetCursorStyle, ExecutableCommand, QueueableCommand};
use jiff::Timestamp;
use std::io;

use crate::{
    action::{Anchor, DeleteDirection, Direction},
    screen::{
        components::StatusBar,
        screen_buffer::{ScreenBuffer, SubScreen},
        view::buffer_view::BufferView,
    },
    server::ServerHandle,
    ActionResult, GlobalContext, Selection, SelectionMode,
};

pub mod animation;
pub mod component;
pub mod components;
mod geo;
pub mod screen_buffer;
pub mod view;

pub use geo::*;

pub struct Screen {
    stdout: io::Stdout,
    buffer: ScreenBuffer,
    server: ServerHandle,
    view: BufferView,
    popups: Vec<Popup>,

    // Default components we always display on screen
    status_bar: StatusBar,
}

pub enum PopupPosition {
    Top,
    Bottom,
    Center,
}

pub struct Popup {
    position: PopupPosition,
    content: BufferView,
}
impl Popup {
    fn draw(&self, screen: &mut SubScreen<'_>) {
        // We want the top and bottom popups to be as far away on the right as possible

        let area = match self.position {
            PopupPosition::Top => ScreenArea::new(
                ScreenCoord {
                    line: 0,
                    column: screen
                        .width()
                        .saturating_sub(self.content.width as u16)
                        .max(screen.width() / 2),
                },
                ScreenCoord {
                    line: self.content.height as u16,
                    column: screen.width(),
                },
            ),
            PopupPosition::Bottom => ScreenArea::new(
                ScreenCoord {
                    line: screen.height() - self.content.height as u16,
                    column: screen
                        .width()
                        .saturating_sub(self.content.width as u16)
                        .max(screen.width() / 2),
                },
                ScreenCoord {
                    line: screen.height(),
                    column: screen.width(),
                },
            ),
            PopupPosition::Center => todo!(),
        };
        let mut sub_screen = screen.sub_screen(area);
        self.content.draw_code(&mut sub_screen);
    }
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
            popups: Vec::new(),
            status_bar: StatusBar::default(),
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

    pub fn draw(&mut self, now: Timestamp, ctx: &GlobalContext) -> ActionResult {
        let mut sub_screen = self.buffer.as_sub_screen();

        let (mut tab_view, mut rem) = sub_screen.split_after_line(0);
        let (mut code, status) = rem.split_after_line(rem.height() - 2);
        self.status_bar.draw(now, &ctx, status);

        self.view.draw_tab(&mut tab_view);
        self.view.draw_code(&mut code);
        for popup in self.popups.iter_mut() {
            popup.draw(&mut sub_screen);
        }

        self.buffer.display_on_screen(&mut self.stdout).unwrap();
        ActionResult::Nothing
    }

    pub fn next_wakeup(&self, now: jiff::Timestamp) -> Option<Timestamp> {
        self.status_bar.next_wakeup(now)
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

    pub fn open_popup(&mut self) -> ActionResult {
        let (_buffer_id, buffer) = self.server.create_scratch_buffer();
        buffer.rope.blocking_write().insert(0, "popup");

        let content = BufferView {
            width: 20,
            height: 5,
            top_line: 0,
            active: false,
            selection: Selection::default(),
            buffer,
        };
        self.popups.push(Popup {
            position: PopupPosition::Bottom,
            content,
        });
        ActionResult::Redraw
    }

    pub fn change_mode(&mut self, mode: crate::Mode) -> io::Result<ActionResult> {
        let cursor_shape = match mode {
            crate::Mode::Normal => SetCursorStyle::DefaultUserShape,
            crate::Mode::Insert => SetCursorStyle::BlinkingBar,
        };
        self.stdout.queue(cursor_shape)?;
        Ok(ActionResult::Redraw)
    }
}
