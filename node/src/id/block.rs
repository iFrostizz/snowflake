use super::{Id, IdError};

#[derive(Debug, Clone, Eq, Hash, PartialEq, Default)]
pub struct BlockID {
    id: Id<{ Self::LEN }>,
}

impl BlockID {
    pub const LEN: usize = 32;
}

impl From<[u8; Self::LEN]> for BlockID {
    fn from(value: [u8; Self::LEN]) -> Self {
        Self {
            id: Id::from(value),
        }
    }
}

impl TryFrom<&[u8]> for BlockID {
    type Error = IdError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let arr: [u8; Self::LEN] = value.try_into().map_err(|_| IdError::WrongSize)?;
        Ok(arr.into())
    }
}

impl std::fmt::Display for BlockID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl AsRef<[u8]> for BlockID {
    fn as_ref(&self) -> &[u8] {
        self.id.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::BlockID;
    use rand::Rng;

    #[test]
    fn conversion() {
        let bytes: [u8; BlockID::LEN] = rand::thread_rng().gen();
        let block_id = BlockID::from(bytes);
        assert_eq!(BlockID::try_from(bytes.as_slice()).unwrap(), block_id);
        assert_eq!(block_id.as_ref(), &bytes);
    }
}
