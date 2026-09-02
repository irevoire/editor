use std::{
    fmt,
    io::{self, Write},
    marker::PhantomData,
    ops::{self, Deref},
};

use crossterm::{
    QueueableCommand,
    cursor::MoveTo,
    style::{ContentStyle, PrintStyledContent, StyledContent},
};
use unicode_segmentation::UnicodeSegmentation;

use crate::screen::{ScreenArea, ScreenCoord};

/// A grapheme is a string that can be represented on a single terminal cell.
#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Grapheme(String);

impl Grapheme {
    pub fn new() -> Grapheme {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.0.clear()
    }

    /// Return a single space " " as a grapheme.
    pub fn space() -> Grapheme {
        Grapheme(String::from(" "))
    }

    /// Returns `true` if the char was pushed to the grapheme.
    /// Returns `false` otherwise.
    pub fn push(&mut self, c: char) -> bool {
        self.0.push(c);
        if self.0.graphemes(true).count() == 1 {
            true
        } else {
            self.0.pop();
            false
        }
    }

    /// Returns `true` if the string was pushed to the grapheme.
    /// Returns `false` otherwise.
    pub fn push_str(&mut self, s: &str) -> bool {
        let len = self.0.len();
        self.0.push_str(s);
        if self.0.graphemes(true).count() == 1 {
            true
        } else {
            self.0.shrink_to(len);
            false
        }
    }
}

impl From<String> for Grapheme {
    #[track_caller]
    fn from(s: String) -> Self {
        let count = s.graphemes(true).count() == 1;
        if count {
            Grapheme(s)
        } else {
            panic!("Expected a single grapheme but instead got {count}")
        }
    }
}

impl From<&str> for Grapheme {
    #[track_caller]
    fn from(s: &str) -> Self {
        Grapheme::from(s.to_string())
    }
}

impl From<char> for Grapheme {
    #[track_caller]
    fn from(c: char) -> Self {
        Grapheme::from(c.to_string())
    }
}

impl fmt::Display for Grapheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Deref for Grapheme {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait IntoGraphemes {
    fn into_graphemes(&self) -> impl Iterator<Item = Grapheme>;
}

/*
impl<T> IntoGraphemes for T
where
    T: UnicodeSegmentation,
{
    fn into_graphemes(&self) -> impl Iterator<Item = Grapheme> {
        self.graphemes(true)
            .map(|grapheme| Grapheme(grapheme.to_string()))
    }
}
*/

impl IntoGraphemes for str {
    fn into_graphemes(&self) -> impl Iterator<Item = Grapheme> {
        self.graphemes(true)
            .map(|grapheme| Grapheme(grapheme.to_string()))
    }
}

struct Graphemes<'a> {
    inner: unicode_segmentation::Graphemes<'a>,
}

/// Represents everything that is currently on the screen, or that will be printed.
/// It's an abstraction over direct calls to help with tests, debugging, and avoiding doing useless
/// syscalls when it's not needed.
/// I assume a terminal screen will never be huge, so overall it shouldn't cost much
/// to duplicate it.
/// As long as we don't try to edit the same cell twice at the same time, it should
/// also be fairly easy to share this buffer between multiple screens.
#[derive(Debug)]
pub struct ScreenBuffer {
    area: ScreenArea,
    cursor: ScreenCoord,
    buffer: Vec<StyledContent<Grapheme>>,
}

impl ScreenBuffer {
    pub fn new(lines: u16, columns: u16) -> Self {
        let c = StyledContent::new(ContentStyle::new(), Grapheme::from(" "));
        Self {
            area: ScreenArea::new(
                ScreenCoord::zero(),
                // The area includes everything
                ScreenCoord {
                    line: lines - 1,
                    column: columns - 1,
                },
            ),
            cursor: ScreenCoord::zero(),
            buffer: vec![c; (lines * columns) as usize],
        }
    }

    pub fn height(&self) -> u16 {
        self.area.height()
    }

    pub fn width(&self) -> u16 {
        self.area.width()
    }

    pub fn display_as_text(&self) -> String {
        let mut output = String::new();
        for (idx, c) in self.buffer.iter().enumerate() {
            let idx = idx as u16;
            if idx != 0 && idx % self.width() == 0 {
                output.push('\n');
            }
            if c.content().is_empty() {
                output.push(' ');
            } else {
                output.push_str(c.content());
            }
        }
        output
    }

