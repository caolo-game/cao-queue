#[cfg(feature = "collections")]
mod impls {
    use super::*;
    pub mod concurrent_message_map;
    pub mod spmcfifo;
}
#[cfg(feature = "collections")]
pub use impls::*;

#[derive(Debug, Clone, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QueueError {
    #[error("Got invalid size: {0}")]
    BadSize(usize),
    #[error("Queue is full")]
    Full,
}
