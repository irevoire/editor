use std::{
    io::{self, Write},
    ops::{self, Deref},
};

use crossterm::{
    cursor::{MoveRight, MoveTo},
    style::{ContentStyle, PrintStyledContent, StyledContent},
    terminal::{Clear, ClearType},
    ExecutableCommand, QueueableCommand,
};

pub struct ScreenBuffer {
    height: usize,
    width: usize,
    buffer: Vec<StyledContent<char>>,
}

impl ScreenBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        let c = StyledContent::new(ContentStyle::new(), ' ');
        Self {
            width,
            height,
            buffer: vec![c; width * height],
        }
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn display_as_text(&self) -> String {
        let mut output = String::new();
        for (idx, c) in self.buffer.iter().enumerate() {
            if idx != 0 && idx % self.width() == 0 {
                output.push('\n');
            }
            output.push(*c.content());
        }
        output
    }

    pub fn display_on_screen(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        stdout.queue(MoveTo(0, 0))?;
        let mut line = 0;
        for (idx, content) in self.buffer.iter().enumerate() {
            if idx != 0 && idx % self.width() == 0 {
                line += 1;
                stdout.queue(MoveTo(0, line))?;
            }

            stdout.queue(PrintStyledContent(content.clone()))?;
        }
        stdout.flush()
    }

    pub fn as_full_sub_screen_buffer<'a>(&'a mut self) -> SubScreenBuffer<'a> {
        SubScreenBuffer {
            bottom_right: (self.height(), self.width()),
            screen_buffer: self,
            top_left: (0, 0),
        }
    }

    pub fn sub_screen_buffer<'a>(
        &'a mut self,
        top_left: (usize, usize),
        bottom_right: (usize, usize),
    ) -> SubScreenBuffer<'a> {
        assert!(top_left.0 < bottom_right.0 && top_left.1 < bottom_right.1);
        assert!(bottom_right.0 < self.height && bottom_right.1 < self.width);

        SubScreenBuffer {
            screen_buffer: self,
            top_left,
            bottom_right,
        }
    }
}

impl<T> ops::Index<(T, T)> for ScreenBuffer
where
    T: Into<usize>,
{
    type Output = StyledContent<char>;

    #[track_caller]
    fn index(&self, (line, column): (T, T)) -> &Self::Output {
        let (line, column) = (line.into(), column.into());
        if line >= self.height || column >= self.width {
            panic!("Overflow: Tried to retrieve the character ({line},{column}) in a buffer of dimensions: ({}, {})", self.height, self.width);
        }
        &self.buffer[line * self.width + column]
    }
}

impl<T> ops::IndexMut<(T, T)> for ScreenBuffer
where
    T: Into<usize>,
{
    #[track_caller]
    fn index_mut(&mut self, (line, column): (T, T)) -> &mut Self::Output {
        let (line, column) = (line.into(), column.into());
        if line >= self.height || column >= self.width {
            panic!("Overflow: Tried to retrieve the character ({line},{column}) in a buffer of dimensions: ({}, {})", self.height, self.width);
        }
        &mut self.buffer[line * self.width + column]
    }
}

pub struct SubScreenBuffer<'a> {
    screen_buffer: &'a mut ScreenBuffer,
    top_left: (usize, usize),
    bottom_right: (usize, usize),
}

impl<'a> SubScreenBuffer<'a> {
    pub fn height(&self) -> usize {
        self.bottom_right.0 - self.top_left.0
    }

    pub fn width(&self) -> usize {
        self.bottom_right.1 - self.top_left.1
    }
}

impl<'a, T> ops::Index<(T, T)> for SubScreenBuffer<'a>
where
    T: Into<usize>,
{
    type Output = StyledContent<char>;

    #[track_caller]
    fn index(&self, (line, column): (T, T)) -> &Self::Output {
        let (line, column) = (line.into(), column.into());
        if line > self.bottom_right.0 || column > self.bottom_right.1 {
            panic!("Overflow: Tried to retrieve the character ({line},{column}) in a sub buffer of dimensions: ({}, {})", self.bottom_right.0, self.bottom_right.1);
        }
        let (line, column) = (line + self.top_left.0, column + self.top_left.1);
        &self.screen_buffer[(line, column)]
    }
}

