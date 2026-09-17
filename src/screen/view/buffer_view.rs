use std::sync::Arc;

use crossterm::style::{ContentStyle, StyledContent, Stylize};

#[cfg(test)]
use crate::screen::screen_buffer::ScreenBuffer;
use crate::{
    ActionResult, Cursor, Selection, SelectionMode,
    screen::{
        ScreenCoord,
        screen_buffer::{Grapheme, SubScreen},
        view::{RopeGraphemes, WrapChunks},
    },
    server::Buffer,
};
use action::{Anchor, DeleteDirection, Direction};

pub struct BufferView {
    pub width: usize,
    pub height: usize,
    pub top_line: usize,
    pub active: bool,
    pub selection: Selection,
    pub buffer: Arc<Buffer>,
    pub background: ContentStyle,
    pub soft_wrap: bool,
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
                let scrolls = if self.soft_wrap {
                    self.rows_used_by_wrapping(&rope, main_anchor.line) >= self.height
                } else {
                    main_anchor.line == self.top_line + self.height
                };
                if scrolls {
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

    /// Total screen rows the buffer lines `self.top_line..=through_line`
    /// occupy once wrapped at the view's current `width`.
    fn rows_used_by_wrapping(&self, rope: &ropey::Rope, through_line: usize) -> usize {
        let gutter_width = Self::gutter_width(self.top_line, self.height);
        let content_width = self.width.saturating_sub(gutter_width).max(1);
        rope.lines_at(self.top_line)
            .take(through_line.saturating_sub(self.top_line) + 1)
            // We have to iterate over the whole chunk because we can't
            // compute ahead of time the size the grapheme can take
            .map(|line| WrapChunks::new(&line, content_width).count())
            .sum()
    }

    /// Updates the size this view renders into. Called by the owning
    /// [`crate::screen::Screen`] whenever the area it was given changes
    /// (e.g. on a terminal resize), so that methods like [`Self::move_anchor`]
    /// -- which don't get handed a `SubScreen` -- can still reason about how
    /// much is currently visible.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;

        // A resize can leave `top_line` above or below the new window.
        // Scroll just enough to bring it back in view.
        let head_line = self.selection.head.line;
        if head_line < self.top_line {
            self.top_line = head_line;
        } else if head_line >= self.top_line + self.height {
            self.top_line = head_line + 1 - self.height;
        }
    }

    pub fn insert(&mut self, c: char) -> ActionResult {
        let mut rope = self.buffer.rope.blocking_write();
        let offset = rope.line_to_char(self.selection.head.line);
        let insert_at_char = offset + self.selection.head.column;
        rope.insert_char(insert_at_char, c);
        if c == '\n' {
            self.selection.head.line += 1;
            self.selection.head.column = 0;
        } else {
            self.selection.head.column += 1;
        }
        self.selection.tail = self.selection.head;
        ActionResult::Redraw
    }

    pub fn delete(&mut self, delete_direction: DeleteDirection) -> ActionResult {
        let mut rope = self.buffer.rope.blocking_write();
        match delete_direction {
            DeleteDirection::Left
                if self.selection.head.column == 0 && self.selection.head.line == 0 =>
            {
                return ActionResult::Nothing;
            }
            DeleteDirection::Left if self.selection.head.column == 0 => {
                // At the beginning of a line we find the previous one and remove
                // its \n.
                let prev_line_start = rope.line_to_char(self.selection.head.line - 1);
                let current_line_start = rope.line_to_char(self.selection.head.line);
                let newline_char = current_line_start - 1;
                debug_assert_eq!(rope.get_slice(newline_char..=newline_char).unwrap(), "\n");
                rope.remove(newline_char..=newline_char);

                self.selection.head.line -= 1;
                self.selection.head.column = newline_char - prev_line_start;
            }
            DeleteDirection::Left => {
                self.selection.head.column -= 1;
                let offset = rope.line_to_char(self.selection.head.line);
                let remove_char = offset + self.selection.head.column;
                rope.remove(remove_char..=remove_char);
            }
            DeleteDirection::Right => todo!(),
        };
        self.selection.tail = self.selection.head;
        ActionResult::Redraw
    }

