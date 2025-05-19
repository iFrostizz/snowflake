use super::Id;

#[derive(PartialEq, Debug, Clone, Copy)]
pub struct ChainId(pub(crate) Id<{ Self::LEN }>);

impl ChainId {
    pub const LEN: usize = 32;
}

impl From<[u8; Self::LEN]> for ChainId {
    fn from(value: [u8; Self::LEN]) -> Self {
        Self(value.into())
    }
}

impl AsRef<[u8]> for ChainId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

pub(crate) const MAINNET_C_CHAIN_ID: ChainId = ChainId(Id {
    inner: [
        4, 39, 212, 178, 42, 42, 120, 188, 221, 212, 86, 116, 44, 175, 145, 181, 107, 173, 191,
        249, 133, 238, 25, 174, 241, 69, 115, 231, 52, 63, 214, 82,
    ],
});
pub(crate) const FUJI_C_CHAIN_ID: ChainId = ChainId(Id {
    inner: [
        127, 201, 61, 133, 198, 214, 44, 91, 42, 192, 181, 25, 200, 112, 16, 234, 82, 148, 1, 45,
        30, 64, 112, 48, 214, 172, 208, 2, 28, 172, 16, 213,
    ],
});
pub(crate) const LOCAL_C_CHAIN_ID: ChainId = ChainId(Id {
    inner: [
        238, 72, 130, 4, 8, 142, 171, 163, 211, 192, 197, 226, 109, 122, 40, 146, 103, 127, 141, 97, 129, 239, 49, 132, 135, 8, 180, 229, 94, 173, 118, 103
    ],
});