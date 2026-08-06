use std::sync::Arc;

use crossterm::style::{ContentStyle, StyledContent};

use crate::{
    action::DeleteDirection,
    screen::{screen_buffer::SubScreenBuffer, view::RopeGraphemes},
    server::Buffer,
    ActionResult, Cursor, Selection,
};

pub struct BufferView {
    pub width: usize,
    pub height: usize,
    pub top_line: usize,
    pub selection: Selection,
    pub buffer: Arc<Buffer>,
}

impl BufferView {
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
    pub fn draw_selection(&self, buffer: &mut SubScreenBuffer) {
        const BOX_MODIFIER: char = '\u{20DE}';
        const UNDERLINE_MODIFIER: char = '\u{0332}';
        const DOUBLE_UNDERLINE_MODIFIER: char = '\u{0333}';

        let gutter_width = ((self.top_line + buffer.height()) as f32).log10().ceil() as usize + 2;

        let mut update_with = |cursor: Cursor, modifier: char| {
            buffer[cursor] = StyledContent::new(
                *buffer[cursor].style(),
                format!("{}{}", buffer[cursor].content(), modifier),
            )
        };

        let head_cursor = Cursor {
            line: self.selection.head.line - self.top_line,
            column: self.selection.head.column + gutter_width,
        };
        update_with(head_cursor, BOX_MODIFIER);

        let tail_cursor = Cursor {
            line: self.selection.tail.line - self.top_line,
            column: self.selection.tail.column + gutter_width,
        };
        if head_cursor == tail_cursor {
            return;
        }
        update_with(tail_cursor, DOUBLE_UNDERLINE_MODIFIER);

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
                    },
                    UNDERLINE_MODIFIER,
                );
            }
        } else {
            todo!()
        }
    }

    pub fn draw(&self, buffer: &mut SubScreenBuffer) {
        let rope = self.buffer.rope.blocking_read();
        // The number of chars needed for the raw number + 2 for the `| `
        let gutter_width = ((self.top_line + buffer.height()) as f32).log10().ceil() as usize + 2;
        for (line_idx, line) in rope
            .lines_at(self.top_line)
            .enumerate()
            .take(buffer.height())
        {
            let gutter = format!(
                "{:width$}| ",
                self.top_line + line_idx,
                width = (gutter_width - 2) as usize
            );
            for (i, c) in gutter.chars().enumerate().take(buffer.width()) {
                buffer[(line_idx, i)] = StyledContent::new(ContentStyle::new(), c.to_string());
            }
            for (i, g) in RopeGraphemes::new(&line)
                .enumerate()
                .take(buffer.width().saturating_sub(gutter_width as usize))
            {
                let i = i + gutter_width as usize;
                let g = g.to_string();

                // If we find a \n we clear everything till the end
                // of the line and skip to the next one
                if g.chars().next() == Some('\n') {
                    for i in i..buffer.width() {
                        buffer[(line_idx, i)] =
                            StyledContent::new(ContentStyle::new(), ' '.to_string());
                    }
                    break;
                }

                buffer[(line_idx, i)] = StyledContent::new(ContentStyle::new(), g.to_string());
            }
        }
    }
}

#[cfg(test)]
mod test {
    use insta::assert_snapshot;
    use ropey::Rope;
    use tokio::sync::RwLock;

    use crate::{screen::screen_buffer::ScreenBuffer, Cursor};

    use super::*;

    fn setup_buffer_view() -> (ScreenBuffer, BufferView) {
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
        };
        let buffer = ScreenBuffer::new(width, height);
        (buffer, view)
    }

    #[test]
    fn basic_display() {
        let (mut buffer, view) = setup_buffer_view();
        view.draw(&mut buffer.as_full_sub_screen_buffer());
        view.draw_selection(&mut buffer.as_full_sub_screen_buffer());

        assert_snapshot!(buffer.display_as_text(), @r"
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
}
