use std::sync::Arc;

use crossterm::style::{ContentStyle, StyledContent, Stylize};

#[cfg(test)]
use crate::screen::screen_buffer::ScreenBuffer;
use crate::{
    action::{Anchor, DeleteDirection, Direction},
    screen::{screen_buffer::SubScreen, view::RopeGraphemes, ScreenCoord},
    server::Buffer,
    ActionResult, Cursor, Selection, SelectionMode,
};

pub struct BufferView {
    pub width: usize,
    pub height: usize,
    pub top_line: usize,
    pub active: bool,
    pub selection: Selection,
    pub buffer: Arc<Buffer>,
}

impl BufferView {
    pub fn move_anchor(
        &mut self,
        anchor: Anchor,
        direction: Direction,
        mode: SelectionMode,
    ) -> ActionResult {
        let mut main_anchor = match anchor {
            Anchor::Tail => self.selection.tail,
            Anchor::Head => self.selection.head,
        };
        let rope = self.buffer.rope.blocking_read();
        match direction {
            Direction::Up => {
                if main_anchor.line == 0 {
                    return ActionResult::Nothing;
                } else if main_anchor.line == self.top_line {
                    self.top_line -= 1;
                } else {
                    main_anchor.line -= 1;
                }
            }
            Direction::Down => {
                if main_anchor.line == rope.len_lines() {
                    return ActionResult::Nothing;
                }
                // we've reached the bottom of the screen. We move all the text but not the anchor
                if main_anchor.line == self.top_line + self.height {
                    self.top_line += 1;
                } else {
                    main_anchor.line += 1;
                }
            }
            Direction::Right => {
                main_anchor.column = main_anchor
                    .column
                    .saturating_add(1)
                    .min(rope.line(main_anchor.line).len_chars())
            }
            Direction::Left => main_anchor.column = main_anchor.column.saturating_sub(1),
            Direction::StartOfLine => main_anchor.column = 0,
            Direction::EndOfLine => main_anchor.column = rope.line(main_anchor.line).len_chars(),
            Direction::StartOfFile => {
                self.top_line = 0;
                main_anchor.line = 0;
                main_anchor.column = 0;
            }
            Direction::EndOfFile => todo!(),
            Direction::PageUp => todo!(),
            Direction::PageDown => todo!(),
        }
        if mode == SelectionMode::Char {
            self.selection.head = main_anchor;
            self.selection.tail = main_anchor;
        }
        ActionResult::Redraw
    }

    pub fn insert(&mut self, c: char) -> ActionResult {
        let mut rope = self.buffer.rope.blocking_write();
        let offset = rope.line_to_char(self.selection.head.line);
        let insert_at_char = offset + self.selection.head.column;
        rope.insert_char(insert_at_char, c);
        self.selection.head.column += 1;
        ActionResult::Redraw
    }

    pub fn delete(&mut self, delete_direction: DeleteDirection) -> ActionResult {
        let mut rope = self.buffer.rope.blocking_write();
        match delete_direction {
            DeleteDirection::Left
                if self.selection.head.column == 0 && self.selection.head.line == 0 =>
            {
                return ActionResult::Nothing
            }
            DeleteDirection::Left => {
                self.selection.head.column = self.selection.head.column.saturating_sub(1);
            }
            DeleteDirection::Right => todo!(),
        };
        let offset = rope.line_to_char(self.selection.head.line);
        let remove_char = offset + self.selection.head.column;
        rope.remove(remove_char..=remove_char);
        ActionResult::Redraw
    }

