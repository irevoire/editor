use crate::Mode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    FocusGained,
    FocusLost,
    Redraw,
    Paste(String),
    ChangeMode(Mode),
    MoveAnchor(Anchor, Direction),
    Insert(char),
    Delete(DeleteDirection),
    OpenPopup,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DeleteDirection {
    Left,
    Right,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Anchor {
    Tail,
    Head,
}
