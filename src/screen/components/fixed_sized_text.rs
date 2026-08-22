use crossterm::style::{ContentStyle, StyledContent};
use jiff::Timestamp;
use unicode_segmentation::UnicodeSegmentation;

use crate::screen::{
    screen_buffer::{Grapheme, IntoGraphemes, SubScreen},
    ScreenCoord,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FixedSizeTextOverflow {
    CropRight,
    CropLeft,
    Animate,
}

/// Lets you display text in a fixed size area on the terminal.
/// If you need to change the size of the area, call the resize method.
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
    /// Last time the text was displayed
    displayed_at: Timestamp,
}

impl FixedSizeText {
    pub fn new(size: usize, overflow: FixedSizeTextOverflow) -> Self {
        Self {
            size,
            text: String::new(),
            graphemes: Vec::new(),
            style: ContentStyle::new(),
            overflow,
            displaying_from: 0,
            displaying_to: 0,
            displayed_at: Timestamp::MIN,
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn resize(&mut self, size: usize) {
        self.size = size;
        self.compute_displaying_from_to();
    }

    pub fn overflow(&self) -> FixedSizeTextOverflow {
        self.overflow
    }

    pub fn set_overflow(&mut self, overflow: FixedSizeTextOverflow) {
        self.overflow = overflow;
        self.compute_displaying_from_to();
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.graphemes = self.text.into_graphemes().collect();
        self.compute_displaying_from_to();
    }

    fn compute_displaying_from_to(&mut self) {
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

    /// Will panic if called from a screen smaller than the required size
    pub fn draw(&mut self, now: Timestamp, mut screen: SubScreen<'_>) {
        assert!(self.size() <= screen.width() as usize);
        if self.displayed_at == Timestamp::MIN {
            self.displayed_at = now;
        } else if now.duration_since(self.displayed_at).as_secs_f32() >= 0.5 {
            self.displaying_from = self.displaying_from + 1 % self.graphemes.len();
            self.displaying_to = self.displaying_to + 1 % self.graphemes.len();
        }
        for (idx, grapheme) in self
            .graphemes
            .iter()
            // if we are doing an animation we want to insert a space between
            // the end and the start.
            .chain(std::iter::once(&Grapheme::space()))
            .cycle()
            .skip(self.displaying_from)
            .take(self.displaying_to.abs_diff(self.displaying_from))
            .enumerate()
        {
            let coord = ScreenCoord {
                line: 0,
                column: idx as u16,
            };
            screen[coord] = StyledContent::new(self.style.clone(), grapheme.clone());
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

        let mut text = FixedSizeText::new(10, FixedSizeTextOverflow::CropLeft);
        assert_eq!(text.text(), "");
        assert_eq!(text.size(), 10);
        assert_eq!(text.overflow(), FixedSizeTextOverflow::CropLeft);

        text.draw(Timestamp::UNIX_EPOCH, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"");

        text.set_text(String::from("Hello"));
        text.set_overflow(FixedSizeTextOverflow::CropRight);
        assert_eq!(text.text(), "Hello");
        assert_eq!(text.size(), 10);
        assert_eq!(text.overflow(), FixedSizeTextOverflow::CropRight);

        text.draw(Timestamp::UNIX_EPOCH, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello");
    }

    #[test]
    fn overflow_crop_fixed_sized_text() {
        let mut screen = ScreenBuffer::new(1, 10);

        let mut text = FixedSizeText::new(10, FixedSizeTextOverflow::CropRight);
        text.set_text(String::from("Hello World!"));
        text.draw(Timestamp::UNIX_EPOCH, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello Worl");

        text.set_overflow(FixedSizeTextOverflow::CropLeft);
        text.draw(Timestamp::UNIX_EPOCH, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"llo World!");
    }

    #[test]
    fn overflow_animate_fixed_sized_text() {
        let mut screen = ScreenBuffer::new(1, 10);

        let mut text = FixedSizeText::new(10, FixedSizeTextOverflow::Animate);
        text.set_text(String::from("Hello World!"));
        let mut now = Timestamp::new(0, 0).unwrap();
        text.draw(now, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello Worl");

        now += 400.milliseconds();
        text.draw(now, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello Worl");

        now += 150.milliseconds();
        text.draw(now, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"ello World");

        now += 10.seconds();
        text.draw(now, screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"llo World!");

        let mut ret = String::new();
        for _ in 0..12 {
            now += 500.milliseconds();
            text.draw(now, screen.as_sub_screen());
            ret.push_str(&screen.display_as_text());
            ret.push('\n');
        }
        assert_snapshot!(ret, @r"
        lo World! 
        o World! H
         World! He
        World! Hel
        orld! Hell
        rld! Hello
        ld! Hello 
        d! Hello W
        ! Hello Wo
         Hello Wor
        Hello Worl
        ello World
        ");
    }
}
