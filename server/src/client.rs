use caoq_core::commands::{Command, CommandResult};
use tracing::debug;

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

    pub async fn handle_command(&mut self, cmd: Command) -> CommandResult {
        debug!("Handling command {:?}", cmd);
        match self {
            QueueClient::Spmc(c) => c.handle_command(cmd).await,
        }
    }
}