    /// This is mostly used for testing purposes as it draw the cursor as an unicode character
    /// instead of drawing an actual cursor on the screen.
    /// See `set_cursor` instead.
    #[cfg(test)]
    pub fn draw_selection(&self, buffer: &mut SubScreen) {
        use crate::{Cursor, screen::ScreenCoord};

        const BOX_MODIFIER: char = '\u{20DE}';
        const UNDERLINE_MODIFIER: char = '\u{0332}';
        const DOUBLE_UNDERLINE_MODIFIER: char = '\u{0333}';

        let gutter_width = ((self.top_line + buffer.height() as usize) as f32)
            .log10()
            .ceil() as usize
            + 2;

        let mut update_with = |coord: ScreenCoord, modifier: char| {
            use crate::screen::screen_buffer::Grapheme;

            buffer[coord] = StyledContent::new(
                *buffer[coord].style(),
                Grapheme::from(format!("{}{}", buffer[coord].content(), modifier)),
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

    /// The number of columns reserved for the gutter (`"NNN| "`), based on
    /// the widest line number that could possibly be visible.
    fn gutter_width(top_line: usize, height: usize) -> usize {
        ((top_line + height) as f32).log10().ceil() as usize + 2
    }

    pub fn draw_code(&self, buffer: &mut SubScreen) {
        // Fill the whole area first so this view fully owns its own
        // background, even where its content doesn't have enough lines to
        // cover every cell itself (e.g. a short file, or a popup's content).
        buffer.fill(StyledContent::new(self.background, Grapheme::space()));

        let rope = self.buffer.rope.blocking_read();

        let gutter_width = Self::gutter_width(self.top_line, buffer.height() as usize);
        let content_width = (buffer.width() as usize)
            .saturating_sub(gutter_width)
            .max(1);

        if self.active && !self.soft_wrap {
            let clamped_column = self.selection.head.column.min(content_width - 1);
            let column =
                (clamped_column + gutter_width).min(buffer.width().saturating_sub(1) as usize);
            let screen_cursor = Cursor {
                line: self.selection.head.line,
                column,
            };
            // SAFE: Because we know there can only be one active screen at once
            unsafe {
                buffer.set_cursor(screen_cursor.to_screen_coord(self.top_line));
            }
        }

        if !self.soft_wrap {
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
                    buffer[coord] = StyledContent::new(self.background, c.into());
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
                            buffer[coord] = StyledContent::new(self.background, Grapheme::space());
                        }
                        break;
                    }

                    let coord = ScreenCoord {
                        line: line_idx as u16,
                        column: i as u16,
                    };

                    buffer[coord] = StyledContent::new(self.background, g.into());
                }
            }
        } else {
            self.draw_code_wrapped(buffer, &rope, gutter_width, content_width);
        }
    }

    /// Same as the non-wrapped path of [`Self::draw_code`], but a line that
    /// doesn't fit `content_width` continues onto "virtual" continuation
    /// rows below it instead of being truncated, with the gutter blanked
    /// out (save for a `↪` marker) on those continuation rows.
    fn draw_code_wrapped(
        &self,
        buffer: &mut SubScreen,
        rope: &ropey::Rope,
        gutter_width: usize,
        content_width: usize,
    ) {
        let height = buffer.height() as usize;
        let mut screen_row = 0usize;
        let mut cursor_coord: Option<ScreenCoord> = None;
        let mut last_head_line_coord: Option<ScreenCoord> = None;

        'lines: for (line_idx, line) in rope.lines_at(self.top_line).enumerate() {
            if screen_row >= height {
                break;
            }
            let buffer_line_number = self.top_line + line_idx;

            for (seg_idx, segment) in WrapChunks::new(&line, content_width).enumerate() {
                if screen_row >= height {
                    break 'lines;
                }

                let gutter = if seg_idx == 0 {
                    format!("{:width$}| ", buffer_line_number, width = gutter_width - 2)
                } else {
                    format!("{:width$}↪ ", "", width = gutter_width - 2)
                };
                for (i, c) in gutter.chars().enumerate().take(buffer.width() as usize) {
                    let coord = ScreenCoord {
                        line: screen_row as u16,
                        column: i as u16,
                    };
                    buffer[coord] = StyledContent::new(self.background, c.into());
                }

                for (i, g) in segment.iter().enumerate() {
                    let column = gutter_width + i;
                    if column >= buffer.width() as usize {
                        break;
                    }
                    let coord = ScreenCoord {
                        line: screen_row as u16,
                        column: column as u16,
                    };
                    buffer[coord] = StyledContent::new(self.background, g.to_string().into());
                }

                if self.active && buffer_line_number == self.selection.head.line {
                    let seg_end_column = (gutter_width + segment.len())
                        .min((buffer.width() as usize).saturating_sub(1));
                    last_head_line_coord = Some(ScreenCoord {
                        line: screen_row as u16,
                        column: seg_end_column as u16,
                    });

                    let target_seg = self.selection.head.column / content_width;
                    if seg_idx == target_seg {
                        let col_in_seg = self.selection.head.column % content_width;
                        cursor_coord = Some(ScreenCoord {
                            line: screen_row as u16,
                            column: (gutter_width + col_in_seg) as u16,
                        });
                    }
                }

                screen_row += 1;
            }
        }

        if self.active {
            let coord = cursor_coord
                .or(last_head_line_coord)
                .unwrap_or(ScreenCoord {
                    line: 0,
                    column: gutter_width as u16,
                });
            let coord = ScreenCoord {
                line: coord.line.min(buffer.height().saturating_sub(1)),
                column: coord.column.min(buffer.width().saturating_sub(1)),
            };
            // SAFE: Because we know there can only be one active screen at once
            unsafe {
                buffer.set_cursor(coord);
            }
        }
    }

    pub fn draw_tab(&self, tab_view: &mut SubScreen<'_>) {
        tab_view.fill(StyledContent::new(
            ContentStyle::new().on_white(),
            Grapheme::space(),
        ));
        for (idx, c) in "*scratch*"
            .chars()
            .take(tab_view.width() as usize)
            .enumerate()
        {
            tab_view[ScreenCoord {
                line: 0,
                column: idx as u16,
            }] = StyledContent::new(ContentStyle::new().white().on_dark_grey(), c.into());
        }
    }
}

