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
        /// create the queue using the provided options if not exists
        create: Option<QueueOptions>,
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
    /// Send the response of a message
    ///
    /// ## On success
    ///
    /// - Success
    MsgResponse { id: MessageId, payload: Vec<u8> },
    /// Sends all accumulated responses to the client
    ///
    /// ## On success
    ///
    /// - Message[]
    ConsumeResponses,
    /// Retrieve the response to the given message
    ///
    /// ## On success
    ///
    /// - Message
    /// - Success if there is no response available
    GetSingleResponse(MessageId),
    /// Waits until a message is available and pops it
    ///
    /// ## On success
    ///
    /// - Message
    ListenForMsg {
        /// maximum time to wait for a message before returning (in milliseconds)
        timeout_ms: Option<u64>,
    },
    /// Clears the current queue
    ///
    /// ## On success
    ///
    /// - Success
    ClearQueue,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QueueOptions {
    pub capacity: u32,
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
            Command::ListenForMsg { timeout_ms } => f
                .debug_struct("Command::ListenForMsg")
                .field("timeout_ms", timeout_ms)
                .finish(),
            Command::MsgResponse { id, .. } => f
                .debug_struct("Command::MsgResponse")
                .field("msg_id", id)
                .finish(),
            Command::ConsumeResponses => f.debug_struct("Command::ConsumeResponses").finish(),
            Command::GetSingleResponse(id) => f
                .debug_struct("Command::GetSingleResponse")
                .field("msg_id", id)
                .finish(),
        }
    }
}

pub type CommandResult = Result<CommandResponse, CommandError>;

/// Possible results of a command
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CommandResponse {
    Success,
    MessageId(MessageId),
    Message(OwnedMessage),
    Messages(Vec<OwnedMessage>),
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
    #[error("Command timed out")]
    Timeout,
}
