use crossterm::{
    cursor::SetCursorStyle,
    style::{ContentStyle, Stylize},
    ExecutableCommand, QueueableCommand,
};
use jiff::{SignedDuration, Timestamp};
use std::io;

use crate::{
    action::{Anchor, DeleteDirection, Direction},
    screen::{
        component::Component,
        components::{Popup, PopupPosition, StatusBar},
        screen_buffer::ScreenBuffer,
        view::buffer_view::BufferView,
    },
    server::ServerHandle,
    ActionResult, GlobalContext, Selection, SelectionMode,
};
use config::Config;

pub mod animation;
pub mod component;
pub mod components;
mod geo;
pub mod screen_buffer;
pub mod view;

pub use geo::*;

/// How long a popup takes to grow into place.
/// TODO: make this configurable.
const POPUP_ANIMATION_DURATION: SignedDuration = SignedDuration::from_millis(200);

pub struct Screen {
    stdout: io::Stdout,
    buffer: ScreenBuffer,
    server: ServerHandle,
    view: BufferView,
    popups: Vec<Popup>,

    // Default components we always display on screen
    status_bar: StatusBar,
}

impl Screen {
    pub fn new(server: ServerHandle, stdout: io::Stdout, config: &Config) -> Screen {
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
                background: ContentStyle::new(),
            },
            buffer: ScreenBuffer::new(row, col),
            popups: Vec::new(),
            status_bar: StatusBar::new(config),
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
        self.popups.retain_mut(|popup| {
            popup.draw(now, ctx, &mut sub_screen);
            !popup.is_closed(now)
        });

        self.buffer.display_on_screen(&mut self.stdout).unwrap();
        ActionResult::Nothing
    }

    pub fn next_wakeup(&self, now: jiff::Timestamp) -> Option<Timestamp> {
        let popups_wakeup = self
            .popups
            .iter()
            .filter_map(|popup| popup.next_wakeup(now))
            .min();
        [self.status_bar.next_wakeup(now), popups_wakeup]
            .into_iter()
            .flatten()
            .min()
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

    /// Opens a new popup, unless one is already open, in which case it closes it instead.
    pub fn open_popup(&mut self, now: Timestamp) -> ActionResult {
        if let Some(popup) = self.popups.last_mut() {
            if !popup.is_closing() {
                popup.close(now);
            }
            return ActionResult::Redraw;
        }

        let (_buffer_id, buffer) = self.server.create_scratch_buffer();
        buffer.rope.blocking_write().insert(0, "popup");

        let content = BufferView {
            width: 20,
            height: 5,
            top_line: 0,
            active: false,
            selection: Selection::default(),
            buffer,
            // The popup owns its own background: fill it with a distinct
            // color so it reads as an overlay on top of the code behind it.
            background: ContentStyle::new().on_dark_grey().white(),
        };
        self.popups.push(Popup::new(
            PopupPosition::Bottom,
            content,
            POPUP_ANIMATION_DURATION,
        ));
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