    pub fn display_on_screen(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        stdout.queue(MoveTo(0, 0))?;
        let mut line = 0;
        for (idx, content) in self.buffer.iter().enumerate() {
            let idx = idx as u16;
            if idx != 0 && idx % self.width() == 0 {
                line += 1;
                stdout.queue(MoveTo(0, line))?;
            }

            stdout.queue(PrintStyledContent(content.clone()))?;
        }
        stdout.queue(MoveTo(self.cursor.column as u16, self.cursor.line as u16))?;
        stdout.flush()
    }

    pub fn get(&self, coord: ScreenCoord) -> Option<&StyledContent<Grapheme>> {
        if coord.line >= self.area.height() || coord.column >= self.area.width() {
            None
        } else {
            self.buffer
                .get((coord.line * self.area.width() + coord.column) as usize)
        }
    }

    pub fn get_mut(&mut self, coord: ScreenCoord) -> Option<&mut StyledContent<Grapheme>> {
        if coord.line >= self.area.height() || coord.column >= self.area.width() {
            None
        } else {
            self.buffer
                .get_mut((coord.line * self.area.width() + coord.column) as usize)
        }
    }

    /// Convert the `ScreenBuffer` to a `SubScreen` covering the whole screen.
    pub fn as_sub_screen<'a>(&'a mut self) -> SubScreen<'a> {
        SubScreen {
            area: ScreenArea::new(
                ScreenCoord::zero(),
                ScreenCoord {
                    line: self.height() - 1,
                    column: self.width() - 1,
                },
            ),
            screen_buffer: self as *mut Self,
            _marker: PhantomData,
        }
    }
}

impl ops::Index<ScreenCoord> for ScreenBuffer {
    type Output = StyledContent<Grapheme>;

    #[track_caller]
    fn index(&self, coord: ScreenCoord) -> &Self::Output {
        match self.get(coord) {
            Some(grapheme) => grapheme,
            None => panic!(
                "Overflow: Tried to retrieve the grapheme {coord:?} in a buffer of dimensions: ({}, {})",
                self.area.height(),
                self.area.width()
            ),
        }
    }
}

impl ops::IndexMut<ScreenCoord> for ScreenBuffer {
    #[track_caller]
    fn index_mut(&mut self, coord: ScreenCoord) -> &mut Self::Output {
        let area = self.area;
        match self.get_mut(coord) {
            Some(grapheme) => grapheme,
            None => panic!(
                "Overflow: Tried to retrieve the grapheme {coord:?} in a buffer of dimensions: ({}, {})",
                area.height(),
                area.width()
            ),
        }
    }
}

#[derive(Debug)]
pub struct SubScreen<'a> {
    screen_buffer: *mut ScreenBuffer,
    area: ScreenArea,
    _marker: PhantomData<&'a mut ScreenBuffer>,
}

impl<'a> SubScreen<'a> {
    pub fn height(&self) -> u16 {
        self.area.height()
    }

    pub fn width(&self) -> u16 {
        self.area.width()
    }

    /// This function is wildly unsafe and MUST NOT be called twice from two different
    /// sub-screen.
    /// It's your job to make sure only one SubScreenBuffer will ever try to set the cursor.
    /// Trying to set the cursor from two different SubScreenBuffer is UB.
    pub unsafe fn set_cursor(&mut self, cursor: ScreenCoord) {
        assert!(self.area.contains_internal_coord(cursor));
        let cursor = self.area.translate_internal_coord(cursor);
        let screen_buffer = unsafe { &mut (*self.screen_buffer) };
        screen_buffer.cursor = cursor;
    }

    /// Split the screen vertically right after the specified column.
    /// Return the left half and the right half. There is nothing in between.
    /// Panics if the column is larger than the `width` of the screen.
    pub fn split_after_col<'b>(&mut self, column: u16) -> (SubScreen<'b>, SubScreen<'b>) {
        assert!(self.area.width() > column);

        let (left, right) = self.area.split_after_internal_column(column);

        let left = SubScreen {
            screen_buffer: self.screen_buffer,
            area: left,
            _marker: PhantomData,
        };

