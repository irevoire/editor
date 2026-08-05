use crossterm::{
    event::{Event, KeyCode},
    style::{ContentStyle, StyledContent},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use env_logger::{Builder, Env, Target};

use std::{
    io::{self, BufWriter},
    panic::catch_unwind,
    sync::Arc,
};

use crate::{
    action::{Action, Anchor, Direction},
    screen::{screen_buffer::SubScreenBuffer, Screen},
    server::{Buffer, Server, ServerHandle},
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

#[derive(Default, Clone, Copy)]
pub struct Cursor {
    line: usize,
    column: usize,
}

#[derive(Default)]
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

pub struct Editor {
    server: ServerHandle,
    screen: Screen,
    mode: Mode,
}

pub struct BufferView {
    width: usize,
    height: usize,
    top_line: usize,
    selection: Selection,
    buffer: Arc<Buffer>,
}

impl BufferView {
    fn insert(&mut self, server: &ServerHandle, c: char) -> ActionResult {
        let mut rope = self.buffer.rope.blocking_write();
        let offset = rope.line_to_char(self.selection.head.line);
        let insert_at_char = offset + self.selection.head.column;
        rope.insert_char(insert_at_char, c);
        self.selection.head.column += 1;
        ActionResult::Redraw
    }

    fn redraw(&self, buffer: &mut SubScreenBuffer) {
        let rope = self.buffer.rope.blocking_read();
        let gutter_width = (self.top_line + buffer.height()).ilog10() + 1;
        for (line_idx, line) in rope.lines_at(self.top_line).enumerate() {
            let gutter = format!(
                "{:width$}|",
                self.top_line + line_idx,
                width = (gutter_width - 1) as usize
            );
            for (i, c) in gutter.chars().enumerate() {
                buffer[(line_idx, i)] = StyledContent::new(ContentStyle::new(), c);
            }
            for (i, c) in line.chars().enumerate() {
                let i = i + gutter_width as usize;
                buffer[(line_idx, i)] = StyledContent::new(ContentStyle::new(), c);
            }
        }
    }
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
            mode: Mode::Normal,
            server,
        }
    }

    /// Returns `true` if we should exit
    pub fn process_action(&mut self, action: Action) -> ActionResult {
        match action {
            Action::Quit => {
                self.server.stop();
                ActionResult::Exit
            }
            Action::FocusGained => self.screen.focus_gained(),
            Action::FocusLost => self.screen.focus_lost(),
            Action::Redraw => self.screen.redraw(),
            Action::Paste(_) => todo!(),
            Action::ChangeMode(mode) => {
                self.mode = mode;
                self.screen.change_mode(mode)
            }
            Action::MoveAnchor(anchor, direction) => self.screen.move_anchor(anchor, direction),
            Action::Insert(c) => self.screen.insert(c),
        }
    }

    pub fn redraw(&mut self) {
        self.screen.redraw();
    }

    fn event_to_action(&self, event: Event) -> Option<Action> {
        log::trace!("Received event {event:?}");
        match event {
            Event::FocusGained => Some(Action::FocusGained),
            Event::FocusLost => Some(Action::FocusLost),
            Event::Key(key_event) => match key_event.code {
                KeyCode::Backspace => todo!(),
                KeyCode::Enter if self.mode == Mode::Insert => Some(Action::Insert('\n')),
                KeyCode::Enter => None,
                KeyCode::Left => Some(Action::MoveAnchor(Anchor::Head, Direction::Left)),
                KeyCode::Right => Some(Action::MoveAnchor(Anchor::Head, Direction::Right)),
                KeyCode::Up => Some(Action::MoveAnchor(Anchor::Head, Direction::Up)),
                KeyCode::Down => Some(Action::MoveAnchor(Anchor::Head, Direction::Down)),
                KeyCode::Home => Some(Action::MoveAnchor(Anchor::Head, Direction::StartOfLine)),
                KeyCode::End => Some(Action::MoveAnchor(Anchor::Head, Direction::EndOfLine)),
                KeyCode::PageUp => Some(Action::MoveAnchor(Anchor::Head, Direction::PageUp)),
                KeyCode::PageDown => Some(Action::MoveAnchor(Anchor::Head, Direction::PageDown)),
                KeyCode::Tab if self.mode == Mode::Insert => Some(Action::Insert('\t')),
                KeyCode::Tab => None,
                KeyCode::BackTab => todo!(),
                KeyCode::Delete => todo!(),
                KeyCode::Insert => Some(Action::ChangeMode(Mode::Insert)),
                KeyCode::F(_) => todo!(),
                KeyCode::Char(c) if self.mode == Mode::Insert => Some(Action::Insert(c)),
                KeyCode::Char(c) => match c {
                    'q' => Some(Action::Quit),
                    'i' => Some(Action::ChangeMode(Mode::Insert)),
                    _ => None,
                },

                KeyCode::Null => todo!(),
                KeyCode::Esc => Some(Action::ChangeMode(Mode::Normal)),
                KeyCode::CapsLock => todo!(),
                KeyCode::ScrollLock => todo!(),
                KeyCode::NumLock => todo!(),
                KeyCode::PrintScreen => todo!(),
                KeyCode::Pause => todo!(),
                KeyCode::Menu => todo!(),
                KeyCode::KeypadBegin => todo!(),
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
                ActionResult::Exit => break,
                ActionResult::Nothing => (),
                ActionResult::Redraw => this.redraw(),
            }
        }
        log::info!("redraw the screen");
    }
}
