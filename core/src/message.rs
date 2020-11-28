use std::{fmt::Debug, io::Write, ops::Deref, ops::DerefMut};

use crate::MessageId;

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OwnedMessage {
    pub id: MessageId,
    pub payload: Vec<u8>,
}

impl Debug for OwnedMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedMessage")
            .field("id", &self.id)
            .finish()
    }
}

impl Write for OwnedMessage {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.payload.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl OwnedMessage {
    pub fn new(id: MessageId, capacity: usize) -> Self {
        Self {
            id,
            payload: Vec::with_capacity(capacity),
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.payload.as_mut_slice()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.payload.as_slice()
    }
}

impl Deref for OwnedMessage {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for OwnedMessage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}
