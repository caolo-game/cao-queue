use caoq_core::commands::{Command, CommandResult};
use slog::{debug, Logger};

pub mod spmc;

pub enum QueueClient {
    Spmc(spmc::SpmcClient),
}

impl QueueClient {
    pub fn cleanup(&mut self) {
        match self {
            QueueClient::Spmc(c) => c.cleanup(),
        }
    }

    pub async fn handle_command(&mut self, log: Logger, cmd: Command) -> CommandResult {
        debug!(log, "Handling command {:?}", cmd);
        match self {
            QueueClient::Spmc(c) => c.handle_command(log, cmd).await,
        }
    }
}