        let right = SubScreen {
            screen_buffer: self.screen_buffer,
            area: right,
            _marker: PhantomData,
        };

        (left, right)
    }

    /// Split the screen horizontally right after the specified line.
    /// Return the top half and the bottom half. There is nothing in between.
    /// Panics if the line is larger than the `height` of the screen.
    pub fn split_after_line(&mut self, line: u16) -> (SubScreen<'_>, SubScreen<'_>) {
        assert!(self.area.height() > line);

        let (top, bottom) = self.area.split_after_internal_line(line);

        let top = SubScreen {
            screen_buffer: self.screen_buffer,
            area: top,
            _marker: PhantomData,
        };

        let bottom = SubScreen {
            screen_buffer: self.screen_buffer,
            area: bottom,
            _marker: PhantomData,
        };

        (top, bottom)
    }

    pub fn sub_screen(&mut self, area: ScreenArea) -> SubScreen<'_> {
        assert!(
            self.area.contains_internal_area(area),
            "Can't create a sub screen of\n{area:?}\nfrom the an initial sub screen of\n{:?}",
            self.area
        );

        SubScreen {
            screen_buffer: self.screen_buffer,
            area: self.area.shrink_to_internal_area(area),
            _marker: PhantomData,
        }
    }

    pub fn get(&self, coord: ScreenCoord) -> Option<&StyledContent<Grapheme>> {
        if !self.area.contains_internal_coord(coord) {
            None
        } else {
            let coord = self.area.translate_internal_coord(coord);
            let screen_buffer = unsafe { &*self.screen_buffer };
            screen_buffer.get(coord)
        }
    }

    pub fn get_mut(&mut self, coord: ScreenCoord) -> Option<&mut StyledContent<Grapheme>> {
        if !self.area.contains_internal_coord(coord) {
            None
        } else {
            let coord = self.area.translate_internal_coord(coord);
            let screen_buffer = unsafe { &mut *self.screen_buffer };
            screen_buffer.get_mut(coord)
        }
    }

    pub fn fill(&mut self, content: StyledContent<Grapheme>) {
        for coord in self.area.iter() {
            self[coord] = content.clone();
        }
    }
}

impl<'a> ops::Index<ScreenCoord> for SubScreen<'a> {
    type Output = StyledContent<Grapheme>;

    #[track_caller]
    fn index(&self, coord: ScreenCoord) -> &Self::Output {
        match self.get(coord) {
            None => panic!(
                "Overflow: Tried to retrieve the character {coord:?} in a sub buffer of dimensions: ({}, {})",
                self.area.height(),
                self.area.width()
            ),
            Some(grapheme) => grapheme,
        }
    }
}

impl<'a> ops::IndexMut<ScreenCoord> for SubScreen<'a> {
    #[track_caller]
    fn index_mut(&mut self, coord: ScreenCoord) -> &mut Self::Output {
        let area = self.area;
        match self.get_mut(coord) {
            None => panic!(
                "Overflow: Tried to retrieve the character {coord:?} in a sub buffer of dimensions: ({}, {})",
                area.height(),
                area.width()
            ),
            Some(grapheme) => grapheme,
        }
    }
}

#[cfg(test)]
mod test {
    use insta::assert_snapshot;

    use super::*;

