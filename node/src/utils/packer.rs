use crate::utils::rlp::RlpError;
use thiserror::Error;

pub struct Packer {
    bytes: Vec<u8>,
}

#[allow(unused)]
impl Packer {
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub fn new_with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
        }
    }

    pub fn pack_fixed_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes)
    }

    pub fn pack_bytes(&mut self, bytes: &[u8]) {
        self.pack_med(&(bytes.len() as u32));
        self.bytes.extend_from_slice(bytes);
    }

    pub fn pack_short(&mut self, short: &u16) {
        self.bytes.extend_from_slice(&short.to_be_bytes())
    }

    pub fn pack_med(&mut self, med: &u32) {
        self.bytes.extend_from_slice(&med.to_be_bytes())
    }

    pub fn pack_long(&mut self, long: &u64) {
        self.bytes.extend_from_slice(&long.to_be_bytes())
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Debug, Error)]
pub enum PackerError {
    #[error("Rlp error: {0:?}")]
    Rlp(#[from] RlpError),
    #[error("Not enough bytes when unpacking {0} (need {1})")]
    UnpackLen(String, usize),
    #[error("Conversion error with {0}")]
    Conversion(String),
}
