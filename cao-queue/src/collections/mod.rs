pub mod spmcfifo;
pub mod bucket_allocator;

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("Got invalid size: {0}")]
    BadSize(usize),
    #[error("Queue is full")]
    Full,
}
