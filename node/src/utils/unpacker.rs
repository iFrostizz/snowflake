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
            .ok_or(PackerError::UnpackLen(name, 32))?;
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

#[derive(Debug)]
pub struct StatelessBlock {
    #[allow(unused)]
    pub version: Option<Vec<u8>>,
    #[allow(unused)]
    pub parent_id: BlockID,
    #[allow(unused)]
    pub timestamp: u64,
    #[allow(unused)]
    pub p_chain_height: u64,
    #[allow(unused)]
    pub certificate: Vec<u8>,
    pub block: Block,
    pub id: BlockID,
    #[allow(unused)]
    pub sig: Vec<u8>,
}

impl StatelessBlock {
    pub fn unpack(bytes: &[u8]) -> Result<StatelessBlock, PackerError> {
        let mut cursor = 0;
        cursor += 6; // version

        let parent_id = BlockID::from(Unpacker::unpack_fixed_bytes(
            bytes,
            &mut cursor,
            "parent_id".to_string(),
        )?);

        let timestamp = u64::from_be_bytes(Unpacker::unpack_fixed_bytes(
            bytes,
            &mut cursor,
            "timestamp".to_string(),
        )?);

        let p_chain_height = u64::from_be_bytes(Unpacker::unpack_fixed_bytes(
            bytes,
            &mut cursor,
            "p_chain_height".to_string(),
        )?);

        let cert_bytes = Unpacker::unpack_bytes(bytes, &mut cursor, "cert_bytes".to_string())?;

        let rlp_bytes = Unpacker::unpack_bytes(bytes, &mut cursor, "rlp_bytes".to_string())?;
        let block = Block::decode(rlp_bytes)?;

        let sig_bytes = Unpacker::unpack_bytes(bytes, &mut cursor, "sig_bytes".to_string())?;

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
            version: None,
            parent_id,
            timestamp,
            p_chain_height,
            certificate: cert_bytes.to_vec(), // TODO (optional) verify proposer if not empty
            block,
            id,
            sig: sig_bytes.to_vec(),
        })
    }
}
