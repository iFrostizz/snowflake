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
// pub(crate) const LOCAL_C_CHAIN_ID: ChainId = ChainId(Id {
//     inner: [
//         181, 115, 193, 119, 229, 91, 19, 104, 235, 98, 10, 50, 135, 64, 142, 182, 190, 161, 233,
//         177, 25, 123, 125, 6, 165, 196, 181, 124, 247, 216, 114, 111,
//     ],
// });
pub(crate) const LOCAL_C_CHAIN_ID: ChainId = ChainId(Id {
    inner: [
        130, 255, 24, 51, 172, 12, 201, 235, 86, 212, 172, 153, 20, 104, 74, 12, 72, 130, 24, 59,
        53, 166, 182, 134, 16, 40, 36, 181, 34, 54, 93, 16,
    ],
});
