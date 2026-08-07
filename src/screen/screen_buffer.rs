use std::{
    io::{self, Write},
    ops,
};

use crossterm::{
    cursor::MoveTo,
    style::{ContentStyle, PrintStyledContent, StyledContent},
    QueueableCommand,
};

use crate::screen::{ScreenArea, ScreenCoord};

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
                ScreenCoord {
                    line: lines,
                    column: columns,
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
            output.push_str(c.content());
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

    pub fn as_full_sub_screen_buffer<'a>(&'a mut self) -> SubScreenBuffer<'a> {
        SubScreenBuffer {
            area: ScreenArea {
                top_left: ScreenCoord::zero(),
                bottom_right: ScreenCoord {
                    line: self.height(),
                    column: self.width(),
                },
            },
            screen_buffer: self,
        }
    }

    pub fn sub_screen_buffer<'a>(&'a mut self, screen_area: ScreenArea) -> SubScreenBuffer<'a> {
        assert!(self.area.contains(screen_area));

        SubScreenBuffer {
            screen_buffer: self,
            area: screen_area,
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

pub struct SubScreenBuffer<'a> {
    screen_buffer: &'a mut ScreenBuffer,
    area: ScreenArea,
}

impl<'a> SubScreenBuffer<'a> {
    pub fn height(&self) -> u16 {
        self.area.height()
    }

    pub fn width(&self) -> u16 {
        self.area.width()
    }

    pub fn set_cursor(&mut self, cursor: ScreenCoord) {
        self.screen_buffer.cursor = ScreenCoord {
            line: cursor.line - self.area.top_left.line,
            column: cursor.column - self.area.top_left.column,
        };
    }

    pub fn sub_screen_buffer(&mut self, area: ScreenArea) -> SubScreenBuffer<'_> {
        assert!(self.area.contains(area));

        let area = ScreenArea::new(
            ScreenCoord {
                line: self.area.top_left.line + area.top_left.line,
                column: self.area.top_left.column + area.top_left.column,
            },
            ScreenCoord {
                line: self.area.bottom_right.line + area.bottom_right.line,
                column: self.area.bottom_right.column + area.bottom_right.column,
            },
        );
        SubScreenBuffer {
            screen_buffer: self.screen_buffer,
            area,
        }
    }
}

impl<'a> ops::Index<ScreenCoord> for SubScreenBuffer<'a> {
    type Output = StyledContent<String>;

    #[track_caller]
    fn index(&self, coord: ScreenCoord) -> &Self::Output {
        if coord.line > self.area.bottom_right.line || coord.column > self.area.bottom_right.column
        {
            panic!("Overflow: Tried to retrieve the character {coord:?} in a sub buffer of dimensions: ({}, {})", self.area.bottom_right.line, self.area.bottom_right.column);
        }
        let coord = ScreenCoord {
            line: coord.line + self.area.top_left.line,
            column: coord.column + self.area.top_left.column,
        };

        &self.screen_buffer[coord]
    }
}

impl<'a> ops::IndexMut<ScreenCoord> for SubScreenBuffer<'a> {
    #[track_caller]
    fn index_mut(&mut self, coord: ScreenCoord) -> &mut Self::Output {
        if coord.line > self.area.bottom_right.line || coord.column > self.area.bottom_right.column
        {
            panic!("Overflow: Tried to retrieve the character {coord:?} in a sub buffer of dimensions: ({}, {})", self.area.bottom_right.line, self.area.bottom_right.column);
        }
        let coord = ScreenCoord {
            line: coord.line + self.area.top_left.line,
            column: coord.column + self.area.top_left.column,
        };

        &mut self.screen_buffer[coord]
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
    fn basic_sub_view_print() {
        let mut screen = ScreenBuffer::new(10, 10);
        let mut status_view = screen.sub_screen_buffer(ScreenArea {
            top_left: ScreenCoord { line: 8, column: 0 },
            bottom_right: ScreenCoord { line: 9, column: 9 },
        });

        for (i, c) in "status bar".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            status_view[coord] = StyledContent::new(ContentStyle::new(), '-'.to_string());
            coord.line += 1;
            status_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
        }

        let mut tabs_view = screen.sub_screen_buffer(ScreenArea {
            top_left: ScreenCoord { line: 0, column: 0 },
            bottom_right: ScreenCoord { line: 1, column: 9 },
        });
        for (i, c) in "|tabs|tabs".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
            coord.line += 1;
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), '-'.to_string());
        }

        let mut code_view = screen.sub_screen_buffer(ScreenArea {
            top_left: ScreenCoord { line: 2, column: 0 },
            bottom_right: ScreenCoord { line: 8, column: 9 },
        });
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
    fn basic_sub_sub_view_print() {
        let mut screen = ScreenBuffer::new(10, 10);
        let mut sub_screen = screen.as_full_sub_screen_buffer();
        let mut status_view = sub_screen.sub_screen_buffer(ScreenArea {
            top_left: ScreenCoord { line: 8, column: 0 },
            bottom_right: ScreenCoord { line: 9, column: 9 },
        });

        for (i, c) in "status bar".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            status_view[coord] = StyledContent::new(ContentStyle::new(), '-'.to_string());
            coord.line += 1;
            status_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
        }

        let mut tabs_view = sub_screen.sub_screen_buffer(ScreenArea {
            top_left: ScreenCoord { line: 0, column: 0 },
            bottom_right: ScreenCoord { line: 1, column: 9 },
        });
        for (i, c) in "|tabs|tabs".chars().enumerate() {
            let mut coord = ScreenCoord {
                line: 0,
                column: i as u16,
            };
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
            coord.line += 1;
            tabs_view[coord] = StyledContent::new(ContentStyle::new(), '-'.to_string());
        }

        let mut code_view = sub_screen.sub_screen_buffer(ScreenArea {
            top_left: ScreenCoord { line: 2, column: 0 },
            bottom_right: ScreenCoord { line: 8, column: 9 },
        });
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
        let area = ScreenArea {
            top_left: ScreenCoord::zero(),
            bottom_right: ScreenCoord {
                line: 0,
                column: 11,
            },
        };
        let _sub = screen.sub_screen_buffer(area);
    }

    #[test]
    #[should_panic]
    fn sub_screen_out_of_bound_on_line() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea {
            top_left: ScreenCoord::zero(),
            bottom_right: ScreenCoord {
                line: 11,
                column: 0,
            },
        };
        let _sub = screen.sub_screen_buffer(area);
    }

    #[test]
    #[should_panic]
    fn sub_screen_out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea {
            top_left: ScreenCoord::zero(),
            bottom_right: ScreenCoord {
                line: 11,
                column: 11,
            },
        };
        let _sub = screen.sub_screen_buffer(area);
    }

    #[test]
    #[should_panic]
    fn big_sub_screen_out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea {
            top_left: ScreenCoord::zero(),
            bottom_right: ScreenCoord {
                line: 10000,
                column: 10000,
            },
        };
        let _sub = screen.sub_screen_buffer(area);
    }

    #[test]
    #[should_panic]
    fn sub_screen_inverted_column() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea::new(
            ScreenCoord { line: 0, column: 2 },
            ScreenCoord { line: 9, column: 1 },
        );
        let _sub = screen.sub_screen_buffer(area);
    }

    #[test]
    #[should_panic]
    fn sub_screen_inverted_line() {
        let mut screen = ScreenBuffer::new(10, 10);
        let area = ScreenArea::new(
            ScreenCoord { line: 2, column: 0 },
            ScreenCoord { line: 1, column: 9 },
        );
        let _sub = screen.sub_screen_buffer(area);
    }
}
