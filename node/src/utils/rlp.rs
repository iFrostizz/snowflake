use alloy::primitives::keccak256;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RlpError {
    #[error("too short")]
    InputTooShort,
    #[error("string exceeding length of u64::MAX")]
    StringTooLong,
    #[error("list exceeding length of u64::MAX")]
    ListTooLong,
    #[error("expected a string, got a list")]
    NotAString,
    #[error("expected a list, got a string")]
    NotAList,
}

#[derive(Debug)]
pub struct Block {
    pub hash: B32,
    pub header: Header,
    pub transactions: Vec<TransactionEnvelope>,
    pub uncles: Vec<Header>,
}

impl Block {
    pub fn decode(bytes: &[u8]) -> Result<Self, RlpError> {
        let hash = *keccak256(bytes);

        let mut cursor = 0;
        Rlp::decode_list(bytes, &mut cursor)?;
        let header = {
            Rlp::decode_list(bytes, &mut cursor)?;

            let parent_hash = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let uncle_hash = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let coinbase = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let state_root = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let tx_root = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let receipt_hash = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let bloom = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let difficulty = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let number = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let gas_limit = u64::from_be_bytes(Rlp::decode_fixed_bytes(bytes, &mut cursor)?);
            let gas_used = u64::from_be_bytes(Rlp::decode_fixed_bytes(bytes, &mut cursor)?);
            let time = u64::from_be_bytes(Rlp::decode_fixed_bytes(bytes, &mut cursor)?);
            let extra = Rlp::decode_string(bytes, &mut cursor)?.to_vec();
            let mix_digest = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let nonce = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;

            Header::Legacy {
                parent_hash,
                uncle_hash,
                coinbase,
                state_root,
                tx_root,
                receipt_hash,
                bloom,
                difficulty,
                number,
                gas_limit,
                gas_used,
                time,
                extra,
                mix_digest,
                nonce,
            }
        };

        let block = Block {
            hash,
            header,
            transactions: vec![],
            uncles: vec![],
        };
        Ok(block)
    }
}

struct Rlp;

impl Rlp {
    pub fn decode_string<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], RlpError> {
        let prefix = bytes.get(*cursor).ok_or(RlpError::InputTooShort)?;
        *cursor += 1;
        match prefix {
            0x00..=0x7f => Ok(bytes.get(*cursor - 1..*cursor).unwrap()),
            0x80..=0xb7 => {
                let len = prefix - 0x80;
                let res = bytes
                    .get(*cursor..*cursor + len as usize)
                    .ok_or(RlpError::InputTooShort);
                *cursor += len as usize;
                res
            }
            0xb8..=0xbf => {
                let len_len_bytes = prefix - 0xb7;
                let len_bytes_read = bytes
                    .get(*cursor..*cursor + len_len_bytes as usize)
                    .ok_or(RlpError::InputTooShort)?;
                if len_bytes_read.len() > 8 {
                    return Err(RlpError::StringTooLong);
                }
                let mut len_bytes = [0; 8];
                len_bytes[8 - len_bytes_read.len()..].copy_from_slice(len_bytes_read);
                *cursor += len_len_bytes as usize;
                let len = u64::from_be_bytes(len_bytes);
                let res = bytes
                    .get(*cursor..*cursor + len as usize)
                    .ok_or(RlpError::InputTooShort);
                *cursor += len as usize;
                res
            }
            _ => Err(RlpError::NotAString),
        }
    }

    pub fn decode_list(bytes: &[u8], cursor: &mut usize) -> Result<u64, RlpError> {
        let prefix = bytes.get(*cursor).ok_or(RlpError::InputTooShort)?;
        *cursor += 1;
        match prefix {
            0xc0..=0xf7 => {
                let len = prefix - 0xc0;
                Ok(len as u64)
            }
            0xf8..=0xff => {
                let len_len_bytes = prefix - 0xf7;
                let len_bytes_read = bytes
                    .get(*cursor..*cursor + len_len_bytes as usize)
                    .ok_or(RlpError::InputTooShort)?;
                if len_bytes_read.len() > 8 {
                    return Err(RlpError::ListTooLong);
                }
                let mut len_bytes = [0; 8];
                len_bytes[8 - len_bytes_read.len()..].copy_from_slice(len_bytes_read);
                *cursor += len_len_bytes as usize;
                let len = u64::from_be_bytes(len_bytes);
                Ok(len)
            }
            _ => Err(RlpError::NotAList),
        }
    }

    pub fn decode_fixed_bytes<const N: usize>(
        bytes: &[u8],
        cursor: &mut usize,
    ) -> Result<[u8; N], RlpError> {
        let bytes = Self::decode_string(bytes, cursor)?;
        if bytes.len() > N {
            return Err(RlpError::StringTooLong);
        }
        let mut res = [0; N];
        res[N - bytes.len()..].copy_from_slice(bytes);
        Ok(res)
    }
}

