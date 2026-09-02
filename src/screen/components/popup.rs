use crossterm::style::{ContentStyle, StyledContent, Stylize};
use jiff::{SignedDuration, Timestamp};

use crate::{
    screen::{
        animation::{ease_out_cubic, OneShotAnimation},
        component::Component,
        screen_buffer::{Grapheme, SubScreen},
        view::buffer_view::BufferView,
        ScreenArea, ScreenCoord,
    },
    GlobalContext,
};

/// The animation redraws at roughly this many frames per second, however
/// long its total duration ends up being.
const TARGET_FPS: i32 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupPosition {
    Top,
    Bottom,
    Center,
}

/// Where the popup currently is in its lifetime.
enum PopupAnimation {
    /// Never drawn yet: the entrance animation hasn't started.
    NotStarted,
    Entering(OneShotAnimation),
    /// Playing the entrance animation in reverse.
    Closing(OneShotAnimation),
}

/// A popup drawn on top of whatever `SubScreen` it's given, anchored to the
/// right edge, growing out of its resting bottom-right corner with a
/// [`OneShotAnimation`] instead of appearing all at once, and shrinking back
/// into it the same way when closed.
pub struct Popup {
    position: PopupPosition,
    content: BufferView,
    animation_duration: SignedDuration,
    animation: PopupAnimation,
}

impl Popup {
    pub fn new(
        position: PopupPosition,
        content: BufferView,
        animation_duration: SignedDuration,
    ) -> Self {
        Self {
            position,
            content,
            animation_duration,
            animation: PopupAnimation::NotStarted,
        }
    }

    /// Start closing the popup: plays the entrance animation in reverse.
    pub fn close(&mut self, now: Timestamp) {
        self.animation =
            PopupAnimation::Closing(OneShotAnimation::start(now, self.animation_duration));
    }

    /// Returns true if the popup is fully closed and can be removed.
    pub fn is_closed(&self, now: Timestamp) -> bool {
        matches!(&self.animation, PopupAnimation::Closing(anim) if anim.is_done(now))
    }

    /// Returns true if the popup is still playing the closing animation.
    pub fn is_closing(&self) -> bool {
        matches!(&self.animation, PopupAnimation::Closing(_))
    }

    /// The coordinate the popup rests at once its entrance animation is done.
    fn resting_bottom_right(&self, screen: &SubScreen<'_>) -> ScreenCoord {
        let column = screen.width() - 1;
        let line = match self.position {
            PopupPosition::Top => self.content.height as u16 - 1,
            PopupPosition::Bottom => screen.height() - 1,
            PopupPosition::Center => todo!(),
        };
        ScreenCoord { line, column }
    }

    /// Duration between two frame.
    fn frame_interval(&self) -> SignedDuration {
        let one_frame_at_target_fps = SignedDuration::from_secs(1) / TARGET_FPS;
        let frame_count = self
            .animation_duration
            .div_duration_f64(one_frame_at_target_fps)
            .round()
            .max(1.0) as i32;
        self.animation_duration / frame_count
    }
}

impl Component for Popup {
    fn draw(&mut self, now: Timestamp, _ctx: &GlobalContext, screen: &mut SubScreen<'_>) {
        if matches!(self.animation, PopupAnimation::NotStarted) {
            self.animation =
                PopupAnimation::Entering(OneShotAnimation::start(now, self.animation_duration));
        }
        let progress = match &self.animation {
            PopupAnimation::NotStarted => unreachable!("just started above"),
            PopupAnimation::Entering(anim) => ease_out_cubic(anim.progress(now)),
            PopupAnimation::Closing(anim) => 1.0 - ease_out_cubic(anim.progress(now)),
        };

        // Never let the popup grow bigger than the screen it's being drawn on.
        let full_width = (self.content.width as u16).min(screen.width());
        let full_height = (self.content.height as u16).min(screen.height());
        // Never let it shrink to nothing, so it always covers at least one cell.
        let width = ((full_width as f32 * progress).round() as u16).clamp(1, full_width);
        let height = ((full_height as f32 * progress).round() as u16).clamp(1, full_height);

        let bottom_right = self.resting_bottom_right(screen);
        let top_left = ScreenCoord {
            line: bottom_right.line + 1 - height,
            column: bottom_right.column + 1 - width,
        };
        let area = ScreenArea::new(top_left, bottom_right);
        let mut popup_screen = screen.sub_screen(area);

        // Fill the whole area first so the popup fully hides whatever was
        // drawn behind it, even where its content doesn't have enough lines
        // to cover every cell itself.
        popup_screen.fill(StyledContent::new(
            ContentStyle::new().on_dark_grey().white(),
            Grapheme::space(),
        ));
        self.content.draw_code(&mut popup_screen);
    }

