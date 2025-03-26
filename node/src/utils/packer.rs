pub struct Packer {
    bytes: Vec<u8>,
}

impl Packer {
    pub fn new_with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
        }
    }

    pub fn pack_fixed_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes)
    }

    pub fn pack_short(&mut self, short: &u16) {
        self.bytes.append(&mut short.to_be_bytes().to_vec())
    }

    pub fn pack_long(&mut self, long: &u64) {
        self.bytes.append(&mut long.to_be_bytes().to_vec())
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}
