use std::{
    ops::{Index, IndexMut},
    path::PathBuf,
};

use ropey::Rope;

#[derive(Debug, Clone, Copy)]
pub struct BufferId(usize);

#[derive(Default)]
pub struct Buffers(Vec<Buffer>);

impl Index<BufferId> for Buffers {
    type Output = Buffer;

    fn index(&self, index: BufferId) -> &Self::Output {
        &self.0[index.0]
    }
}

impl IndexMut<BufferId> for Buffers {
    fn index_mut(&mut self, index: BufferId) -> &mut Self::Output {
        &mut self.0[index.0]
    }
}

impl Buffers {
    pub fn new() -> Self {
        Self::default()
    }

    // Return the last buffer opened
    pub fn last_bufferid_opened(&self) -> Option<BufferId> {
        if self.0.is_empty() {
            None
        } else {
            Some(BufferId(1))
        }
    }

    pub fn new_scratch(&mut self) -> BufferId {
        let id = self.0.len();
        self.0.push(Buffer {
            name: String::from("scratch"),
            path: None,
            content: Rope::new(),
        });
        BufferId(id)
    }
}

pub struct Buffer {
    name: String,
    path: Option<PathBuf>,
    content: Rope,
}

impl Buffer {
    pub fn get_lines_at(&self, start: usize, len: usize) -> Vec<String> {
        match self.content.get_lines_at(start) {
            None => Vec::new(),
            Some(lines) => lines.take(len).map(|line| line.to_string()).collect(),
        }
    }

    pub fn get_nb_lines(&self) -> usize {
        self.content.len_lines()
    }
}