impl<'a, T> ops::IndexMut<(T, T)> for SubScreenBuffer<'a>
where
    T: Into<usize>,
{
    #[track_caller]
    fn index_mut(&mut self, (line, column): (T, T)) -> &mut Self::Output {
        let (line, column) = (line.into(), column.into());
        if line > self.bottom_right.0 || column > self.bottom_right.1 {
            panic!("Overflow: Tried to retrieve the character ({line},{column}) in a sub buffer of dimensions: ({}, {})", self.bottom_right.0, self.bottom_right.1);
        }
        let (line, column) = (line + self.top_left.0, column + self.top_left.1);
        &mut self.screen_buffer[(line, column)]
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
            screen[(8, i)] = StyledContent::new(ContentStyle::new(), '-');
            screen[(9, i)] = StyledContent::new(ContentStyle::new(), c);
        }
        for (i, c) in "|tabs|tabs".chars().enumerate() {
            screen[(0, i)] = StyledContent::new(ContentStyle::new(), c);
            screen[(1, i)] = StyledContent::new(ContentStyle::new(), '-');
        }
        for (i, c) in "Gutter".chars().enumerate() {
            let i = i + 2;
            screen[(i, 0)] = StyledContent::new(ContentStyle::new(), c);
            screen[(i, 1)] = StyledContent::new(ContentStyle::new(), '|');
            for (j, c) in "Code..".chars().enumerate() {
                screen[(i, 2 + j + (i % 3))] = StyledContent::new(ContentStyle::new(), c);
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
        let mut status_view = screen.sub_screen_buffer((8, 0), (9, 9));

        for (i, c) in "status bar".chars().enumerate() {
            status_view[(0, i)] = StyledContent::new(ContentStyle::new(), '-');
            status_view[(1, i)] = StyledContent::new(ContentStyle::new(), c);
        }

        let mut tabs_view = screen.sub_screen_buffer((0, 0), (1, 9));
        for (i, c) in "|tabs|tabs".chars().enumerate() {
            tabs_view[(0, i)] = StyledContent::new(ContentStyle::new(), c);
            tabs_view[(1, i)] = StyledContent::new(ContentStyle::new(), '-');
        }
        let mut code_view = screen.sub_screen_buffer((2, 0), (8, 9));
        for (i, c) in "Gutter".chars().enumerate() {
            code_view[(i, 0)] = StyledContent::new(ContentStyle::new(), c);
            code_view[(i, 1)] = StyledContent::new(ContentStyle::new(), '|');
            for (j, c) in "Code..".chars().enumerate() {
                code_view[(i, 2 + j + (i % 3))] = StyledContent::new(ContentStyle::new(), c);
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
        screen[(0_usize, 10_usize)] = StyledContent::new(ContentStyle::new(), '|');
    }

    #[test]
    #[should_panic]
    fn out_of_bound_on_line() {
        let mut screen = ScreenBuffer::new(10, 10);
        screen[(10_usize, 0_usize)] = StyledContent::new(ContentStyle::new(), '|');
    }

    #[test]
    #[should_panic]
    fn out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        screen[(10_usize, 10_usize)] = StyledContent::new(ContentStyle::new(), '|');
    }

    #[test]
    #[should_panic]
    fn big_out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        screen[(10000_usize, 1000000_usize)] = StyledContent::new(ContentStyle::new(), '|');
    }

    #[test]
    #[should_panic]
    fn sub_screen_out_of_bound_on_col() {
        let mut screen = ScreenBuffer::new(10, 10);
        let _sub = screen.sub_screen_buffer((0, 0), (0, 10));
    }

    #[test]
    #[should_panic]
    fn sub_screen_out_of_bound_on_line() {
        let mut screen = ScreenBuffer::new(10, 10);
        let _sub = screen.sub_screen_buffer((0, 0), (10, 0));
    }

    #[test]
    #[should_panic]
    fn sub_screen_out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let _sub = screen.sub_screen_buffer((0, 0), (10, 10));
    }

    #[test]
    #[should_panic]
    fn big_sub_screen_out_of_bound_on_both() {
        let mut screen = ScreenBuffer::new(10, 10);
        let _sub = screen.sub_screen_buffer((0, 0), (10000, 1000000));
    }

    #[test]
    #[should_panic]
    fn sub_screen_inverted_column() {
        let mut screen = ScreenBuffer::new(10, 10);
        let _sub = screen.sub_screen_buffer((0, 2), (9, 1));
    }

    #[test]
    #[should_panic]
    fn sub_screen_inverted_line() {
        let mut screen = ScreenBuffer::new(10, 10);
        let _sub = screen.sub_screen_buffer((2, 0), (1, 9));
    }
}