    fn next_wakeup(&self, now: Timestamp) -> Option<Timestamp> {
        match &self.animation {
            // We've never been drawn: ask to be drawn ASAP to kick off the animation.
            PopupAnimation::NotStarted => Some(now),
            PopupAnimation::Entering(anim) | PopupAnimation::Closing(anim) => {
                anim.next_wakeup(now, self.frame_interval())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use insta::assert_snapshot;
    use ropey::Rope;
    use tokio::sync::RwLock;

    use super::*;
    use crate::{screen::screen_buffer::ScreenBuffer, server::Buffer, Selection};

    /// Fill the whole screen with a recognizable "code" pattern, standing in
    /// for whatever the rest of the editor draws behind the popup.
    fn draw_code(screen: &mut ScreenBuffer) {
        screen
            .as_sub_screen()
            .fill(StyledContent::new(ContentStyle::new(), Grapheme::from('.')));
    }

    fn popup_content(text: &str, width: usize, height: usize) -> BufferView {
        BufferView {
            width,
            height,
            top_line: 0,
            active: false,
            selection: Selection::default(),
            buffer: Arc::new(Buffer {
                name: String::from("*popup*"),
                path: None,
                rope: RwLock::new(Rope::from_str(text)),
            }),
        }
    }

    #[test]
    fn popup_opens_then_closes_revealing_the_code_behind_it_again() {
        let mut screen = ScreenBuffer::new(8, 20);
        let ctx = GlobalContext::default();
        let content = popup_content("AA\nBB\nCC", 8, 4);
        let mut popup = Popup::new(
            PopupPosition::Bottom,
            content,
            SignedDuration::from_millis(200),
        );

        let t0 = Timestamp::new(0, 0).unwrap();
        let mut opening = String::new();
        for step in 0..=4 {
            let now = t0 + SignedDuration::from_millis(50) * step;
            // The rest of the screen gets redrawn every frame too, exactly
            // like `Screen::draw` does: code first, then popups on top.
            draw_code(&mut screen);
            popup.draw(now, &ctx, &mut screen.as_sub_screen());
            opening.push_str(&screen.display_as_text());
            opening.push_str("\n====\n");
        }
        assert_snapshot!(opening, @"
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        ...................0
        ====
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        ...............0| AA
        ...............1| BB
        ====
        ....................
        ....................
        ....................
        ....................
        .............0| AA  
        .............1| BB  
        .............2| CC  
        .............       
        ====
        ....................
        ....................
        ....................
        ....................
        ............0| AA   
        ............1| BB   
        ............2| CC   
        ............        
        ====
        ....................
        ....................
        ....................
        ....................
        ............0| AA   
        ............1| BB   
        ............2| CC   
        ............        
        ====
        ");

        let close_start = t0 + SignedDuration::from_millis(200);
        popup.close(close_start);
        let mut closing = String::new();
        for step in 0..=4 {
            let now = close_start + SignedDuration::from_millis(50) * step;
            draw_code(&mut screen);
            popup.draw(now, &ctx, &mut screen.as_sub_screen());
            closing.push_str(&screen.display_as_text());
            closing.push_str("\n====\n");
        }
        assert_snapshot!(closing, @"
        ....................
        ....................
        ....................
        ....................
        ............0| AA   
        ............1| BB   
        ............2| CC   
        ............        
        ====
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        .................0| 
        .................1| 
        ====
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        ...................0
        ====
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        ...................0
        ====
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        ....................
        ...................0
        ====
        ");
        assert!(popup.is_closed(close_start + SignedDuration::from_millis(200)));
    }

    #[test]
    fn next_wakeup_transitions_to_none_once_the_animation_is_done() {
        let mut screen = ScreenBuffer::new(8, 20);
        let ctx = GlobalContext::default();
        let content = popup_content("AA", 6, 3);
        let mut popup = Popup::new(
            PopupPosition::Bottom,
            content,
            SignedDuration::from_millis(200),
        );

        let t0 = Timestamp::new(0, 0).unwrap();
        // Before the first draw, the popup hasn't started its clock yet.
        assert_eq!(popup.next_wakeup(t0), Some(t0));

        popup.draw(t0, &ctx, &mut screen.as_sub_screen());
        // The frame interval is derived from the animation's own 200ms
        // duration split into ~60fps steps (200ms / 12 frames = 16.666...ms).
        assert_eq!(
            popup.next_wakeup(t0 + SignedDuration::from_millis(100)),
            Some(t0 + SignedDuration::from_nanos(116_666_666))
        );
        assert!(popup.is_animating(t0 + SignedDuration::from_millis(100)));

        let done_at = t0 + SignedDuration::from_millis(200);
        assert_eq!(popup.next_wakeup(done_at), None);
        assert!(!popup.is_animating(done_at));
        assert_eq!(
            popup.next_wakeup(done_at + SignedDuration::from_secs(1)),
            None
        );
    }

    #[test]
    fn a_popup_taller_than_its_content_still_fully_hides_the_code_behind_it() {
        let mut screen = ScreenBuffer::new(8, 20);
        let ctx = GlobalContext::default();
        draw_code(&mut screen);
        // A single line of content in a 5-line-tall popup: without the
        // background fill, 4 rows of "." would still be visible.
        let content = popup_content("Hi", 8, 5);
        let mut popup = Popup::new(
            PopupPosition::Bottom,
            content,
            SignedDuration::from_millis(200),
        );

        let t0 = Timestamp::new(0, 0).unwrap();
        // First draw starts the entrance animation's clock; draw again once
        // its duration has fully elapsed so it's at rest.
        popup.draw(t0, &ctx, &mut screen.as_sub_screen());
        popup.draw(
            t0 + SignedDuration::from_millis(200),
            &ctx,
            &mut screen.as_sub_screen(),
        );

        assert_snapshot!(screen.display_as_text(), @"
        ....................
        ....................
        ....................
        ............0| Hi   
        ............        
        ............        
        ............        
        ............
        ");
    }
}
