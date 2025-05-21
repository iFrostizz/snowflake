use super::packer::PackerError;
use crate::id::BlockID;
use crate::utils::rlp::Block;
use sha2::Digest;
use sha2::Sha256;

pub struct Unpacker;

impl Unpacker {
    pub fn unpack_fixed_bytes<const N: usize>(
        bytes: &[u8],
        cursor: &mut usize,
        name: String,
    ) -> Result<[u8; N], PackerError> {
        let res = bytes
            .get(*cursor..*cursor + N)
            .map(|bytes| bytes.try_into().unwrap())
            .ok_or(PackerError::UnpackLen(name, N))?;
        *cursor += N;
        Ok(res)
    }

    pub fn unpack_bytes<'a>(
        bytes: &'a [u8],
        cursor: &mut usize,
        name: String,
    ) -> Result<&'a [u8], PackerError> {
        let len = u32::from_be_bytes(Unpacker::unpack_fixed_bytes(
            bytes,
            cursor,
            name.clone() + "len",
        )?);
        let len = len as usize;
        let res = bytes
            .get(*cursor..*cursor + len)
            .ok_or(PackerError::UnpackLen(name, len))?;
        *cursor += len;
        Ok(res)
    }
}

#[allow(unused)]
#[derive(Debug)]
pub struct StatelessBlock {
    pub version: Option<u16>,
    pub parent_id: BlockID,
    pub timestamp: i64,
    pub p_chain_height: u64,
    pub certificate: Vec<u8>,
    pub block: Block,
    pub sig: Vec<u8>,

    id: BlockID,
    bytes: Vec<u8>,
}

impl StatelessBlock {
    /// `ParseWithoutVerification` in avalanchego
    pub fn unpack(bytes: Vec<u8>) -> Result<StatelessBlock, PackerError> {
        if let Ok(block) = Block::decode(&bytes) {
            let parent_id = BlockID::from(*block.header.parent_hash());
            let timestamp = i64::from_be_bytes(*block.header.time());
            let id = BlockID::from(block.hash().0);
            Ok(StatelessBlock {
                version: None,
                parent_id,
                timestamp,
                p_chain_height: 0,
                certificate: vec![],
                block,
                sig: vec![],
                id,
                bytes,
            })
        } else {
            let mut cursor = 0;

            // First unpack the version (2 bytes)
            let version = u16::from_be_bytes(Unpacker::unpack_fixed_bytes(
                &bytes,
                &mut cursor,
                "version".to_string(),
            )?);

            cursor += 4; // interface https://github.com/ava-labs/avalanchego/blob/84a2c77186ce66381ec1dbdca5b8537472bd46e2/codec/reflectcodec/type_codec.go#L638-L639

            // Then unpack the nested statelessUnsignedBlock
            let parent_id = BlockID::from(Unpacker::unpack_fixed_bytes(
                &bytes,
                &mut cursor,
                "parent_id".to_string(),
            )?);

            let timestamp = i64::from_be_bytes(Unpacker::unpack_fixed_bytes(
                &bytes,
                &mut cursor,
                "timestamp".to_string(),
            )?);

            let p_chain_height = u64::from_be_bytes(Unpacker::unpack_fixed_bytes(
                &bytes,
                &mut cursor,
                "p_chain_height".to_string(),
            )?);

            let cert_bytes = Unpacker::unpack_bytes(&bytes, &mut cursor, "cert_bytes".to_string())?;
            let block_bytes =
                Unpacker::unpack_bytes(&bytes, &mut cursor, "block_bytes".to_string())?;
            let sig_bytes = Unpacker::unpack_bytes(&bytes, &mut cursor, "sig_bytes".to_string())?;

            if cursor != bytes.len() {
                return Err(PackerError::Conversion(String::from("wrong block length")));
            }

            let unsigned_bytes_len = bytes
                .len()
                .checked_sub(i32::BITS as usize / 8)
                .ok_or_else(|| PackerError::Conversion(String::from("len underflow")))?
                .checked_sub(sig_bytes.len())
                .ok_or_else(|| PackerError::Conversion(String::from("len underflow")))?;

            let mut hasher = Sha256::new();
            hasher.update(&bytes[..unsigned_bytes_len]);
            let id_hash: [u8; BlockID::LEN] = hasher.finalize().into();
            let id = id_hash.into();

            Ok(StatelessBlock {
                version: Some(version), // Now we can store the version
                parent_id,
                timestamp,
                p_chain_height,
                certificate: cert_bytes.to_vec(),
                block: Block::decode(block_bytes)?,
                sig: sig_bytes.to_vec(),
                id,
                bytes,
            })
        }
    }

    pub fn id(&self) -> &BlockID {
        &self.id
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
