use std::sync::Arc;

use crossterm::style::{ContentStyle, StyledContent};

use crate::{
    action::DeleteDirection, screen::screen_buffer::SubScreenBuffer, server::Buffer, ActionResult,
    Selection,
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

    pub fn redraw(&self, buffer: &mut SubScreenBuffer) {
        let rope = self.buffer.rope.blocking_read();
        // The number of chars needed for the raw number + 2 for the `| `
        let gutter_width = ((self.top_line + buffer.height()) as f32).log10().ceil() as usize + 2;
        dbg!(gutter_width);
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
                buffer[(line_idx, i)] = StyledContent::new(ContentStyle::new(), c);
            }
            for (i, c) in line
                .chars()
                .enumerate()
                .take(buffer.width().saturating_sub(gutter_width as usize))
            {
                let i = i + gutter_width as usize;
                // If we find a \n we clear everything till the end
                // of the line and skip to the next one
                if c == '\n' {
                    for i in i..buffer.width() {
                        buffer[(line_idx, i)] = StyledContent::new(ContentStyle::new(), ' ');
                    }
                    break;
                }

                buffer[(line_idx, i)] = StyledContent::new(ContentStyle::new(), c);
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
        view.redraw(&mut buffer.as_full_sub_screen_buffer());
        dbg!(&view.buffer.rope);
        assert_snapshot!(buffer.display_as_text(), @r"
        248| Of course, in the beginning, this cannot be effected except by means of despotic inroads on the rights of
                                                                                                                      
        250| These measures will, of course, be different in different count                                          
                                                                                                                      
        252| Nevertheless, in most advanced countries, the following will be pretty generally applic                  
                                                                                                                      
        254| 1. Abolition of property in land and application of all rents of land to public purp                     
        255| 2. A heavy progressive or graduated income                                                               
        256| 3. Abolition of all rights of inherit                                                                    
        257| 4. Confiscation of the property of all emigrants and re
        ");
    }
}