#[cfg(test)]
pub mod test {
    use insta::assert_snapshot;
    use ropey::Rope;
    use tokio::sync::RwLock;

    use crate::{Cursor, screen::screen_buffer::ScreenBuffer};

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
            background: ContentStyle::new(),
            soft_wrap: false,
        };
        let buffer = ScreenBuffer::new(height as u16, width as u16);
        (buffer, view)
    }

    fn setup_wrap_test(
        text: &str,
        width: usize,
        height: usize,
        soft_wrap: bool,
    ) -> (ScreenBuffer, BufferView) {
        let view = BufferView {
            width,
            height,
            top_line: 0,
            selection: Selection::default(),
            buffer: Arc::new(Buffer {
                name: String::from("*scratch*"),
                path: None,
                rope: RwLock::new(Rope::from_str(text)),
            }),
            active: true,
            background: ContentStyle::new(),
            soft_wrap,
        };
        let buffer = ScreenBuffer::new(height as u16, width as u16);
        (buffer, view)
    }

    /// Draws only the code (no cursor overlay), for tests that don't
    /// exercise `draw_selection` (which doesn't account for soft-wrap).
    fn buffer_text(view: &BufferView, buffer: &mut ScreenBuffer) -> String {
        view.draw_code(&mut buffer.as_sub_screen());
        buffer.display_as_text()
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
    fn insert_newline_moves_cursor_to_next_line() {
        let (mut buffer, mut view) = setup_buffer_view();

        assert_eq!(
            view.selection.head,
            Cursor {
                line: 250,
                column: 0
            }
        );

        view.insert('\n');

        assert_eq!(
            view.selection.head,
            Cursor {
                line: 251,
                column: 0
            }
        );
        assert_snapshot!(view.draw_and_display(&mut buffer), @"
        248| Of course, in the beginning, this cannot be effected except by means of despotic inroads on the rights of
        249|                                                                                                          
        250|                                                                                                          
        251| T⃞hese measures will, of course, be different in different countries.                                     
        252|                                                                                                          
        253| Nevertheless, in most advanced countries, the following will be pretty generally applicable.             
        254|                                                                                                          
        255| 1. Abolition of property in land and application of all rents of land to public purposes.                
        256| 2. A heavy progressive or graduated income tax.                                                          
        257| 3. Abolition of all rights of inheritance.
        ");
    }

    #[test]
    fn delete_at_start_of_file_does_nothing() {
        let (_buffer, mut view) = setup_buffer_view();
        view.move_anchor(Anchor::Head, Direction::StartOfFile, SelectionMode::Char);
        assert_eq!(view.selection.head, Cursor { line: 0, column: 0 });

        let content_before = view.buffer.rope.blocking_read().to_string();

        let result = view.delete(DeleteDirection::Left);

        assert!(matches!(result, ActionResult::Nothing));
        assert_eq!(view.selection.head, Cursor { line: 0, column: 0 });
        assert_eq!(view.buffer.rope.blocking_read().to_string(), content_before);
    }

    #[test]
    fn delete_at_start_of_line_joins_with_previous_line() {
        let (mut buffer, mut view) = setup_buffer_view();

        // The cursor starts at the beginning of the (blank) line 250, right
        // after the blank line 249.
        assert_eq!(
            view.selection.head,
            Cursor {
                line: 250,
                column: 0
            }
        );

        assert_snapshot!(view.draw_and_display(&mut buffer), @"
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

        view.delete(DeleteDirection::Left);

        // The blank line 249 had no characters of its own, so joining with
        // it leaves the cursor at its former (empty) start.
        assert_eq!(
            view.selection.head,
            Cursor {
                line: 249,
                column: 0
            }
        );
        assert_snapshot!(view.draw_and_display(&mut buffer), @"
        248| Of course, in the beginning, this cannot be effected except by means of despotic inroads on the rights of
        249| T⃞hese measures will, of course, be different in different countries.                                     
        250|                                                                                                          
        251| Nevertheless, in most advanced countries, the following will be pretty generally applicable.             
        252|                                                                                                          
        253| 1. Abolition of property in land and application of all rents of land to public purposes.                
        254| 2. A heavy progressive or graduated income tax.                                                          
        255| 3. Abolition of all rights of inheritance.                                                               
        256| 4. Confiscation of the property of all emigrants and rebels.                                             
        257| 5. Centralisation of credit in the hands of the state, by means of a national bank with State capital and
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
        assert_snapshot!(view.draw_and_display(&mut buffer), @"
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

    #[test]
    fn resize_updates_the_cached_width_and_height() {
        let (_buffer, mut view) = setup_buffer_view();
        assert_eq!(view.width, 110);
        assert_eq!(view.height, 10);

        view.resize(42, 7);

        assert_eq!(view.width, 42);
        assert_eq!(view.height, 7);
    }

    #[test]
    fn resize_scrolls_up_to_keep_the_cursor_visible_when_shrinking() {
        let (mut buffer, mut view) = setup_buffer_view();
        // head is at line 250, top_line is 248, height is 10: the cursor is
        // comfortably visible.
        assert_eq!(view.selection.head.line, 250);
        assert_eq!(view.top_line, 248);

        // Shrinking to a single row used to leave `top_line` stale (still
        // 248), pushing the cursor's line off the bottom of the view and
        // crashing the next draw via `contains_internal_coord`'s assert.
        view.resize(110, 1);

        assert_eq!(view.top_line, 250);
        // Doesn't panic.
        view.draw_code(&mut buffer.as_sub_screen());
    }

    #[test]
    fn resize_scrolls_down_if_top_line_is_already_past_the_cursor() {
        let (mut buffer, mut view) = setup_buffer_view();
        // Simulate `top_line` having drifted ahead of the cursor's line.
        view.top_line = 255;

        view.resize(110, 10);

        assert_eq!(view.top_line, view.selection.head.line);
        // Doesn't panic.
        view.draw_code(&mut buffer.as_sub_screen());
    }

    #[test]
    fn resize_narrower_while_deep_in_a_wrapped_line_keeps_the_cursor_sensible() {
        let (_buffer, mut view) = setup_wrap_test("", 20, 5, true);
        for _ in 0..50 {
            view.insert('x');
        }
        assert_eq!(
            view.selection.head,
            Cursor {
                line: 0,
                column: 50
            }
        );

        // Narrow the view a lot while deep into a wrapped line: the same
        // line that used to wrap onto ~3 rows at width 20 now wraps onto
        // ~8 rows at width 10, far more than the view's height of 5.
        view.resize(10, 5);

        let mut buffer = ScreenBuffer::new(5, 10);
        view.draw_code(&mut buffer.as_sub_screen());

        // The cursor's own line is longer, wrapped, than the view is tall,
        // so it can never be fully visible -- but it should still land
        // somewhere sensible on that line, not jump to the unrelated
        // top-left corner of the view.
        assert_eq!(buffer.cursor().line, 4);
    }

    #[test]
    fn typing_past_the_view_width_with_soft_wrap_off_does_not_crash() {
        let (mut buffer, mut view) = setup_buffer_view();
        assert!(!view.soft_wrap);

        // Push the cursor's column far past the view's content width
        // (~105). This used to panic via `contains_internal_coord`'s assert
        // once `head.column + gutter_width` reached the view's width.
        for _ in 0..150 {
            view.insert('x');
        }

        view.draw_code(&mut buffer.as_sub_screen());

        // Clamped to the rightmost visible column instead of overflowing.
        assert_eq!(
            buffer.cursor(),
            ScreenCoord {
                line: 2,
                column: 109
            }
        );
    }

    #[test]
    fn soft_wrap_renders_a_continuation_row_with_a_wrap_marker_gutter() {
        let (mut buffer, view) = setup_wrap_test("0123456789ABCDEF", 10, 5, true);

        assert_snapshot!(buffer_text(&view, &mut buffer), @"
        0| 0123456
         ↪ 789ABCD
         ↪ EF
        ");
    }

    #[test]
    fn soft_wrap_moves_the_cursor_onto_the_wrapped_row() {
        let (mut buffer, mut view) = setup_wrap_test("", 10, 5, true);

        for c in "abcdefghi".chars() {
            view.insert(c);
        }
        assert_eq!(view.selection.head, Cursor { line: 0, column: 9 });

        view.draw_code(&mut buffer.as_sub_screen());

        // content_width is 7 (10 - gutter_width of 3): column 9 is on the
        // second wrapped row (segment 1), at offset 9 % 7 == 2.
        assert_eq!(buffer.cursor(), ScreenCoord { line: 1, column: 5 });
    }

    #[test]
    fn cursor_exactly_at_the_wrap_boundary_moves_to_the_start_of_the_next_row() {
        let (mut buffer, mut view) = setup_wrap_test("", 10, 5, true);

        for c in "abcdefg".chars() {
            view.insert(c);
        }

        assert_snapshot!(buffer_text(&view, &mut buffer), @"
        0| abcdefg
         ↪
        ");

        assert_eq!(view.selection.head, Cursor { line: 0, column: 7 });

        view.draw_code(&mut buffer.as_sub_screen());

        // The cursor should sit at the start of the next line
        assert_eq!(buffer.cursor(), ScreenCoord { line: 1, column: 3 });

        view.draw_code(&mut buffer.as_sub_screen());

        assert_snapshot!(buffer_text(&view, &mut buffer), @"
        0| abcdefg
         ↪
        ");
    }

    #[test]
    fn soft_wrap_can_wrap_a_line_more_than_once() {
        let (mut buffer, view) = setup_wrap_test("0123456789ABCDEFGH\nnext", 8, 6, true);

        assert_snapshot!(buffer_text(&view, &mut buffer), @"
        0| 01234
         ↪ 56789
         ↪ ABCDE
         ↪ FGH  
        1| next
        ");
    }

    #[test]
    fn soft_wrap_line_ending_exactly_on_a_chunk_boundary_gets_a_blank_continuation_row() {
        let (mut buffer, view) = setup_wrap_test("01234\nnext", 8, 4, true);

        assert_snapshot!(buffer_text(&view, &mut buffer), @"
        0| 01234
         ↪      
        1| next
        ");
    }

    #[test]
    fn soft_wrap_scrolls_earlier_once_a_wrapped_line_fills_the_screen() {
        let (_buffer, mut view) = setup_wrap_test("01234567\nb\nc\nd", 8, 3, true);
        assert!(view.soft_wrap);
        assert_eq!(view.top_line, 0);

        // Line 0 ("01234567") wraps onto 2 screen rows (content_width is 5),
        // so line 1 ("b") already lands on the 3rd (last) visible row.
        view.move_anchor(Anchor::Head, Direction::Down, SelectionMode::Char);
        assert_eq!(view.top_line, 0);
        assert_eq!(view.selection.head.line, 1);

        // Moving down once more must scroll now, since the 3 rows are
        // already fully used by wrapped line 0 + line 1 -- a line-count
        // based check (as used when soft_wrap is off) wouldn't scroll until
        // the anchor reached buffer line 3.
        view.move_anchor(Anchor::Head, Direction::Down, SelectionMode::Char);
        assert_eq!(view.top_line, 1);
        assert_eq!(view.selection.head.line, 1);
    }
}
