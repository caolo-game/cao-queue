use std::fmt::Debug;

use crate::{collections::QueueError, message::OwnedMessage, MessageId, Role};

/// The __On success__ sections will describe the valid responses for a given command.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Command {
    /// Operate on the given queue
    /// ## On success
    ///
    /// - Success
    ActiveQueue {
        role: Role,
        name: String,
        /// create the queue by name if not exists
        create: bool,
    },
    /// Switches the current client's role, but does not change active queue
    ///
    /// ## On success
    ///
    /// - Success
    ChangeRole(Role),
    /// ## On success
    ///
    /// - MessageId
    PushMsg(Vec<u8>),
    /// Attempts to pop a message from the active queue
    ///
    /// ## On success
    ///
    /// - Success if the queue was empty
    /// - Message
    PopMsg,
    /// Waits until a message is available and pops it
    ///
    /// ## On success
    ///
    /// - Message
    ListenForMsg,
    /// Clears the current queue
    ///
    /// ## On success
    ///
    /// - Success
    ClearQueue,
}

impl Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::ActiveQueue { role, name, create } => f
                .debug_struct("Command::ActiveQueue")
                .field("role", role)
                .field("name", name)
                .field("create", create)
                .finish(),
            Command::ChangeRole(role) => f
                .debug_struct("Command::ChangeRole")
                .field("role", role)
                .finish(),
            // this is why this is implemented 'by hand'
            Command::PushMsg(_) => f.debug_struct("Command::PushMsg").finish(),
            Command::PopMsg => f.debug_struct("Command::PopMsg").finish(),
            Command::ClearQueue => f.debug_struct("Command::ClearQueue").finish(),
            Command::ListenForMsg => f.debug_struct("Command::ListenForMsg").finish(),
        }
    }
}

pub type CommandResult = Result<CommandResponse, CommandError>;

/// Possible results of a command
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CommandResponse {
    Success,
    Message(OwnedMessage),
    MessageId(MessageId),
}

#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CommandError {
    #[error( "Attempting to take the producer role on a single-producer queue that already has a producer.")]
    HasProducer,
    #[error("Given command was malformed.")]
    BadCommand,
    #[error("Queue does not exist")]
    QueueNotFound,
    #[error("Attempting to push to a queue without producer role")]
    NotProducer,
    #[error("Attempting to pop a queue without consumer role")]
    NotConsumer,
    #[error("The attempted command would produce a deadlock.")]
    WouldBlock,
    #[error("The producer disconnected while the current command was waiting for it")]
    LostProducer,
    #[error("Error in the underlying queue {0}")]
    QueueError(QueueError),
}
