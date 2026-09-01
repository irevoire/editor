use crossterm::style::{ContentStyle, StyledContent};
use jiff::{SignedDuration, Timestamp};

use crate::{
    config::Config,
    screen::{
        animation::LoopingAnimation,
        component::Component,
        screen_buffer::{Grapheme, IntoGraphemes, SubScreen},
        ScreenCoord,
    },
    GlobalContext,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FixedSizeTextOverflow {
    CropRight,
    CropLeft,
    Animate,
}

/// Lets you display text in a fixed size area on the terminal.
/// If you need to change the size of the area, call the resize method.
#[derive(Debug)]
pub struct FixedSizeText {
    /// The number of terminal cell available to display the text
    size: usize,
    /// The text to display
    text: String,
    /// The text splitted on its grapheme
    /// TODO: This could be a Vec<smallstr>
    graphemes: Vec<Grapheme>,
    /// The style of the text. It must be applied to every terminal cell
    style: ContentStyle,
    /// What the component should do in case the text is larger than the terminal
    /// its being displayed on.
    overflow: FixedSizeTextOverflow,
    /// The byte index we're displaying the string from
    displaying_from: usize,
    /// The byte index we stop displaying the text at
    displaying_to: usize,
    /// How long a single step of the animation should last
    animation_speed: SignedDuration,
    /// Store our step in the animation.
    /// If set to `None`, it means we've never been drawn.
    loop_anim: Option<LoopingAnimation>,
}

impl FixedSizeText {
    pub fn new(size: usize, overflow: FixedSizeTextOverflow, config: &Config) -> Self {
        Self {
            size,
            text: String::new(),
            graphemes: Vec::new(),
            style: ContentStyle::new(),
            overflow,
            displaying_from: 0,
            displaying_to: 0,
            animation_speed: config.get_status_bar_animation_speed(),
            loop_anim: None,
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn resize(&mut self, size: usize) {
        self.size = size;
        self.recompute_displaying_from_to();
    }

    pub fn overflow(&self) -> FixedSizeTextOverflow {
        self.overflow
    }

    pub fn set_overflow(&mut self, overflow: FixedSizeTextOverflow) {
        self.overflow = overflow;
        self.recompute_displaying_from_to();
    }

    pub fn set_style(&mut self, style: ContentStyle) {
        self.style = style;
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.graphemes = self.text.into_graphemes().collect();
        self.recompute_displaying_from_to();
    }

    fn recompute_displaying_from_to(&mut self) {
        self.loop_anim = None;
        match self.overflow {
            FixedSizeTextOverflow::CropRight | FixedSizeTextOverflow::Animate => {
                self.displaying_from = 0;
                self.displaying_to = self.size.min(self.graphemes.len());
            }
            FixedSizeTextOverflow::CropLeft => {
                self.displaying_from = self.graphemes.len().saturating_sub(self.size);
                self.displaying_to = self.graphemes.len();
            }
        }
    }
}

impl Component for FixedSizeText {
    /// Will panic if called from a screen smaller than the required size
    fn draw(&mut self, now: Timestamp, _ctx: &GlobalContext, screen: &mut SubScreen<'_>) {
        assert!(self.size() <= screen.width() as usize);

        let (from, to) = if self.overflow == FixedSizeTextOverflow::Animate {
            let anim = self
                .loop_anim
                .get_or_insert_with(|| LoopingAnimation::start(now, self.animation_speed));
            let step = anim.step(now);
            let width = self.displaying_to - self.displaying_from;
            (step, step + width)
        } else {
            (self.displaying_from, self.displaying_to)
        };

        for (idx, grapheme) in self
            .graphemes
            .iter()
            // if we are doing an animation we want to insert a space between
            // the end and the start.
            .chain(std::iter::once(&Grapheme::space()))
            .cycle()
            .skip(from)
            .take(to.abs_diff(from))
            .enumerate()
        {
            let coord = ScreenCoord {
                line: 0,
                column: idx as u16,
            };
            screen[coord] = StyledContent::new(self.style.clone(), grapheme.clone());
        }
    }

    fn next_wakeup(&self, now: Timestamp) -> Option<Timestamp> {
        if self.overflow != FixedSizeTextOverflow::Animate {
            return None;
        }
        match &self.loop_anim {
            Some(anim) => anim.next_wakeup(now),
            // This means we've never been drawn ever.
            None => Some(now),
        }
    }
}

#[cfg(test)]
mod test {

    use insta::assert_snapshot;
    use jiff::ToSpan;

    use crate::screen::screen_buffer::ScreenBuffer;

    use super::*;

    #[test]
    fn crud_fixed_sized_text() {
        let mut screen = ScreenBuffer::new(1, 10);
        let ctx = GlobalContext::default();

        let mut text = FixedSizeText::new(10, FixedSizeTextOverflow::CropLeft, &ctx.config);
        assert_eq!(text.text(), "");
        assert_eq!(text.size(), 10);
        assert_eq!(text.overflow(), FixedSizeTextOverflow::CropLeft);

        text.draw(Timestamp::UNIX_EPOCH, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"");

        text.set_text(String::from("Hello"));
        text.set_overflow(FixedSizeTextOverflow::CropRight);
        assert_eq!(text.text(), "Hello");
        assert_eq!(text.size(), 10);
        assert_eq!(text.overflow(), FixedSizeTextOverflow::CropRight);

        text.draw(Timestamp::UNIX_EPOCH, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello");
    }

    #[test]
    fn overflow_crop_fixed_sized_text() {
        let mut screen = ScreenBuffer::new(1, 15);
        let ctx = GlobalContext::default();

        let mut text = FixedSizeText::new(10, FixedSizeTextOverflow::CropRight, &ctx.config);
        text.set_text(String::from("Hello World!"));
        text.draw(Timestamp::UNIX_EPOCH, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello Worl");
        text.resize(11);
        text.draw(Timestamp::UNIX_EPOCH, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello World");

        text.set_overflow(FixedSizeTextOverflow::CropLeft);
        text.draw(Timestamp::UNIX_EPOCH, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"ello World!");
        text.resize(11);
        text.draw(Timestamp::UNIX_EPOCH, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"ello World!");
    }

    #[test]
    fn overflow_animate_fixed_sized_text() {
        let mut screen = ScreenBuffer::new(1, 15);
        let ctx = GlobalContext::default();

        let mut text = FixedSizeText::new(10, FixedSizeTextOverflow::Animate, &ctx.config);
        text.set_text(String::from("Hello World!"));
        let mut now = Timestamp::new(0, 0).unwrap();
        text.draw(now, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello Worl");
        text.draw(now, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello Worl");

        now += 400.milliseconds();
        text.draw(now, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello Worl");

        now += 150.milliseconds();
        text.draw(now, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"ello World");

        now += 10.seconds();
        text.draw(now, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"rld! Hello");

        let mut ret = String::new();
        for _ in 0..12 {
            now += 500.milliseconds();
            text.draw(now, &ctx, &mut screen.as_sub_screen());
            ret.push_str(&screen.display_as_text());
            ret.push('\n');
        }
        assert_snapshot!(ret, @"
        ld! Hello      
        d! Hello W     
        ! Hello Wo     
         Hello Wor     
        Hello Worl     
        ello World     
        llo World!     
        lo World!      
        o World! H     
         World! He     
        World! Hel     
        orld! Hell
        ");

        // By adding one char we could either show the h or the !. But we should
        // reset the animation and start again from the beginning.
        text.resize(11);
        text.draw(now, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello World");
    }

    #[test]
    fn animation_speed_is_read_from_config() {
        let mut screen = ScreenBuffer::new(1, 10);
        let ctx = GlobalContext::default();
        ctx.config
            .set_status_bar_animation_speed(jiff::SignedDuration::from_millis(100));

        let mut text = FixedSizeText::new(10, FixedSizeTextOverflow::Animate, &ctx.config);
        text.set_text(String::from("Hello World!"));
        let mut now = Timestamp::new(0, 0).unwrap();

        text.draw(now, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello Worl");

        // With the default 500ms step, 100ms wouldn't be enough to advance
        // the animation. With the configured 100ms step, it advances by one.
        now += 100.milliseconds();
        text.draw(now, &ctx, &mut screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"ello World");
    }
}
