use crossterm::{
    ExecutableCommand,
    event::{Event, KeyCode},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use env_logger::{Builder, Env, Target};

use std::{
    cmp::Ordering,
    io::{self, BufWriter},
    panic::catch_unwind,
};

use crate::{
    action::{Action, Anchor, DeleteDirection, Direction},
    screen::{Screen, ScreenCoord, components::StatusBar},
    server::{Server, ServerHandle},
};

mod action;
mod screen;
mod server;
mod utils;

fn init_logger() {
    let env = Env::default()
        .filter("MY_LOG_LEVEL")
        .default_filter_or("trace")
        .write_style("MY_LOG_STYLE");

    let file = std::fs::File::create("logs.editor").unwrap();
    let writer = BufWriter::new(file);
    let writer = Box::new(writer);
    Builder::from_env(env).target(Target::Pipe(writer)).init();
}

fn main() {
    init_logger();

    enable_raw_mode().unwrap();

    let server = Server::new();
    let mut stdout = std::io::stdout();
    stdout.execute(EnterAlternateScreen).unwrap();

    let s = server.clone();
    let ret = catch_unwind(move || Editor::run(s, stdout));
    if let Err(panic) = ret {
        match panic.downcast::<&str>() {
            Ok(panic) => log::error!("{panic}"),
            Err(panic) => {
                if let Ok(panic) = panic.downcast::<String>() {
                    log::error!("{panic}");
                }
            }
        }
    }

    crossterm::execute!(std::io::stdout(), LeaveAlternateScreen).unwrap();
    let _ = disable_raw_mode();
    server.stop();

    let logs = std::fs::read_to_string("logs.editor").unwrap();
    println!("{logs}");
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    line: usize,
    column: usize,
}

impl Cursor {
    /// Convert a cursor pointing at a specific position in a file / buffer
    /// into a screen coordinate.
    /// The line offset is the line number that represents the top of the screen.
    #[track_caller]
    pub fn to_screen_coord(self, line_offset: usize) -> ScreenCoord {
        assert!(line_offset <= self.line);
        ScreenCoord {
            line: (self.line - line_offset) as u16,
            column: self.column as u16,
        }
    }
}

impl PartialOrd for Cursor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.line.partial_cmp(&other.line) {
            Some(Ordering::Equal) => self.column.partial_cmp(&other.column),
            ord => ord,
        }
    }
}

#[derive(Default, Clone, PartialEq, Eq)]
pub struct Selection {
    tail: Cursor,
    head: Cursor,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    #[default]
    Char,
}

pub struct Editor {
    server: ServerHandle,
    screen: Screen,
    context: GlobalContext,
}

#[derive(Default)]
pub struct GlobalContext {
    mode: Mode,
    selection_mode: SelectionMode,
    status_bar: StatusBar,
}

pub enum ActionResult {
    Redraw,
    Exit,
    Nothing,
}

impl Editor {
    pub fn new(server: ServerHandle, stdout: io::Stdout) -> Self {
        Self {
            screen: Screen::new(server.clone(), stdout),
            server,
            context: Default::default(),
        }
    }

    /// Returns `true` if we should exit
    pub fn process_action(&mut self, action: Action) -> io::Result<ActionResult> {
        match action {
            Action::Quit => {
                self.server.stop();
                Ok(ActionResult::Exit)
            }
            Action::FocusGained => Ok(self.screen.focus_gained()),
            Action::FocusLost => Ok(self.screen.focus_lost()),
            Action::Redraw => Ok(self.screen.draw(&self.context)),
            Action::Paste(_) => todo!(),
            Action::ChangeMode(mode) => {
                self.context.mode = mode;
                self.screen.change_mode(mode)
            }
            Action::MoveAnchor(anchor, direction) => {
                Ok(self
                    .screen
                    .move_anchor(anchor, direction, self.context.selection_mode))
            }
            Action::Insert(c) => Ok(self.screen.insert(c)),
            Action::Delete(delete_direction) => Ok(self.screen.delete(delete_direction)),
        }
    }

    pub fn redraw(&mut self) {
        self.screen.draw(&self.context);
    }

    fn event_to_action(&self, event: Event) -> Option<Action> {
        log::trace!("Received event {event:?}");
        match event {
            Event::FocusGained => Some(Action::FocusGained),
            Event::FocusLost => Some(Action::FocusLost),
            Event::Key(key_event) => match key_event.code {
                KeyCode::Backspace => Some(Action::Delete(DeleteDirection::Left)),
                KeyCode::Delete => Some(Action::Delete(DeleteDirection::Right)),
                KeyCode::Enter if self.context.mode == Mode::Insert => Some(Action::Insert('\n')),
                KeyCode::Enter => None,
                KeyCode::Left => Some(Action::MoveAnchor(Anchor::Head, Direction::Left)),
                KeyCode::Right => Some(Action::MoveAnchor(Anchor::Head, Direction::Right)),
                KeyCode::Up => Some(Action::MoveAnchor(Anchor::Head, Direction::Up)),
                KeyCode::Down => Some(Action::MoveAnchor(Anchor::Head, Direction::Down)),
                KeyCode::Home => Some(Action::MoveAnchor(Anchor::Head, Direction::StartOfLine)),
                KeyCode::End => Some(Action::MoveAnchor(Anchor::Head, Direction::EndOfLine)),
                KeyCode::PageUp => Some(Action::MoveAnchor(Anchor::Head, Direction::PageUp)),
                KeyCode::PageDown => Some(Action::MoveAnchor(Anchor::Head, Direction::PageDown)),
                KeyCode::Tab if self.context.mode == Mode::Insert => Some(Action::Insert('\t')),
                KeyCode::Tab => None,
                KeyCode::BackTab => todo!(),
                KeyCode::Insert => Some(Action::ChangeMode(Mode::Insert)),
                KeyCode::F(_) => None,
                KeyCode::Char(c) if self.context.mode == Mode::Insert => Some(Action::Insert(c)),
                KeyCode::Char(c) => match c {
                    'q' => Some(Action::Quit),
                    'i' => Some(Action::ChangeMode(Mode::Insert)),
                    _ => None,
                },

                KeyCode::Null => None,
                KeyCode::Esc => Some(Action::ChangeMode(Mode::Normal)),
                KeyCode::CapsLock => None,
                KeyCode::ScrollLock => None,
                KeyCode::NumLock => None,
                KeyCode::PrintScreen => None,
                KeyCode::Pause => None,
                KeyCode::Menu => None,
                KeyCode::KeypadBegin => None,
                KeyCode::Media(media_key_code) => todo!(),
                KeyCode::Modifier(modifier_key_code) => todo!(),
            },
            Event::Mouse(mouse_event) => None,
            Event::Paste(content) => Some(Action::Paste(content)),
            Event::Resize(_, _) => Some(Action::Redraw),
        }
    }

    pub fn run(server: ServerHandle, stdout: io::Stdout) {
        let mut this = Self::new(server, stdout);
        loop {
            let key = crossterm::event::read().unwrap();
            let Some(action) = this.event_to_action(key) else {
                continue;
            };
            let exit = this.process_action(action);
            match exit {
                Err(_) | Ok(ActionResult::Exit) => break,
                Ok(ActionResult::Nothing) => (),
                Ok(ActionResult::Redraw) => this.redraw(),
            }
        }
        log::info!("redraw the screen");
    }
}
