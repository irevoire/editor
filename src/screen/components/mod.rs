use crossterm::style::{ContentStyle, StyledContent, Stylize};

use crate::screen::screen_buffer::SubScreen;

/// The status bar at the bottom of the screen
#[derive(Default, Debug)]
pub struct StatusBar {
    position: StatusBarPosition,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
enum StatusBarPosition {
    #[default]
    Bottom,
}

impl StatusBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn draw<'a: 'b, 'b>(&self, mut screen: SubScreen<'a>) -> SubScreen<'b> {
        let (remaining, mut status_bar) = screen.split_after_col(screen.height() - 1);
        status_bar.fill(StyledContent::new(
            ContentStyle::new().on_dark_grey().white(),
            " ".to_string(),
        ));

        remaining
    }
}
