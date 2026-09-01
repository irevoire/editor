use crossterm::style::{ContentStyle, StyledContent, Stylize};
use jiff::Timestamp;

use crate::{
    config::Config,
    screen::{
        component::Component,
        screen_buffer::{Grapheme, SubScreen},
    },
    GlobalContext,
};

mod fixed_sized_text;
pub use fixed_sized_text::*;

/// The status bar at the bottom of the screen
#[derive(Debug)]
pub struct StatusBar {
    position: StatusBarPosition,
    mode: FixedSizeText,
}

/// The number of terminal cells reserved to display the current mode.
const MODE_WIDTH: usize = 5;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
enum StatusBarPosition {
    #[default]
    Bottom,
}

impl StatusBar {
    pub fn new(config: &Config) -> Self {
        let mut mode = FixedSizeText::new(MODE_WIDTH, FixedSizeTextOverflow::Animate, config);
        mode.set_style(ContentStyle::new().on_dark_grey().white());

        Self {
            position: StatusBarPosition::default(),
            mode,
        }
    }

    pub fn draw<'a: 'b, 'b>(
        &mut self,
        now: jiff::Timestamp,
        ctx: &GlobalContext,
        mut screen: SubScreen<'a>,
    ) -> SubScreen<'b> {
        let (remaining, mut status_bar) = screen.split_after_col(screen.height() - 1);
        status_bar.fill(StyledContent::new(
            ContentStyle::new().on_dark_grey().white(),
            Grapheme::space(),
        ));

        // TODO: For now we hard code the size mode, it should be configurable
        if self.mode.size() != 5 {
            self.mode.resize(5);
        }
        // Check if the mode changed
        if self.mode.text() != ctx.mode.as_str() {
            self.mode.set_text(ctx.mode.as_str().to_string());
        }
        let (mut mode_view, _remaining) = status_bar.split_after_col(5 as u16);
        self.mode.draw(now, ctx, &mut mode_view);

        remaining
    }

    /// The next timestamp at which the status bar wants to be redrawn, if it's
    /// currently animating.
    pub fn next_wakeup(&self, now: Timestamp) -> Option<Timestamp> {
        self.mode.next_wakeup(now)
    }
}

#[cfg(test)]
mod test {
    use insta::assert_snapshot;
    use jiff::ToSpan;

    use crate::{screen::screen_buffer::ScreenBuffer, Mode};

    use super::*;

    #[test]
    fn status_bar_displays_current_mode() {
        let mut screen = ScreenBuffer::new(1, 15);
        let mut ctx = GlobalContext::default();
        let mut status_bar = StatusBar::new(&ctx.config);

        ctx.mode = Mode::Normal;
        status_bar.draw(Timestamp::UNIX_EPOCH, &ctx, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @" norma         ");

        ctx.mode = Mode::Insert;
        status_bar.draw(Timestamp::UNIX_EPOCH, &ctx, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @" inser         ");
    }

    #[test]
    fn status_bar_animates_mode_that_overflows() {
        let mut screen = ScreenBuffer::new(1, 15);
        let mut ctx = GlobalContext::default();
        let mut status_bar = StatusBar::new(&ctx.config);
        ctx.mode = Mode::Insert;

        let mut now = Timestamp::UNIX_EPOCH;
        status_bar.draw(now, &ctx, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @" inser         ");

        now += 500.milliseconds();
        status_bar.draw(now, &ctx, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @" nsert         ");

        now += 500.milliseconds();
        status_bar.draw(now, &ctx, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @" sert          ");
    }
}
