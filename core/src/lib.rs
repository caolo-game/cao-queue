//! # Features
//!
//! | name | description |
//! | :-- | :-- |
//! | collections | Enables message collections module. Most useful for servers |
//! | serde | Enables serde integration |
//!
pub mod collections;
pub mod message;

pub mod commands;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MessageId(pub u64);

/// Role of a client
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Role {
    NoRole = 0,
    Producer = 1,
    Consumer = 1 << 1,
    /// Indicates a client that's both producer and consumer
    ProdCon = 1 + (1 << 1),
}

impl Role {
    #[inline]
    pub fn is_producer(self) -> bool {
        (self as u8 & Role::Producer as u8) != 0
    }

    #[inline]
    pub fn is_consumer(self) -> bool {
        (self as u8 & Role::Consumer as u8) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roles() {
        let role = Role::Producer;

        assert!(role.is_producer());
        assert!(!role.is_consumer());

        let role = Role::Consumer;

        assert!(!role.is_producer());
        assert!(role.is_consumer());

        let role = Role::ProdCon;

        assert!(role.is_producer());
        assert!(role.is_consumer());
    }
}
