use crate::server::BufferId;

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
    GetDefaultBuffer(oneshot::Sender<BufferId>),
    CreateScratchBuffer(oneshot::Sender<BufferId>),
}

#[derive(Debug)]
pub enum Query {
    GetLines {
        start: usize,
        len: usize,
        sender: oneshot::Sender<(Vec<String>, usize)>,
    },
}