type B32 = [u8; 32];
type U256 = [u8; 32];
type Address = [u8; 20];
type Bloom = [u8; 256];
type Nonce = [u8; 8];

#[derive(Debug)]
pub enum Header {
    Legacy {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U256,
        gas_limit: u64,
        gas_used: u64,
        time: u64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
    },
    London {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U256,
        gas_limit: u64,
        gas_used: u64,
        time: u64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        base_fee: U256,
    },
    Shanghai {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U256,
        gas_limit: u64,
        gas_used: u64,
        time: u64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        base_fee: U256,
        withdrawal_root: B32,
    },
    Cancun {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U256,
        gas_limit: u64,
        gas_used: u64,
        time: u64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        base_fee: U256,
        withdrawal_root: B32,
        blob_gas_used: u64,
        excess_blob_gas: u64,
        parent_beacon_block_root: B32,
    },
    TODO {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U256,
        gas_limit: u64,
        gas_used: u64,
        time: u64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        TODO: (),
    },
}

impl Header {
    pub fn parent_hash(&self) -> B32 {
        match self {
            Header::Legacy { parent_hash, .. }
            | Header::London { parent_hash, .. }
            | Header::Shanghai { parent_hash, .. }
            | Header::Cancun { parent_hash, .. }
            | Header::TODO { parent_hash, .. } => *parent_hash,
        }
    }

    // TODO ...

    pub fn number(&self) -> U256 {
        match self {
            Header::Legacy { number, .. }
            | Header::London { number, .. }
            | Header::Shanghai { number, .. }
            | Header::Cancun { number, .. }
            | Header::TODO { number, .. } => *number,
        }
    }
}

#[derive(Debug)]
pub enum TransactionEnvelope {
    Legacy(TransactionLegacy),
    AccessList(TransactionAccessList),
    DynamicFee(TransactionDynamicFee),
    Blob(TransactionBlob),
}

impl TransactionEnvelope {
    pub fn decode(bytes: &[u8]) -> Result<Self, RlpError> {
        todo!()
    }

    pub fn hash(&self) -> B32 {
        todo!()
    }
}

#[derive(Debug)]
pub struct TransactionLegacy {
    pub nonce: u64,
    pub gas_price: U256,
    pub gas_limit: u64,
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

#[derive(Debug)]
pub struct TransactionAccessList {
    pub chain_id: U256,
    pub nonce: u64,
    pub gas_price: U256,
    pub gas_limit: u64,
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub access_list: Vec<AccessList>,
    pub y_parity: U256,
    pub r: U256,
    pub s: U256,
}

#[derive(Debug)]
pub struct TransactionDynamicFee {
    pub chain_id: U256,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas_limit: u64,
    pub destination: Address,
    pub amount: U256,
    pub data: Vec<u8>,
    pub access_list: Vec<AccessList>,
    pub y_parity: U256,
    pub r: U256,
    pub s: U256,
}

#[derive(Debug)]
pub struct TransactionBlob {
    pub chain_id: U256,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas_limit: u64,
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub access_list: Vec<AccessList>,
    pub max_fee_per_blob_gas: U256,
    pub blob_hashes: Vec<U256>,
    pub y_parity: U256,
    pub r: U256,
    pub s: U256,
}

#[derive(Debug)]
pub struct AccessList {
    pub address: Address,
    pub storage_keys: Vec<U256>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::unpacker::StatelessBlock;
    use std::fmt::Debug;
    use std::fs;

    const TEST_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/");

    fn can_parse<F, OK, ERR>(dir: &str, f: F)
    where
        F: Fn(&[u8]) -> Result<OK, ERR>,
        ERR: Debug,
    {
        let test_dir = TEST_DIR.to_owned() + dir;
        for entry in fs::read_dir(test_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() && path.extension().unwrap() == "bin" {
                let file_name = path.file_name().unwrap().to_str().unwrap();
                println!("{}....", file_name);

                let bytes = fs::read(&path).unwrap();
                println!("{:?}", &bytes);
                assert!(!bytes.is_empty());

                f(&bytes).unwrap();
                println!("ok!");
            } else {
                panic!("invalid path {:?}", path);
            }
        }
    }

    #[test]
    fn can_parse_block() {
        can_parse("blocks", StatelessBlock::unpack);
    }

    #[test]
    fn can_parse_transaction() {
        can_parse("txs", TransactionEnvelope::decode);
    }
}
