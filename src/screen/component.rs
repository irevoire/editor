use jiff::Timestamp;

use crate::{screen::screen_buffer::SubScreen, GlobalContext};

/// Something that can be drawn to a [`SubScreen`], given the current time.
pub trait Component {
    fn draw(&mut self, now: Timestamp, ctx: &GlobalContext, screen: &mut SubScreen<'_>);

    /// The next timestamp at which this component wants to be redrawn.
    /// This doesn't give any guarantee on when you'll be drawn next
    /// It could be before or after the specified timestamp, it's just
    /// used as a hint.
    /// By default it never asks to be redrawn.
    fn next_wakeup(&self, now: Timestamp) -> Option<Timestamp> {
        let _ = now;
        None
    }

    /// Return `true` if an animation is still going on.
    fn is_animating(&self, now: Timestamp) -> bool {
        self.next_wakeup(now).is_some()
    }
}
