use std::sync::Arc;

use crate::server::{Buffer, BufferId};

#[derive(Debug)]
pub enum Command {
    GlobalCommand(GlobalCommand),
    GlobalQuery(GlobalQuery),
    LocalQuery { buffer: BufferId, query: Query },
}

#[derive(Debug)]
pub enum GlobalCommand {
    Quit(oneshot::Sender<()>),
}

#[derive(Debug)]
pub enum GlobalQuery {
    GetDefaultBuffer(oneshot::Sender<(BufferId, Arc<Buffer>)>),
    CreateScratchBuffer(oneshot::Sender<(BufferId, Arc<Buffer>)>),
}

#[derive(Debug)]
pub enum Query {
    GetLines {
        start: usize,
        len: usize,
        sender: oneshot::Sender<(Vec<String>, usize)>,
    },
}