    /// This is mostly used for testing purposes as it draw the cursor as an unicode character
    /// instead of drawing an actual cursor on the screen.
    /// See `set_cursor` instead.
    #[cfg(test)]
    pub fn draw_selection(&self, buffer: &mut SubScreen) {
        use crate::{screen::ScreenCoord, Cursor};

        const BOX_MODIFIER: char = '\u{20DE}';
        const UNDERLINE_MODIFIER: char = '\u{0332}';
        const DOUBLE_UNDERLINE_MODIFIER: char = '\u{0333}';

        let gutter_width = ((self.top_line + buffer.height() as usize) as f32)
            .log10()
            .ceil() as usize
            + 2;

        let mut update_with = |coord: ScreenCoord, modifier: char| {
            buffer[coord] = StyledContent::new(
                *buffer[coord].style(),
                format!("{}{}", buffer[coord].content(), modifier),
            )
        };

        let head_cursor = Cursor {
            line: self.selection.head.line,
            column: self.selection.head.column + gutter_width,
        };
        update_with(head_cursor.to_screen_coord(self.top_line), BOX_MODIFIER);

        let tail_cursor = Cursor {
            line: self.selection.tail.line,
            column: self.selection.tail.column + gutter_width,
        };
        if head_cursor == tail_cursor {
            return;
        }
        update_with(
            tail_cursor.to_screen_coord(self.top_line),
            DOUBLE_UNDERLINE_MODIFIER,
        );

        let (start, end) = if head_cursor < tail_cursor {
            (head_cursor, tail_cursor)
        } else {
            (tail_cursor, head_cursor)
        };
        if start.line == end.line {
            for col in start.column..end.column {
                let col = col + gutter_width;
                update_with(
                    Cursor {
                        line: start.line,
                        column: col,
                    }
                    .to_screen_coord(self.top_line),
                    UNDERLINE_MODIFIER,
                );
            }
        } else {
            todo!()
        }
    }

    #[cfg(test)]
    pub fn draw_and_display(&self, buffer: &mut ScreenBuffer) -> String {
        self.draw_code(&mut buffer.as_sub_screen());
        self.draw_selection(&mut buffer.as_sub_screen());
        buffer.display_as_text()
    }

    pub fn draw_code(&self, buffer: &mut SubScreen) {
        let rope = self.buffer.rope.blocking_read();

        // The number of chars needed for the raw number + 2 for the `| `
        let gutter_width = ((self.top_line + buffer.height() as usize) as f32)
            .log10()
            .ceil() as usize
            + 2;
        if self.active {
            let screen_cursor = Cursor {
                line: self.selection.head.line,
                column: self.selection.head.column + gutter_width,
            };
            // SAFE: Because we know there can only be one active screen at once
            unsafe {
                buffer.set_cursor(screen_cursor.to_screen_coord(self.top_line));
            }
        }

        for (line_idx, line) in rope
            .lines_at(self.top_line)
            .enumerate()
            .take(buffer.height() as usize)
        {
            let gutter = format!(
                "{:width$}| ",
                self.top_line + line_idx,
                width = (gutter_width - 2) as usize
            );
            for (i, c) in gutter.chars().enumerate().take(buffer.width() as usize) {
                let coord = ScreenCoord {
                    line: line_idx as u16,
                    column: i as u16,
                };
                buffer[coord] = StyledContent::new(ContentStyle::new(), c.to_string());
            }
            for (i, g) in RopeGraphemes::new(&line)
                .enumerate()
                .take(buffer.width().saturating_sub(gutter_width as u16) as usize)
            {
                let i = i + gutter_width as usize;
                let g = g.to_string();

                // If we find a \n we clear everything till the end
                // of the line and skip to the next one
                if g.chars().next() == Some('\n') {
                    for i in i as u16..buffer.width() {
                        let coord = ScreenCoord {
                            line: line_idx as u16,
                            column: i as u16,
                        };
                        buffer[coord] = StyledContent::new(ContentStyle::new(), ' '.to_string());
                    }
                    break;
                }

                let coord = ScreenCoord {
                    line: line_idx as u16,
                    column: i as u16,
                };

                buffer[coord] = StyledContent::new(ContentStyle::new(), g.to_string());
            }
        }
    }

    pub fn draw_tab(&self, tab_view: &mut SubScreen<'_>) {
        tab_view.fill(StyledContent::new(
            ContentStyle::new().on_white(),
            " ".to_string(),
        ));
        for (idx, c) in "*scratch*"
            .chars()
            .take(tab_view.width() as usize)
            .enumerate()
        {
            tab_view[ScreenCoord {
                line: 0,
                column: idx as u16,
            }] = StyledContent::new(ContentStyle::new().white().on_dark_grey(), c.to_string());
        }
    }
}

#[cfg(test)]
pub mod test {
    use insta::assert_snapshot;
    use ropey::Rope;
    use tokio::sync::RwLock;

    use crate::{screen::screen_buffer::ScreenBuffer, Cursor};

    use super::*;

