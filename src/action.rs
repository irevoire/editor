use crate::Mode;

pub enum Action {
    Quit,
    FocusGained,
    FocusLost,
    Redraw,
    Paste(String),
    ChangeMode(Mode),
    MoveAnchor(Anchor, Direction),
    Insert(char),
}

pub enum Direction {
    Up,
    Down,
    Right,
    Left,

    StartOfLine,
    EndOfLine,
    StartOfFile,
    EndOfFile,

    PageUp,
    PageDown,
}

pub enum Anchor {
    Tail,
    Head,
}
