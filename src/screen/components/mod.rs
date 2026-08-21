use crossterm::style::{ContentStyle, StyledContent, Stylize};
use unicode_segmentation::UnicodeSegmentation;

use crate::screen::{screen_buffer::SubScreen, ScreenCoord};

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
    graphemes: Vec<String>,
    /// The style of the text. It must be applied to every terminal cell
    style: ContentStyle,
    /// What the component should do in case the text is larger than the terminal
    /// its being displayed on.
    overflow: FixedSizeTextOverflow,
    /// The byte index we're displaying the string from
    displaying_from: usize,
    /// The byte index we stop displaying the text at
    displaying_to: usize,
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
        self.graphemes = self.text.graphemes(true).map(|s| s.to_string()).collect();
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
    pub fn draw(&self, mut screen: SubScreen<'_>) {
        assert!(self.size() <= screen.width() as usize);
        for (idx, grapheme) in self
            .graphemes
            .iter()
            .skip(self.displaying_from)
            .take(self.displaying_to - self.displaying_from)
            .enumerate()
        {
            let coord = ScreenCoord {
                line: 0,
                column: idx as u16,
            };
            screen[coord] = StyledContent::new(self.style.clone(), grapheme.to_string());
        }
    }
}

#[cfg(test)]
mod test {

    use insta::assert_snapshot;

    use crate::screen::{screen_buffer::ScreenBuffer, Screen};

    use super::*;

    #[test]
    fn crud_fixed_sized_text() {
        let mut screen = ScreenBuffer::new(1, 10);

        let mut text = FixedSizeText::new(10, FixedSizeTextOverflow::CropLeft);
        assert_eq!(text.text(), "");
        assert_eq!(text.size(), 10);
        assert_eq!(text.overflow(), FixedSizeTextOverflow::CropLeft);

        text.draw(screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"");

        text.set_text(String::from("Hello"));
        text.set_overflow(FixedSizeTextOverflow::CropRight);
        assert_eq!(text.text(), "Hello");
        assert_eq!(text.size(), 10);
        assert_eq!(text.overflow(), FixedSizeTextOverflow::CropRight);

        text.draw(screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello");
    }

    #[test]
    fn overflow_crop_fixed_sized_text() {
        let mut screen = ScreenBuffer::new(1, 10);

        let mut text = FixedSizeText::new(10, FixedSizeTextOverflow::CropRight);
        text.set_text(String::from("Hello World!"));
        text.draw(screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"Hello Worl");

        text.set_overflow(FixedSizeTextOverflow::CropLeft);
        text.draw(screen.as_sub_screen());
        assert_snapshot!(screen.display_as_text(), @"llo World!");
    }
}