    pub fn setup_buffer_view() -> (ScreenBuffer, BufferView) {
        let width = 110;
        let height = 10;
        let view = BufferView {
            width,
            height,
            // at this specific line there is a lot of lines fitting on 120 characters
            top_line: 248,
            selection: Selection {
                tail: Cursor {
                    line: 250,
                    column: 0,
                },
                head: Cursor {
                    line: 250,
                    column: 0,
                },
            },
            buffer: Arc::new(Buffer {
                name: String::from("*scratch*"),
                path: None,
                rope: RwLock::new(Rope::from_str(std::include_str!("test_document.txt"))),
            }),
            active: true,
        };
        let buffer = ScreenBuffer::new(height as u16, width as u16);
        (buffer, view)
    }

    #[test]
    fn basic_display() {
        let (mut buffer, view) = setup_buffer_view();

        assert_snapshot!(view.draw_and_display(&mut buffer), @r"
        248| Of course, in the beginning, this cannot be effected except by means of despotic inroads on the rights of
        249|                                                                                                          
        250| T⃞hese measures will, of course, be different in different countries.                                     
        251|                                                                                                          
        252| Nevertheless, in most advanced countries, the following will be pretty generally applicable.             
        253|                                                                                                          
        254| 1. Abolition of property in land and application of all rents of land to public purposes.                
        255| 2. A heavy progressive or graduated income tax.                                                          
        256| 3. Abolition of all rights of inheritance.                                                               
        257| 4. Confiscation of the property of all emigrants and rebels.
        ");
    }

    #[test]
    fn move_anchor_basic() {
        let (mut buffer, mut view) = setup_buffer_view();
        assert_snapshot!(view.draw_and_display(&mut buffer), @r"
        248| Of course, in the beginning, this cannot be effected except by means of despotic inroads on the rights of
        249|                                                                                                          
        250| T⃞hese measures will, of course, be different in different countries.                                     
        251|                                                                                                          
        252| Nevertheless, in most advanced countries, the following will be pretty generally applicable.             
        253|                                                                                                          
        254| 1. Abolition of property in land and application of all rents of land to public purposes.                
        255| 2. A heavy progressive or graduated income tax.                                                          
        256| 3. Abolition of all rights of inheritance.                                                               
        257| 4. Confiscation of the property of all emigrants and rebels.
        ");

        view.move_anchor(Anchor::Head, Direction::Right, SelectionMode::Char);
        assert_snapshot!(view.draw_and_display(&mut buffer), @r"
        248| Of course, in the beginning, this cannot be effected except by means of despotic inroads on the rights of
        249|                                                                                                          
        250| Th⃞ese measures will, of course, be different in different countries.                                     
        251|                                                                                                          
        252| Nevertheless, in most advanced countries, the following will be pretty generally applicable.             
        253|                                                                                                          
        254| 1. Abolition of property in land and application of all rents of land to public purposes.                
        255| 2. A heavy progressive or graduated income tax.                                                          
        256| 3. Abolition of all rights of inheritance.                                                               
        257| 4. Confiscation of the property of all emigrants and rebels.
        ");

        view.move_anchor(Anchor::Head, Direction::Up, SelectionMode::Char);
        assert_snapshot!(view.draw_and_display(&mut buffer), @r"
        248| Of course, in the beginning, this cannot be effected except by means of despotic inroads on the rights of
        249|   ⃞                                                                                                       
        250| These measures will, of course, be different in different countries.                                     
        251|                                                                                                          
        252| Nevertheless, in most advanced countries, the following will be pretty generally applicable.             
        253|                                                                                                          
        254| 1. Abolition of property in land and application of all rents of land to public purposes.                
        255| 2. A heavy progressive or graduated income tax.                                                          
        256| 3. Abolition of all rights of inheritance.                                                               
        257| 4. Confiscation of the property of all emigrants and rebels.
        ");

        view.move_anchor(Anchor::Head, Direction::Down, SelectionMode::Char);
        assert_snapshot!(view.draw_and_display(&mut buffer), @r"
        248| Of course, in the beginning, this cannot be effected except by means of despotic inroads on the rights of
        249|                                                                                                          
        250| Th⃞ese measures will, of course, be different in different countries.                                     
        251|                                                                                                          
        252| Nevertheless, in most advanced countries, the following will be pretty generally applicable.             
        253|                                                                                                          
        254| 1. Abolition of property in land and application of all rents of land to public purposes.                
        255| 2. A heavy progressive or graduated income tax.                                                          
        256| 3. Abolition of all rights of inheritance.                                                               
        257| 4. Confiscation of the property of all emigrants and rebels.
        ");
    }
}
