use std::{
    io::{self, Write},
    marker::PhantomData,
    ops,
};

use crossterm::{
    cursor::MoveTo,
    style::{ContentStyle, PrintStyledContent, StyledContent},
    QueueableCommand,
};

use crate::screen::{ScreenArea, ScreenCoord};

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
    buffer: Vec<StyledContent<String>>,
}

impl ScreenBuffer {
    pub fn new(lines: u16, columns: u16) -> Self {
        let c = StyledContent::new(ContentStyle::new(), "".to_string());
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
    type Output = StyledContent<String>;

    #[track_caller]
    fn index(&self, coord: ScreenCoord) -> &Self::Output {
        if coord.line >= self.area.height() || coord.column >= self.area.width() {
            panic!("Overflow: Tried to retrieve the character {coord:?} in a buffer of dimensions: ({}, {})", self.area.height(), self.area.width());
        }
        &self.buffer[(coord.line * self.area.width() + coord.column) as usize]
    }
}

impl ops::IndexMut<ScreenCoord> for ScreenBuffer {
    #[track_caller]
    fn index_mut(&mut self, coord: ScreenCoord) -> &mut Self::Output {
        if coord.line >= self.area.height() || coord.column >= self.area.width() {
            panic!("Overflow: Tried to retrieve the character {coord:?} in a buffer of dimensions: ({}, {})", self.area.height(), self.area.width());
        }
        &mut self.buffer[(coord.line * self.area.width() + coord.column) as usize]
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

    pub fn fill(&mut self, content: StyledContent<String>) {
        for coord in self.area.iter() {
            self[coord] = content.clone();
        }
    }
}

impl<'a> ops::Index<ScreenCoord> for SubScreen<'a> {
    type Output = StyledContent<String>;

    #[track_caller]
    fn index(&self, coord: ScreenCoord) -> &Self::Output {
        if !self.area.contains_internal_coord(coord) {
            panic!("Overflow: Tried to retrieve the character {coord:?} in a sub buffer of dimensions: ({}, {})", self.area.height(), self.area.width());
        }
        let coord = self.area.translate_internal_coord(coord);
        let screen_buffer = unsafe { &*self.screen_buffer };
        &screen_buffer[coord]
    }
}

impl<'a> ops::IndexMut<ScreenCoord> for SubScreen<'a> {
    #[track_caller]
    fn index_mut(&mut self, coord: ScreenCoord) -> &mut Self::Output {
        if !self.area.contains_internal_coord(coord) {
            panic!("Overflow: Tried to retrieve the character {coord:?} in a sub buffer of dimensions: ({}, {})", self.area.height(), self.area.width());
        }
        let coord = self.area.translate_internal_coord(coord);
        let screen_buffer = unsafe { &mut *self.screen_buffer };
        &mut screen_buffer[coord]
    }
}

#[cfg(test)]
mod test {
    use insta::{assert_debug_snapshot, assert_snapshot};

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
            screen[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
        }
        for (i, c) in "|tabs|tabs".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            screen[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
            coord.line += 1;
            screen[coord] = StyledContent::new(ContentStyle::new(), '-'.into());
        }
        for (i, c) in "Gutter".chars().enumerate() {
            let i = i + 2;
            let mut coord = ScreenCoord {
                line: i as u16,
                column: 0,
            };
            screen[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
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
                screen[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
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
            status_view[coord] = StyledContent::new(ContentStyle::new(), '-'.to_string());
            coord.line += 1;
            status_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
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
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
            coord.line += 1;
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), '-'.to_string());
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
            code_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
            coord.column += 1;
            code_view[coord] = StyledContent::new(ContentStyle::new(), '|'.to_string());
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
                code_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
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
            status_view[coord] = StyledContent::new(ContentStyle::new(), '-'.to_string());
            coord.line += 1;
            status_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
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
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
            coord.line += 1;
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), '-'.to_string());
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
            gutter_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
            coord.column += 1;
            gutter_view[coord] = StyledContent::new(ContentStyle::new(), '|'.to_string());

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
                code_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
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
        screen[coord] = StyledContent::new(ContentStyle::new(), '|'.to_string());
    }

    #[test]
    #[should_panic]
    fn out_of_bound_on_line() {
        let mut screen = ScreenBuffer::new(10, 10);
        let coord = ScreenCoord {
            line: 10,
            column: 0,
        };
        screen[coord] = StyledContent::new(ContentStyle::new(), '|'.to_string());
    }

    #[test]
    #[should_panic]
    fn out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let coord = ScreenCoord {
            line: 10,
            column: 10,
        };
        screen[coord] = StyledContent::new(ContentStyle::new(), '|'.to_string());
    }

    #[test]
    #[should_panic]
    fn big_out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let coord = ScreenCoord {
            line: 10000,
            column: 10000,
        };
        screen[coord] = StyledContent::new(ContentStyle::new(), '|'.to_string());
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
