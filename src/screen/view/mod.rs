use ropey::{iter::Chunks, RopeSlice};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

pub mod buffer_view;

/// An implementation of a graphemes iterator, for iterating over
/// the graphemes of a RopeSlice.
/// All this code comes from https://github.com/cessen/ropey/blob/master/examples/graphemes_iter.rs
pub struct RopeGraphemes<'a> {
    text: RopeSlice<'a>,
    chunks: Chunks<'a>,
    cur_chunk: &'a str,
    cur_chunk_start: usize,
    cursor: GraphemeCursor,
}

impl<'a> RopeGraphemes<'a> {
    fn new<'b>(slice: &RopeSlice<'b>) -> RopeGraphemes<'b> {
        let mut chunks = slice.chunks();
        let first_chunk = chunks.next().unwrap_or("");
        RopeGraphemes {
            text: *slice,
            chunks: chunks,
            cur_chunk: first_chunk,
            cur_chunk_start: 0,
            cursor: GraphemeCursor::new(0, slice.len_bytes(), true),
        }
    }
}

impl<'a> Iterator for RopeGraphemes<'a> {
    type Item = RopeSlice<'a>;

    fn next(&mut self) -> Option<RopeSlice<'a>> {
        let a = self.cursor.cur_cursor();
        let b;
        loop {
            match self
                .cursor
                .next_boundary(self.cur_chunk, self.cur_chunk_start)
            {
                Ok(None) => {
                    return None;
                }
                Ok(Some(n)) => {
                    b = n;
                    break;
                }
                Err(GraphemeIncomplete::NextChunk) => {
                    self.cur_chunk_start += self.cur_chunk.len();
                    self.cur_chunk = self.chunks.next().unwrap_or("");
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (chunk, byte_idx, _, _) = self.text.chunk_at_byte(idx.saturating_sub(1));
                    self.cursor.provide_context(chunk, byte_idx);
                }
                _ => unreachable!(),
            }
        }

        if a < self.cur_chunk_start {
            let a_char = self.text.byte_to_char(a);
            let b_char = self.text.byte_to_char(b);

            Some(self.text.slice(a_char..b_char))
        } else {
            let a2 = a - self.cur_chunk_start;
            let b2 = b - self.cur_chunk_start;
            Some((&self.cur_chunk[a2..b2]).into())
        }
    }
}

/// Splits a one buffer line fixed-width chunks for soft-wrap.
/// A trailing `\n` terminate the chunk without being included or counted
/// in the `content_width`.
/// Always yields at least one chunk, so an empty line still count as one line.
pub struct WrapChunks<'a> {
    graphemes: RopeGraphemes<'a>,
    content_width: usize,
    done: bool,
    started: bool,
    /// Whether the previously yielded chunk was exactly `content_width`.
    /// That's helpful to know where the cursor should be drawn next.
    last_was_full: bool,
}

impl<'a> WrapChunks<'a> {
    pub fn new(line: &RopeSlice<'a>, content_width: usize) -> WrapChunks<'a> {
        WrapChunks {
            graphemes: RopeGraphemes::new(line),
            content_width: content_width.max(1),
            done: false,
            started: false,
            last_was_full: false,
        }
    }
}

impl<'a> Iterator for WrapChunks<'a> {
    type Item = Vec<RopeSlice<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let mut chunk = Vec::with_capacity(self.content_width);
        for _ in 0..self.content_width {
            match self.graphemes.next() {
                Some(g) if g.chars().next() == Some('\n') => {
                    self.done = true;
                    self.started = true;
                    return Some(chunk);
                }
                Some(g) => chunk.push(g),
                None => {
                    self.done = true;
                    let was_started = self.started;
                    let last_was_full = self.last_was_full;
                    self.started = true;
                    return if chunk.is_empty() && was_started && !last_was_full {
                        // The line ended mid-row: no extra blank row.
                        None
                    } else {
                        // Either the line is empty or ended exactly on
                        // the previous chunk's boundary.
                        // Either way, a row starts here.
                        Some(chunk)
                    };
                }
            }
        }
        self.started = true;
        self.last_was_full = true;
        Some(chunk)
    }
}
