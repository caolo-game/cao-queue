#[cfg(feature = "collections")]
pub mod spmcfifo;

#[derive(Debug, Clone, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QueueError {
    #[error("Got invalid size: {0}")]
    BadSize(usize),
    #[error("Queue is full")]
    Full,
}
