use std::{
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc,
    },
};

use ropey::Rope;

use crate::{
    action::Action,
    server::command::{Command, GlobalCommand, GlobalQuery, Query},
};

mod buffer;
mod command;

pub use buffer::{Buffer, BufferId, Buffers};

pub struct Server {
    receiver: Receiver<Command>,
    buffers: Buffers,
}

#[derive(Clone)]
pub struct ServerHandle {
    sender: Sender<Command>,
}

impl ServerHandle {
    pub fn stop(&self) {
        let (sender, receiver) = oneshot::channel();
        let _ = self
            .sender
            .send(Command::GlobalCommand(GlobalCommand::Quit(sender)));
        let _ = receiver.recv();
    }

    pub fn current_buffer(&self) -> (BufferId, Arc<Buffer>) {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::GlobalQuery(GlobalQuery::GetDefaultBuffer(sender)))
            .unwrap();
        receiver.recv().unwrap()
    }

    pub fn get_lines(&self, buffer: BufferId, start: usize, len: usize) -> (Vec<String>, usize) {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::LocalQuery {
                buffer,
                query: Query::GetLines { start, len, sender },
            })
            .unwrap();
        receiver.recv().unwrap()
    }
}

impl Server {
    pub fn new() -> ServerHandle {
        let (sender, receiver) = mpsc::channel();
        let server = Self {
            receiver,
            buffers: Buffers::new(),
        };
        server.run();
        ServerHandle { sender }
    }

    pub fn run(mut self) {
        std::thread::spawn(move || {
            for action in self.receiver {
                match action {
                    Command::GlobalCommand(global_command) => match global_command {
                        GlobalCommand::Quit(sender) => {
                            let _ = sender.send(());
                            break;
                        }
                    },
                    Command::GlobalQuery(global_query) => match global_query {
                        GlobalQuery::GetDefaultBuffer(sender) => {
                            if let Some(buffer) = self.buffers.last_bufferid_opened() {
                                let _ = sender.send(buffer);
                            } else {
                                let scratch = self.buffers.new_scratch();
                                let _ = sender.send(scratch);
                            }
                        }
                        GlobalQuery::CreateScratchBuffer(sender) => {
                            let scratch = self.buffers.new_scratch();
                            let _ = sender.send(scratch);
                        }
                    },
                    Command::LocalQuery { buffer, query } => match query {
                        Query::GetLines { start, len, sender } => {
                            let buffer = &self.buffers[buffer];
                            let nb_lines = buffer.get_nb_lines();
                            let lines = buffer.get_lines_at(start, len);
                            let _ = sender.send((lines, nb_lines));
                        }
                    },
                }
            }
        });
    }
}
