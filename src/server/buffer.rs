use std::{fmt, ops::Index, path::PathBuf, sync::Arc};

use ropey::Rope;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy)]
pub struct BufferId(usize);

#[derive(Default)]
pub struct Buffers(Vec<Arc<Buffer>>);

impl Index<BufferId> for Buffers {
    type Output = Buffer;

    fn index(&self, index: BufferId) -> &Self::Output {
        &self.0[index.0]
    }
}

impl Buffers {
    pub fn new() -> Self {
        Self::default()
    }

    // Return the last buffer opened
    pub fn last_bufferid_opened(&self) -> Option<(BufferId, Arc<Buffer>)> {
        if self.0.is_empty() {
            None
        } else {
            Some((BufferId(1), self.0[0].clone()))
        }
    }

    pub fn new_scratch(&mut self) -> (BufferId, Arc<Buffer>) {
        let id = self.0.len();
        let buffer = Arc::new(Buffer {
            name: String::from("scratch"),
            path: None,
            rope: Rope::new().into(),
        });
        self.0.push(buffer.clone());
        (BufferId(id), buffer)
    }
}

pub struct Buffer {
    pub name: String,
    pub path: Option<PathBuf>,
    pub rope: RwLock<Rope>,
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Buffer")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("content", &"...")
            .finish()
    }
}

impl Buffer {
    pub fn get_lines_at(&self, start: usize, len: usize) -> Vec<String> {
        match self.rope.blocking_read().get_lines_at(start) {
            None => Vec::new(),
            Some(lines) => lines.take(len).map(|line| line.to_string()).collect(),
        }
    }

    pub fn get_nb_lines(&self) -> usize {
        self.rope.blocking_read().len_lines()
    }
}
