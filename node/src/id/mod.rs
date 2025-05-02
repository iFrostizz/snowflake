mod block;
pub use block::*;
mod chain;
pub use chain::*;
mod node;
pub use node::*;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::fmt::Formatter;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdError {
    #[error("ID has wrong size")]
    WrongSize,
    #[error("not starting with prefix")]
    MissingPrefix,
    #[error("cb58 decode error")]
    CB58DecodeError,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct Id<const LEN: usize> {
    inner: [u8; LEN],
}

impl<const LEN: usize> Id<LEN> {
    pub fn as_slice(&self) -> &[u8] {
        self.inner.as_slice()
    }
}

impl<const LEN: usize> Default for Id<LEN> {
    fn default() -> Self {
        Self { inner: [0; LEN] }
    }
}

impl<const LEN: usize> TryFrom<&str> for Id<LEN> {
    type Error = IdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let bytes = bs58::decode(value)
            .as_cb58(None)
            .into_vec()
            .map_err(|_| IdError::CB58DecodeError)?;
        if bytes.len() != LEN {
            return Err(IdError::WrongSize);
        }

        let mut id = [0; LEN];
        id.copy_from_slice(&bytes);

        Ok(Self { inner: id })
    }
}

impl<const LEN: usize> From<[u8; LEN]> for Id<LEN> {
    fn from(value: [u8; LEN]) -> Self {
        Self { inner: value }
    }
}

impl<const LEN: usize> From<Id<LEN>> for [u8; LEN] {
    fn from(id: Id<LEN>) -> Self {
        id.inner
    }
}

impl<const LEN: usize> std::fmt::Display for Id<LEN> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            bs58::encode(self.inner).as_cb58(None).into_string()
        )
    }
}

impl<const LEN: usize> std::fmt::Debug for Id<LEN> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

impl<const LEN: usize> PartialOrd for Id<LEN> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<const LEN: usize> Ord for Id<LEN> {
    fn cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.inner.iter().zip(other.inner.iter()) {
            match a.cmp(b) {
                Ordering::Equal => continue,
                rest => return rest,
            }
        }

        Ordering::Equal
    }
}

impl<const LEN: usize> Serialize for Id<LEN> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de, const LEN: usize> Deserialize<'de> for Id<LEN> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id_string = <String>::deserialize(deserializer)?;
        let id = Id::try_from(id_string.as_str()).map_err(serde::de::Error::custom)?;
        Ok(id)
    }
}