    #[test]
    fn basic_print() {
        let mut screen = ScreenBuffer::new(10, 10);

        for (i, c) in "status bar".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 8,
                column: i as u16,
            };
            screen[coord] = StyledContent::new(ContentStyle::new(), "-".into());
            coord.line += 1;
            screen[coord] = StyledContent::new(ContentStyle::new(), c.into());
        }
        for (i, c) in "|tabs|tabs".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            screen[coord] = StyledContent::new(ContentStyle::new(), c.into());
            coord.line += 1;
            screen[coord] = StyledContent::new(ContentStyle::new(), '-'.into());
        }
        for (i, c) in "Gutter".chars().enumerate() {
            let i = i + 2;
            let mut coord = ScreenCoord {
                line: i as u16,
                column: 0,
            };
            screen[coord] = StyledContent::new(ContentStyle::new(), c.into());
            coord.column += 1;
            screen[coord] = StyledContent::new(ContentStyle::new(), '|'.into());
            // shove 3 spaces for identation.
            for _ in 0..4 {
                coord.column += 1;
                screen[coord] = StyledContent::new(ContentStyle::new(), ' '.into());
            }
            for (j, c) in "Code..".chars().enumerate() {
                let coord = ScreenCoord {
                    line: i as u16,
                    column: (2 + j + (i % 3)) as u16,
                };
                println!("Showing {c} in {coord:?}");
                screen[coord] = StyledContent::new(ContentStyle::new(), c.into());
            }
        }

        assert_snapshot!(screen.display_as_text(), @r"
        |tabs|tabs
        ----------
        G|  Code..
        u|Code..  
        t| Code.. 
        t|  Code..
        e|Code..  
        r| Code.. 
        ----------
        status bar
        ");
    }

    #[test]
    fn basic_sub_screen_print() {
        let mut screen = ScreenBuffer::new(10, 10);
        let mut sub_screen = screen.as_sub_screen();
        let mut status_view = sub_screen.sub_screen(ScreenArea::new(
            ScreenCoord { line: 8, column: 0 },
            ScreenCoord { line: 9, column: 9 },
        ));

        for (i, c) in "status bar".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            status_view[coord] = StyledContent::new(ContentStyle::new(), '-'.into());
            coord.line += 1;
            status_view[coord] = StyledContent::new(ContentStyle::new(), c.into());
        }

        let mut tabs_view = sub_screen.sub_screen(ScreenArea::new(
            ScreenCoord { line: 0, column: 0 },
            ScreenCoord { line: 1, column: 9 },
        ));
        for (i, c) in "|tabs|tabs".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), c.into());
            coord.line += 1;
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), '-'.into());
        }

        let mut code_view = sub_screen.sub_screen(ScreenArea::new(
            ScreenCoord { line: 2, column: 0 },
            ScreenCoord { line: 8, column: 9 },
        ));
        for (i, c) in "Gutter".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: i as u16,
                column: 0,
            };
            code_view[coord] = StyledContent::new(ContentStyle::new(), c.into());
            coord.column += 1;
            code_view[coord] = StyledContent::new(ContentStyle::new(), '|'.into());
            // shove 3 spaces for identation.
            for _ in 0..4 {
                coord.column += 1;
                code_view[coord] = StyledContent::new(ContentStyle::new(), ' '.into());
            }
            for (j, c) in "Code..".chars().enumerate() {
                let coord = ScreenCoord {
                    line: i as u16,
                    column: (2 + j + (i % 3)) as u16,
                };
                code_view[coord] = StyledContent::new(ContentStyle::new(), c.into());
            }
        }

        assert_snapshot!(screen.display_as_text(), @r"
        |tabs|tabs
        ----------
        G|Code..  
        u| Code.. 
        t|  Code..
        t|Code..  
        e| Code.. 
        r|  Code..
        ----------
        status bar
        ");
    }

    #[test]
    fn basic_sub_screen_split_print() {
        // In this test we're going to use the `split_after_line` and `split_after_col` methods.
        // We want to make sure that all the area we create have the right width and height and
        // that the final print did actually put the right characters in the right places.
        let mut screen = ScreenBuffer::new(10, 10);
        let mut sub_screen = screen.as_sub_screen();
        assert_eq!(sub_screen.width(), 10);
        assert_eq!(sub_screen.height(), 10);
        let (mut sub_screen, mut status_view) = sub_screen.split_after_line(7);
        assert_eq!(status_view.width(), 10);
        assert_eq!(status_view.height(), 2);
        assert_eq!(sub_screen.width(), 10);
        assert_eq!(sub_screen.height(), 8);

        for (i, c) in "status bar".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            status_view[coord] = StyledContent::new(ContentStyle::new(), '-'.into());
            coord.line += 1;
            status_view[coord] = StyledContent::new(ContentStyle::new(), c.into());
        }

        let (mut tabs_view, mut sub_screen) = sub_screen.split_after_line(1);
        assert_eq!(status_view.width(), 10);
        assert_eq!(status_view.height(), 2);
        assert_eq!(sub_screen.width(), 10);
        assert_eq!(sub_screen.height(), 6);

        for (i, c) in "|tabs|tabs".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), c.into());
            coord.line += 1;
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), '-'.into());
        }

        let (mut gutter_view, mut code_view) = sub_screen.split_after_col(1);
        assert_eq!(gutter_view.width(), 2);
        assert_eq!(gutter_view.height(), 6);
        assert_eq!(code_view.width(), 8);
        assert_eq!(code_view.height(), 6);

        for (i, c) in "Gutter".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: i as u16,
                column: 0,
            };
            gutter_view[coord] = StyledContent::new(ContentStyle::new(), c.into());
            coord.column += 1;
            gutter_view[coord] = StyledContent::new(ContentStyle::new(), '|'.into());

            // shove 3 spaces for identation.
            coord.column = 0;
            for _ in 0..4 {
                coord.column += 1;
                code_view[coord] = StyledContent::new(ContentStyle::new(), ' '.into());
            }
            for (j, c) in "Code..".chars().enumerate() {
                let coord = ScreenCoord {
                    line: i as u16,
                    column: (j + (i % 3)) as u16,
                };
                code_view[coord] = StyledContent::new(ContentStyle::new(), c.into());
            }
        }

        assert_snapshot!(screen.display_as_text(), @r"
        |tabs|tabs
        ----------
        G|Code..  
        u| Code.. 
        t|  Code..
        t|Code..  
        e| Code.. 
        r|  Code..
        ----------
        status bar
        ");
    }

    #[test]
    #[should_panic]
    fn out_of_bound_on_col() {
        let mut screen = ScreenBuffer::new(10, 10);
        let coord = ScreenCoord {
            line: 0,
            column: 10,
        };
        screen[coord] = StyledContent::new(ContentStyle::new(), '|'.into());
    }

    #[test]
    #[should_panic]
    fn out_of_bound_on_line() {
        let mut screen = ScreenBuffer::new(10, 10);
        let coord = ScreenCoord {
            line: 10,
            column: 0,
        };
        screen[coord] = StyledContent::new(ContentStyle::new(), '|'.into());
    }

    #[test]
    #[should_panic]
    fn out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let coord = ScreenCoord {
            line: 10,
            column: 10,
        };
        screen[coord] = StyledContent::new(ContentStyle::new(), '|'.into());
    }

    #[test]
    #[should_panic]
    fn big_out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let coord = ScreenCoord {
            line: 10000,
            column: 10000,
        };
        screen[coord] = StyledContent::new(ContentStyle::new(), '|'.into());
    }

    #[test]
    #[should_panic]
    fn sub_screen_out_of_bound_on_col() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: 0,
                column: 11,
            },
        );
        let _sub = screen.as_sub_screen().sub_screen(area);
    }

    #[test]
    #[should_panic]
    fn sub_screen_out_of_bound_on_line() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: 11,
                column: 0,
            },
        );
        let _sub = screen.as_sub_screen().sub_screen(area);
    }

    #[test]
    #[should_panic]
    fn sub_screen_out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: 11,
                column: 11,
            },
        );
        let _sub = screen.as_sub_screen().sub_screen(area);
    }

    #[test]
    #[should_panic]
    fn big_sub_screen_out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea::new(
            ScreenCoord::zero(),
            ScreenCoord {
                line: 10000,
                column: 10000,
            },
        );
        let _sub = screen.as_sub_screen().sub_screen(area);
    }

    #[test]
    #[should_panic]
    fn sub_screen_inverted_column() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea::new(
            ScreenCoord { line: 0, column: 2 },
            ScreenCoord { line: 9, column: 1 },
        );
        let _sub = screen.as_sub_screen().sub_screen(area);
    }

    #[test]
    #[should_panic]
    fn sub_screen_inverted_line() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea::new(
            ScreenCoord { line: 2, column: 0 },
            ScreenCoord { line: 1, column: 9 },
        );
        let _sub = screen.as_sub_screen().sub_screen(area);
    }

    #[test]
    #[should_panic]
    fn split_col_cant_create_empty_screen_max() {
        let mut screen = ScreenBuffer::new(10, 10);
        let _sub = screen.as_sub_screen().split_after_col(10);
    }

    #[test]
    #[should_panic]
    fn split_line_cant_create_empty_screen_max() {
        let mut screen = ScreenBuffer::new(10, 10);
        let _sub = screen.as_sub_screen().split_after_line(10);
    }
}
