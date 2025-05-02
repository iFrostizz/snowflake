use alloy::consensus::SignableTransaction;
use alloy::primitives::{keccak256, B256};
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
    #[error("invalid format")]
    InvalidFormat,
}

#[derive(Debug)]
pub struct Block {
    pub hash: B256,
    pub size: usize,
    pub header: Header,
    pub transactions: Vec<Transaction>,
    #[allow(unused)]
    pub uncles: Vec<Header>,
    pub version: u32,
    pub ext_data: Vec<u8>
}

impl Block {
    pub fn decode(bytes: &[u8]) -> Result<Self, RlpError> {
        let mut cursor = 0;
        let start_list = cursor;
        let length = Rlp::decode_list(bytes, &mut cursor)?;
        let (header, hash) = {
            let start_list = cursor;
            let length = Rlp::decode_list(bytes, &mut cursor)?;
            let hash = keccak256(&bytes[start_list..cursor + length as usize]);
            let header = Self::decode_header(&bytes[cursor..cursor + length as usize])?;
            cursor += length as usize;
            (header, hash)
        };
        let transactions = {
            let start_list = cursor;
            let length = Rlp::decode_list(bytes, &mut cursor)?;
            let transactions = Self::decode_transactions(&bytes[cursor..cursor + length as usize])?;
            cursor += length as usize;
            #[allow(clippy::let_and_return)]
            transactions
        };
        let uncles = {
            let length = Rlp::decode_list(bytes, &mut cursor)?;
            let mut uncles = Vec::new();
            for _ in 0..length {
                let start_list = cursor;
                let length = Rlp::decode_list(bytes, &mut cursor)?;
                let header = Self::decode_header(&bytes[cursor..cursor + length as usize])?;
                cursor += length as usize;
                uncles.push(header);
            }
            uncles
        };
        let version = { let arr = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            u32::from_be_bytes(arr)
        };
        let ext_data = Rlp::decode_string(bytes, &mut cursor)?.to_vec();
        // dbg!(&bytes[cursor..]);
        // if cursor != start_list + length as usize {
        //     return Err(RlpError::InvalidFormat);
        // }

        let block = Block {
            hash,
            size: bytes.len(),
            header,
            transactions,
            uncles,
            version,
            ext_data,
        };
        Ok(block)
    }

    fn decode_header(bytes: &[u8]) -> Result<Header, RlpError> {
        let mut cursor = 0;
        let parent_hash = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let uncle_hash = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let coinbase = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let state_root = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let tx_root = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let receipt_hash = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let bloom = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let difficulty = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let number = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let gas_limit = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let gas_used = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let time = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let extra = Rlp::decode_string(bytes, &mut cursor)?.to_vec();
        let mix_digest = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let nonce = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;

        // let header = if cursor == bytes.len() {
        //     Header::Legacy {
        //         parent_hash,
        //         uncle_hash,
        //         coinbase,
        //         state_root,
        //         tx_root,
        //         receipt_hash,
        //         bloom,
        //         difficulty,
        //         number,
        //         gas_limit,
        //         gas_used,
        //         time,
        //         extra,
        //         mix_digest,
        //         nonce,
        //     }
        // } else {
        //     let base_fee = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //     if cursor == bytes.len() {
        //         Header::EIP1559 {
        //             parent_hash,
        //             uncle_hash,
        //             coinbase,
        //             state_root,
        //             tx_root,
        //             receipt_hash,
        //             bloom,
        //             difficulty,
        //             number,
        //             gas_limit,
        //             gas_used,
        //             time,
        //             extra,
        //             mix_digest,
        //             nonce,
        //             base_fee,
        //         }
        //     } else {
        //         let withdrawal_root = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //         if cursor == bytes.len() {
        //             Header::EIP4895 {
        //                 parent_hash,
        //                 uncle_hash,
        //                 coinbase,
        //                 state_root,
        //                 tx_root,
        //                 receipt_hash,
        //                 bloom,
        //                 difficulty,
        //                 number,
        //                 gas_limit,
        //                 gas_used,
        //                 time,
        //                 extra,
        //                 mix_digest,
        //                 nonce,
        //                 base_fee,
        //                 withdrawal_root,
        //             }
        //         } else {
        //             let ext_data_gas_used = withdrawal_root;
        //             if cursor == bytes.len() {
        //                 let block_gas_cost = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 Header::Apricot4 {
        //                     parent_hash,
        //                     uncle_hash,
        //                     coinbase,
        //                     state_root,
        //                     tx_root,
        //                     receipt_hash,
        //                     bloom,
        //                     difficulty,
        //                     number,
        //                     gas_limit,
        //                     gas_used,
        //                     time,
        //                     extra,
        //                     mix_digest,
        //                     nonce,
        //                     base_fee,
        //                     ext_data_gas_used,
        //                     block_gas_cost,
        //                 }
        //             } else if cursor == bytes.len() {
        //                 let blob_gas_used = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 let excess_blob_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 Header::EIP4844 {
        //                     parent_hash,
        //                     uncle_hash,
        //                     coinbase,
        //                     state_root,
        //                     tx_root,
        //                     receipt_hash,
        //                     bloom,
        //                     difficulty,
        //                     number,
        //                     gas_limit,
        //                     gas_used,
        //                     time,
        //                     extra,
        //                     mix_digest,
        //                     nonce,
        //                     base_fee,
        //                     withdrawal_root,
        //                     blob_gas_used,
        //                     excess_blob_gas,
        //                 }
        //             } else if cursor == bytes.len() {
        //                 let blob_gas_used = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 let excess_blob_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 let parent_beacon_block_root = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 Header::EIP4788 {
        //                     parent_hash,
        //                     uncle_hash,
        //                     coinbase,
        //                     state_root,
        //                     tx_root,
        //                     receipt_hash,
        //                     bloom,
        //                     difficulty,
        //                     number,
        //                     gas_limit,
        //                     gas_used,
        //                     time,
        //                     extra,
        //                     mix_digest,
        //                     nonce,
        //                     base_fee,
        //                     withdrawal_root,
        //                     blob_gas_used,
        //                     excess_blob_gas,
        //                     parent_beacon_block_root,
        //                 }
        //             } else {
        //                 let block_gas_cost = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 let blob_gas_used = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 let excess_blob_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 let parent_beacon_block_root = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        //                 if cursor != bytes.len() {
        //                     return Err(RlpError::InvalidFormat);
        //                 }
        //                 Header::Extra {
        //                     parent_hash,
        //                     uncle_hash,
        //                     coinbase,
        //                     state_root,
        //                     tx_root,
        //                     receipt_hash,
        //                     bloom,
        //                     difficulty,
        //                     number,
        //                     gas_limit,
        //                     gas_used,
        //                     time,
        //                     extra,
        //                     mix_digest,
        //                     nonce,
        //                     ext_data_hash,
        //                     base_fee,
        //                     ext_data_gas_used,
        //                     block_gas_cost,
        //                     blob_gas_used,
        //                     excess_blob_gas,
        //                     parent_beacon_block_root,
        //                 }
        //             }
        //         }
        //     }
        // };

        let header = {
            let ext_data_hash = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let base_fee = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let ext_data_gas_used = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let block_gas_cost = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let blob_gas_used = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let excess_blob_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            let parent_beacon_block_root = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            Header::Extra {
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
                ext_data_hash,
                base_fee,
                ext_data_gas_used,
                block_gas_cost,
                blob_gas_used,
                excess_blob_gas,
                parent_beacon_block_root,
            }
        };
        Ok(header)
    }

    fn decode_transactions(bytes: &[u8]) -> Result<Vec<Transaction>, RlpError> {
        let mut transactions = Vec::new();
        let mut cursor = 0;
        while cursor < bytes.len() {
            if Rlp::is_list(bytes, &cursor)? {
                // legacy
                let start_light = cursor;
                let length = Rlp::decode_list(bytes, &mut cursor)?;
                let hash = keccak256(&bytes[start_light..cursor + length as usize]);
                let tx = TransactionLegacy::decode(&bytes[cursor..cursor + length as usize])?;
                cursor += length as usize;
                transactions.push(Transaction::Legacy { tx, hash });
            } else {
                // envelope
                let tx_bytes = Rlp::decode_string(bytes, &mut cursor)?;
                let hash = keccak256(tx_bytes);
                let envelope = TransactionEnvelope::decode(tx_bytes)?;
                transactions.push(Transaction::EIP2718 { envelope, hash });
            }
        }

        if cursor != bytes.len() {
            return Err(RlpError::InvalidFormat);
        }
        Ok(transactions)
    }

    pub fn encode(&self) -> Result<Vec<u8>, RlpError> {
        // TODO this function is allocating way too much!
        let mut buf = Vec::new();
        {
            let mut buf2 = Vec::new();
            {
                let mut buf3 = Vec::new();
                buf3.append(&mut self.header.encode()?);
                Rlp::encode_list(&mut buf2, &buf3)?;
            }
            {
                let mut buf3 = Vec::new();
                Self::encode_transactions(&mut buf3, &self.transactions)?;
                Rlp::encode_list(&mut buf2, &buf3)?;
            }
            {
                let mut buf3 = Vec::new();
                Self::encode_uncles(&mut buf3, &self.uncles)?;
                Rlp::encode_list(&mut buf2, &buf3)?;
            }
            {
                Rlp::encode_string(&mut buf2, &self.version.to_be_bytes())?;
            }
            {
                Rlp::encode_normalized_string(&mut buf2, &self.ext_data)?;
            }
            Rlp::encode_list(&mut buf, &buf2)?;
        }
        Ok(buf)
    }

    fn encode_transactions(
        buf: &mut Vec<u8>,
        transactions: &Vec<Transaction>,
    ) -> Result<(), RlpError> {
        for tx in transactions {
            match tx {
                Transaction::Legacy {
                    tx:
                        TransactionLegacy {
                            nonce,
                            gas_price,
                            gas_limit,
                            to,
                            value,
                            data,
                            v,
                            r,
                            s,
                        },
                    ..
                } => {
                    let mut ret2 = Vec::new();
                    Rlp::encode_string(&mut ret2, nonce)?;
                    Rlp::encode_string(&mut ret2, gas_price)?;
                    Rlp::encode_string(&mut ret2, gas_limit)?;
                    if to == &[0; 20] {
                        Rlp::encode_string(&mut ret2, &[])?;
                    } else {
                        Rlp::encode_normalized_string(&mut ret2, to)?;
                    }
                    Rlp::encode_string(&mut ret2, value)?;
                    Rlp::encode_string(&mut ret2, data)?;
                    Rlp::encode_string(&mut ret2, v)?;
                    Rlp::encode_string(&mut ret2, r)?;
                    Rlp::encode_string(&mut ret2, s)?;
                    Rlp::encode_list(buf, &ret2)?;
                }
                Transaction::EIP2718 { envelope, .. } => {
                    let mut ret2 = Vec::new();
                    match envelope {
                        TransactionEnvelope::DynamicFee(TransactionDynamicFee {
                            chain_id,
                            nonce,
                            max_priority_fee_per_gas,
                            max_fee_per_gas,
                            gas_limit,
                            destination,
                            amount,
                            data,
                            access_list,
                            v,
                            r,
                            s,
                        }) => {
                            ret2.push(0x02);
                            {
                                let mut ret3 = Vec::new();
                                Rlp::encode_string(&mut ret3, chain_id)?;
                                Rlp::encode_string(&mut ret3, nonce)?;
                                Rlp::encode_string(&mut ret3, max_priority_fee_per_gas)?;
                                Rlp::encode_string(&mut ret3, max_fee_per_gas)?;
                                Rlp::encode_string(&mut ret3, gas_limit)?;
                                if destination == &[0; 20] {
                                    Rlp::encode_string(&mut ret3, &[])?;
                                } else {
                                    Rlp::encode_normalized_string(&mut ret3, destination)?;
                                }
                                Rlp::encode_string(&mut ret3, amount)?;
                                Rlp::encode_string(&mut ret3, data)?;
                                {
                                    let mut ret4 = Vec::new();
                                    ret4.append(&mut access_list.encode()?);
                                    Rlp::encode_list(&mut ret3, &ret4)?;
                                }
                                Rlp::encode_string(&mut ret3, v)?;
                                Rlp::encode_string(&mut ret3, r)?;
                                Rlp::encode_string(&mut ret3, s)?;
                                Rlp::encode_list(&mut ret2, &ret3)?;
                            }
                        }
                        _ => todo!(),
                    }
                    Rlp::encode_string(buf, &ret2)?;
                }
            }
        }
        Ok(())
    }

    fn encode_uncles(buf: &mut Vec<u8>, uncles: &Vec<Header>) -> Result<(), RlpError> {
        for uncle in uncles {
            todo!()
        }
        Ok(())
    }
}

struct Rlp;

impl Rlp {
    pub fn is_list(bytes: &[u8], cursor: &usize) -> Result<bool, RlpError> {
        let prefix = bytes.get(*cursor).ok_or(RlpError::InputTooShort)?;
        match prefix {
            0xc0..=0xf7 => Ok(true),
            0xf8..=0xff => Ok(true),
            _ => Ok(false),
        }
    }

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

    pub fn encode_list(buf: &mut Vec<u8>, bytes: &[u8]) -> Result<(), RlpError> {
        let length = bytes.len() as u64;
        let mut list = match length {
            0..=55 => {
                let mut res = vec![0xc0 + length as u8];
                res.extend_from_slice(bytes);
                res
            }
            56.. => {
                let length_bytes = length.to_be_bytes();
                let length_bytes = {
                    let i = length_bytes.iter().position(|x| x != &0).unwrap();
                    &length_bytes[i..]
                };
                let mut res = vec![0xf7 + length_bytes.len() as u8];
                res.extend_from_slice(length_bytes);
                res.extend_from_slice(bytes);
                res
            }
        };
        buf.append(&mut list);
        Ok(())
    }

    pub fn encode_string(buf: &mut Vec<u8>, value: &[u8]) -> Result<(), RlpError> {
        if value.len() as u64 > u64::MAX {
            return Err(RlpError::StringTooLong);
        }
        let value = {
            match value.iter().position(|x| x != &0) {
                Some(i) => &value[i..],
                None => &[],
            }
        };
        Self::encode_normalized_string(buf, value)
    }

    pub fn encode_normalized_string(buf: &mut Vec<u8>, value: &[u8]) -> Result<(), RlpError> {
        if value.len() as u64 > u64::MAX {
            return Err(RlpError::StringTooLong);
        }
        let len = value.len();
        let mut string = match len {
            1 if value.first().unwrap() <= &0x7f => vec![*value.first().unwrap()],
            0..=55 => {
                let mut res = vec![0x80 + len as u8];
                res.extend_from_slice(value);
                res
            }
            56.. => {
                let length_bytes = len.to_be_bytes();
                let length_bytes = {
                    let i = length_bytes.iter().position(|x| x != &0).unwrap();
                    &length_bytes[i..]
                };
                let mut res = vec![0xb7 + length_bytes.len() as u8];
                res.extend_from_slice(length_bytes);
                res.extend_from_slice(value);
                res
            }
        };
        buf.append(&mut string);
        Ok(())
    }
}

type B32 = [u8; 32];
type U256 = [u8; 32];
type U64 = [u8; 8];
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
        number: U64,
        gas_limit: U64,
        gas_used: U64,
        time: U64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
    },
    EIP1559 {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U64,
        gas_limit: U64,
        gas_used: U64,
        time: U64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        base_fee: U256,
    },
    EIP4895 {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U64,
        gas_limit: U64,
        gas_used: U64,
        time: U64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        base_fee: U256,
        withdrawal_root: B32,
    },
    Apricot4 {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U64,
        gas_limit: U64,
        gas_used: U64,
        time: U64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        base_fee: U256,
        ext_data_gas_used: U256,
        block_gas_cost: U256,
    },
    EIP4844 {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U64,
        gas_limit: U64,
        gas_used: U64,
        time: U64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        base_fee: U256,
        withdrawal_root: B32,
        blob_gas_used: U64,
        excess_blob_gas: U64,
    },
    EIP4788 {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U64,
        gas_limit: U64,
        gas_used: U64,
        time: U64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        base_fee: U256,
        withdrawal_root: U256,
        blob_gas_used: U64,
        excess_blob_gas: U64,
        parent_beacon_block_root: B32,
    },
    Extra {
        parent_hash: B32,
        uncle_hash: B32,
        coinbase: Address,
        state_root: B32,
        tx_root: B32,
        receipt_hash: B32,
        bloom: Bloom,
        difficulty: U256,
        number: U64,
        gas_limit: U64,
        gas_used: U64,
        time: U64,
        extra: Vec<u8>,
        mix_digest: B32,
        nonce: Nonce,
        ext_data_hash: B32,
        base_fee: U256,
        ext_data_gas_used: U256,
        block_gas_cost: U256,
        blob_gas_used: U64,
        excess_blob_gas: U64,
        parent_beacon_block_root: B32,
    },
}

macro_rules! make_helper {
    ($field:ident, $type:ty) => {
        pub fn $field(&self) -> &$type {
            match self {
                Header::Legacy { $field, .. }
                | Header::EIP1559 { $field, .. }
                | Header::EIP4895 { $field, .. }
                | Header::Apricot4 { $field, .. }
                | Header::EIP4844 { $field, .. }
                | Header::EIP4788 { $field, .. }
                | Header::Extra { $field, .. } => $field,
            }
        }
    };
}

impl Header {
    make_helper!(parent_hash, B32);
    make_helper!(uncle_hash, B32);
    make_helper!(coinbase, Address);
    make_helper!(state_root, B32);
    make_helper!(tx_root, B32);
    make_helper!(receipt_hash, B32);
    make_helper!(bloom, Bloom);
    make_helper!(difficulty, U256);
    make_helper!(number, U64);
    make_helper!(gas_limit, U64);
    make_helper!(gas_used, U64);
    make_helper!(time, U64);
    make_helper!(extra, Vec<u8>);
    make_helper!(mix_digest, B32);
    make_helper!(nonce, Nonce);

    fn encode(&self) -> Result<Vec<u8>, RlpError> {
        let mut buf = Vec::new();
        // match self {
        //     Header::Legacy {
        //         parent_hash,
        //         uncle_hash,
        //         coinbase,
        //         state_root,
        //         tx_root,
        //         receipt_hash,
        //         bloom,
        //         difficulty,
        //         number,
        //         gas_limit,
        //         gas_used,
        //         time,
        //         extra,
        //         mix_digest,
        //         nonce,
        //     }
        //     | Header::EIP1559 {
        //         parent_hash,
        //         uncle_hash,
        //         coinbase,
        //         state_root,
        //         tx_root,
        //         receipt_hash,
        //         bloom,
        //         difficulty,
        //         number,
        //         gas_limit,
        //         gas_used,
        //         time,
        //         extra,
        //         mix_digest,
        //         nonce,
        //         ..
        //     }
        //     | Header::EIP4895 {
        //         parent_hash,
        //         uncle_hash,
        //         coinbase,
        //         state_root,
        //         tx_root,
        //         receipt_hash,
        //         bloom,
        //         difficulty,
        //         number,
        //         gas_limit,
        //         gas_used,
        //         time,
        //         extra,
        //         mix_digest,
        //         nonce,
        //         ..
        //     }
        //     | Header::Apricot4 {
        //         parent_hash,
        //         uncle_hash,
        //         coinbase,
        //         state_root,
        //         tx_root,
        //         receipt_hash,
        //         bloom,
        //         difficulty,
        //         number,
        //         gas_limit,
        //         gas_used,
        //         time,
        //         extra,
        //         mix_digest,
        //         nonce,
        //         ..
        //     }
        //     | Header::EIP4844 {
        //         parent_hash,
        //         uncle_hash,
        //         coinbase,
        //         state_root,
        //         tx_root,
        //         receipt_hash,
        //         bloom,
        //         difficulty,
        //         number,
        //         gas_limit,
        //         gas_used,
        //         time,
        //         extra,
        //         mix_digest,
        //         nonce,
        //         ..
        //     }
        //     | Header::EIP4788 {
        //         parent_hash,
        //         uncle_hash,
        //         coinbase,
        //         state_root,
        //         tx_root,
        //         receipt_hash,
        //         bloom,
        //         difficulty,
        //         number,
        //         gas_limit,
        //         gas_used,
        //         time,
        //         extra,
        //         mix_digest,
        //         nonce,
        //         ..
        //     }
        //     | Header::Extra {
        //         parent_hash,
        //         uncle_hash,
        //         coinbase,
        //         state_root,
        //         tx_root,
        //         receipt_hash,
        //         bloom,
        //         difficulty,
        //         number,
        //         gas_limit,
        //         gas_used,
        //         time,
        //         extra,
        //         mix_digest,
        //         nonce,
        //         ..
        //     } => {
        //         Rlp::encode_string(&mut buf, parent_hash)?;
        //         Rlp::encode_string(&mut buf, uncle_hash)?;
        //         Rlp::encode_string(&mut buf, coinbase)?;
        //         Rlp::encode_normalized_string(&mut buf, state_root)?;
        //         Rlp::encode_normalized_string(&mut buf, tx_root)?;
        //         Rlp::encode_normalized_string(&mut buf, receipt_hash)?;
        //         Rlp::encode_normalized_string(&mut buf, bloom)?;
        //         Rlp::encode_string(&mut buf, difficulty)?;
        //         Rlp::encode_string(&mut buf, number)?;
        //         Rlp::encode_string(&mut buf, gas_limit)?;
        //         Rlp::encode_string(&mut buf, gas_used)?;
        //         Rlp::encode_string(&mut buf, time)?;
        //         Rlp::encode_normalized_string(&mut buf, extra)?;
        //         Rlp::encode_normalized_string(&mut buf, mix_digest)?;
        //         Rlp::encode_normalized_string(&mut buf, nonce)?;
        //         match self {
        //             Header::EIP1559 { base_fee, .. }
        //             | Header::EIP4895 { base_fee, .. }
        //             | Header::Apricot4 { base_fee, .. }
        //             | Header::EIP4844 { base_fee, .. }
        //             | Header::EIP4788 { base_fee, .. } => {
        //                 Rlp::encode_string(&mut buf, base_fee)?;
        //                 match self {
        //                     Header::EIP4895 {
        //                         withdrawal_root, ..
        //                     }
        //                     | Header::EIP4844 {
        //                         withdrawal_root, ..
        //                     }
        //                     | Header::EIP4788 {
        //                         withdrawal_root, ..
        //                     } => {
        //                         Rlp::encode_string(&mut buf, withdrawal_root)?;
        //                         match self {
        //                             Header::EIP4844 {
        //                                 blob_gas_used,
        //                                 excess_blob_gas,
        //                                 ..
        //                             }
        //                             | Header::EIP4788 {
        //                                 blob_gas_used,
        //                                 excess_blob_gas,
        //                                 ..
        //                             } => {
        //                                 Rlp::encode_string(&mut buf, blob_gas_used)?;
        //                                 Rlp::encode_string(&mut buf, excess_blob_gas)?;
        //                                 if let Header::EIP4788 {
        //                                     parent_beacon_block_root,
        //                                     ..
        //                                 } = self
        //                                 {
        //                                     Rlp::encode_string(&mut buf, parent_beacon_block_root)?;
        //                                 }
        //                             }
        //                             _ => (),
        //                         }
        //                     }
        //                     Header::Apricot4 {
        //                         ext_data_gas_used,
        //                         block_gas_cost,
        //                         ..
        //                     } => {
        //                         Rlp::encode_normalized_string(&mut buf, ext_data_gas_used)?;
        //                         Rlp::encode_string(&mut buf, block_gas_cost)?;
        //                     }
        //                     _ => (),
        //                 }
        //             }
        //             Header::Extra {
        //                 ext_data_hash,
        //                 base_fee,
        //                 ext_data_gas_used,
        //                 block_gas_cost,
        //                 blob_gas_used,
        //                 excess_blob_gas,
        //                 parent_beacon_block_root,
        //                 ..
        //             } => {
        //                 Rlp::encode_normalized_string(&mut buf, ext_data_hash)?;
        //                 Rlp::encode_string(&mut buf, base_fee)?;
        //                 Rlp::encode_string(&mut buf, ext_data_gas_used)?;
        //                 Rlp::encode_string(&mut buf, block_gas_cost)?;
        //                 Rlp::encode_string(&mut buf, blob_gas_used)?;
        //                 Rlp::encode_string(&mut buf, excess_blob_gas)?;
        //                 Rlp::encode_normalized_string(&mut buf, parent_beacon_block_root)?;
        //             }
        //             _ => (),
        //         }
        //     }
        // }
        let Header::Extra {
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
            ext_data_hash,
            base_fee,
            ext_data_gas_used,
            block_gas_cost,
            blob_gas_used,
            excess_blob_gas,
            parent_beacon_block_root,
        } = self
        else {
            panic!()
        };
        Rlp::encode_normalized_string(&mut buf, parent_hash)?;
        Rlp::encode_normalized_string(&mut buf, uncle_hash)?;
        Rlp::encode_string(&mut buf, coinbase)?;
        Rlp::encode_normalized_string(&mut buf, state_root)?;
        Rlp::encode_normalized_string(&mut buf, tx_root)?;
        Rlp::encode_normalized_string(&mut buf, receipt_hash)?;
        Rlp::encode_normalized_string(&mut buf, bloom)?;
        Rlp::encode_string(&mut buf, difficulty)?;
        Rlp::encode_string(&mut buf, number)?;
        Rlp::encode_string(&mut buf, gas_limit)?;
        Rlp::encode_string(&mut buf, gas_used)?;
        Rlp::encode_string(&mut buf, time)?;
        Rlp::encode_normalized_string(&mut buf, extra)?;
        Rlp::encode_normalized_string(&mut buf, mix_digest)?;
        Rlp::encode_normalized_string(&mut buf, nonce)?;
        Rlp::encode_normalized_string(&mut buf, ext_data_hash)?;
        Rlp::encode_string(&mut buf, base_fee)?;
        Rlp::encode_string(&mut buf, ext_data_gas_used)?;
        Rlp::encode_string(&mut buf, block_gas_cost)?;
        Rlp::encode_string(&mut buf, blob_gas_used)?;
        Rlp::encode_string(&mut buf, excess_blob_gas)?;
        Rlp::encode_normalized_string(&mut buf, parent_beacon_block_root)?;
        Ok(buf)
    }
}

impl From<Header> for alloy::consensus::Header {
    fn from(value: Header) -> Self {
        alloy::consensus::Header {
            parent_hash: value.parent_hash().into(),
            ommers_hash: value.uncle_hash().into(),
            beneficiary: value.coinbase().into(),
            state_root: value.state_root().into(),
            transactions_root: value.tx_root().into(),
            receipts_root: value.receipt_hash().into(),
            logs_bloom: value.bloom().into(),
            difficulty: ruint::Uint::from_be_bytes(*value.receipt_hash()),
            number: u64::from_be_bytes(*value.number()),
            gas_limit: u64::from_be_bytes(*value.gas_limit()),
            gas_used: u64::from_be_bytes(*value.gas_used()),
            timestamp: u64::from_be_bytes(*value.time()),
            extra_data: alloy::primitives::Bytes::copy_from_slice(value.extra()),
            mix_hash: value.mix_digest().into(),
            nonce: value.nonce().into(),
            base_fee_per_gas: None,
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
            parent_beacon_block_root: None,
            requests_hash: None,
        }
    }
}

#[derive(Debug)]
pub enum Transaction {
    Legacy {
        hash: B256,
        tx: TransactionLegacy,
    },
    EIP2718 {
        hash: B256,
        envelope: TransactionEnvelope,
    },
}

#[derive(Debug)]
pub enum TransactionEnvelope {
    AccessList(TransactionAccessList),
    DynamicFee(TransactionDynamicFee),
    Blob(TransactionBlob),
}

impl TransactionLegacy {
    pub fn decode(bytes: &[u8]) -> Result<Self, RlpError> {
        let mut cursor = 0;
        let nonce = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let gas_price = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let gas_limit = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let to = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let value = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let data = Rlp::decode_string(bytes, &mut cursor)?.to_vec();
        let v = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let r = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
        let s = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;

        Ok(TransactionLegacy {
            nonce,
            gas_price,
            gas_limit,
            to,
            value,
            data,
            v,
            r,
            s,
        })
    }
}

impl TransactionEnvelope {
    pub fn decode(bytes: &[u8]) -> Result<Self, RlpError> {
        // EIP-2781 envelope
        let mut cursor = 0;
        let prefix = bytes.get(cursor).ok_or(RlpError::InputTooShort)?;
        let envelope = match prefix {
            0x01 => {
                cursor += 1;
                let length = Rlp::decode_list(bytes, &mut cursor)?;
                let start_cursor = cursor;
                let chain_id = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let nonce = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let gas_price = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let gas_limit = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let to = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let value = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let data = Rlp::decode_string(bytes, &mut cursor)?.to_vec();
                let access_list = {
                    let length = Rlp::decode_list(bytes, &mut cursor)?;
                    let access_list = AccessList::decode(&bytes[cursor..cursor + length as usize])?;
                    cursor += length as usize;
                    access_list
                };
                let v = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let r = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let s = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;

                if cursor != bytes.len() || cursor != start_cursor + length as usize {
                    return Err(RlpError::InvalidFormat);
                }

                TransactionEnvelope::AccessList(TransactionAccessList {
                    chain_id,
                    nonce,
                    gas_price,
                    gas_limit,
                    to,
                    value,
                    data,
                    access_list,
                    v,
                    r,
                    s,
                })
            }
            0x02 => {
                cursor += 1;
                let length = Rlp::decode_list(bytes, &mut cursor)?;
                let start_cursor = cursor;
                let chain_id = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let nonce = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let max_priority_fee_per_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let max_fee_per_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let gas_limit = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let destination = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let amount = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let data = Rlp::decode_string(bytes, &mut cursor)?.to_vec();
                let access_list = {
                    let length = Rlp::decode_list(bytes, &mut cursor)?;
                    let access_list = AccessList::decode(&bytes[cursor..cursor + length as usize])?;
                    cursor += length as usize;
                    access_list
                };
                let v = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let r = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let s = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;

                if cursor != bytes.len() || cursor != start_cursor + length as usize {
                    return Err(RlpError::InvalidFormat);
                }

                TransactionEnvelope::DynamicFee(TransactionDynamicFee {
                    chain_id,
                    nonce,
                    max_priority_fee_per_gas,
                    max_fee_per_gas,
                    gas_limit,
                    destination,
                    amount,
                    data,
                    access_list,
                    v,
                    r,
                    s,
                })
            }
            0x03 => {
                // TODO need to find blocks with this transaction type
                todo!();
                // cursor += 1;
                // let length = Rlp::decode_list(bytes, &mut cursor)?;
                // let start_cursor = cursor;
                // let chain_id = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let nonce = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let max_priority_fee_per_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let max_fee_per_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let gas_limit = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let to = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let value = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let data = Rlp::decode_string(bytes, &mut cursor)?.to_vec();
                // let access_list = {
                //     let length = Rlp::decode_list(bytes, &mut cursor)?;
                //     let access_list = AccessList::decode(&bytes[cursor..cursor + length as usize])?;
                //     cursor += length as usize;
                //     access_list
                // };
                // let max_fee_per_blob_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let blob_hashes = todo!();
                // let v = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let r = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                // let s = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                //
                // if cursor != bytes.len() || cursor != start_cursor + length as usize {
                //     return Err(RlpError::InvalidFormat);
                // }
                //
                // TransactionEnvelope::Blob(TransactionBlob {
                //     chain_id,
                //     nonce,
                //     max_priority_fee_per_gas,
                //     max_fee_per_gas,
                //     gas_limit,
                //     to,
                //     value,
                //     data,
                //     access_list,
                //     max_fee_per_blob_gas,
                //     blob_hashes,
                //     v,
                //     r,
                //     s,
                // })
            }
            _ => return Err(RlpError::InvalidFormat),
        };

        Ok(envelope)
    }
}

impl From<AccessList> for alloy::eips::eip2930::AccessList {
    fn from(AccessList(access_list): AccessList) -> Self {
        alloy::eips::eip2930::AccessList(
            access_list
                .into_iter()
                .map(|item| alloy::eips::eip2930::AccessListItem {
                    address: item.address.into(),
                    storage_keys: item
                        .storage_keys
                        .into_iter()
                        .map(|item| item.into())
                        .collect(),
                })
                .collect(),
        )
    }
}

impl From<Transaction> for alloy::rpc::types::Transaction {
    fn from(value: Transaction) -> Self {
        const NO_PARITY: [u8; 32] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 27,
        ];
        const PARITY: [u8; 32] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 28,
        ];
        const V_PARITY: [u8; 32] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 1,
        ];

        let transaction = match value {
            Transaction::Legacy { tx, .. } => {
                let (chain_id, y_parity) = {
                    if tx.v == NO_PARITY {
                        (None, false)
                    } else if tx.v == PARITY {
                        (None, true)
                    } else {
                        // v = yParity + 35 + 2 * chainId
                        // yParity = {0; 1}
                        // chainId = (v - yParity - 35) / 2
                        let v = alloy::primitives::U256::from_be_bytes(tx.v);
                        if v >= 37.try_into().unwrap() {
                            if tx.v[31] % 2 == 0 {
                                let sub: alloy::primitives::U256 =
                                    v - alloy::primitives::U256::from(35);
                                let chain_id: alloy::primitives::U256 =
                                    sub / alloy::primitives::U256::from(2);
                                (Some(chain_id.try_into().unwrap()), false)
                            } else {
                                let chain_id: alloy::primitives::U256 =
                                    v - alloy::primitives::U256::from(36);
                                (Some(chain_id.try_into().unwrap()), true)
                            }
                        } else if v == 1.try_into().unwrap() {
                            (None, true)
                        } else if v == 0.try_into().unwrap() {
                            (None, false)
                        } else {
                            // TODO: ?
                            (None, false)
                        }
                    }
                };

                alloy::consensus::TxEnvelope::Legacy(
                    alloy::consensus::TxLegacy {
                        chain_id,
                        nonce: u64::from_be_bytes(tx.nonce),
                        gas_price: u128::from_be_bytes(tx.gas_price[16..].try_into().unwrap()),
                        gas_limit: u64::from_be_bytes(tx.gas_limit),
                        to: if tx.to == [0; 20] {
                            alloy::primitives::TxKind::Create
                        } else {
                            alloy::primitives::TxKind::Call(tx.to.into())
                        },
                        value: alloy::primitives::U256::from_be_bytes(tx.value),
                        input: tx.data.into(),
                    }
                    .into_signed(alloy::signers::Signature::new(
                        alloy::primitives::U256::from_be_bytes(tx.r),
                        alloy::primitives::U256::from_be_bytes(tx.s),
                        y_parity,
                    )),
                )
            }
            Transaction::EIP2718 { envelope, .. } => match envelope {
                TransactionEnvelope::DynamicFee(TransactionDynamicFee {
                    chain_id,
                    nonce,
                    max_priority_fee_per_gas,
                    max_fee_per_gas,
                    gas_limit,
                    destination,
                    amount,
                    data,
                    access_list,
                    v,
                    r,
                    s,
                }) => alloy::consensus::TxEnvelope::Eip1559(
                    alloy::consensus::TxEip1559 {
                        chain_id: u64::from_be_bytes(chain_id[24..].try_into().unwrap()),
                        nonce: u64::from_be_bytes(nonce),
                        gas_limit: u64::from_be_bytes(gas_limit),
                        max_fee_per_gas: u128::from_be_bytes(
                            max_fee_per_gas[16..].try_into().unwrap(),
                        ),
                        max_priority_fee_per_gas: u128::from_be_bytes(
                            max_priority_fee_per_gas[16..].try_into().unwrap(),
                        ),
                        to: if destination == [0; 20] {
                            alloy::primitives::TxKind::Create
                        } else {
                            alloy::primitives::TxKind::Call(destination.into())
                        },
                        value: alloy::primitives::U256::from_be_bytes(amount),
                        access_list: access_list.into(),
                        input: data.into(),
                    }
                    .into_signed(alloy::signers::Signature::new(
                        alloy::primitives::U256::from_be_bytes(r),
                        alloy::primitives::U256::from_be_bytes(s),
                        v == V_PARITY,
                    )),
                ),
                TransactionEnvelope::AccessList(TransactionAccessList {
                    chain_id,
                    nonce,
                    gas_price,
                    gas_limit,
                    to,
                    value,
                    data,
                    access_list,
                    v,
                    r,
                    s,
                }) => alloy::consensus::TxEnvelope::Eip2930(
                    alloy::consensus::TxEip2930 {
                        chain_id: u64::from_be_bytes(chain_id[24..].try_into().unwrap()),
                        nonce: u64::from_be_bytes(nonce),
                        gas_price: u128::from_be_bytes(gas_price[16..].try_into().unwrap()),
                        gas_limit: u64::from_be_bytes(gas_limit),
                        to: if to == [0; 20] {
                            alloy::primitives::TxKind::Create
                        } else {
                            alloy::primitives::TxKind::Call(to.into())
                        },
                        value: alloy::primitives::U256::from_be_bytes(value),
                        access_list: access_list.into(),
                        input: data.into(),
                    }
                    .into_signed(alloy::signers::Signature::new(
                        alloy::primitives::U256::from_be_bytes(r),
                        alloy::primitives::U256::from_be_bytes(s),
                        v == V_PARITY,
                    )),
                ),
                TransactionEnvelope::Blob(TransactionBlob {
                    chain_id,
                    nonce,
                    max_priority_fee_per_gas,
                    max_fee_per_gas,
                    gas_limit,
                    to,
                    value,
                    data,
                    access_list,
                    max_fee_per_blob_gas,
                    blob_hashes,
                    v,
                    r,
                    s,
                }) => alloy::consensus::TxEnvelope::Eip4844(
                    alloy::consensus::TxEip4844Variant::TxEip4844(alloy::consensus::TxEip4844 {
                        chain_id: u64::from_be_bytes(chain_id[24..].try_into().unwrap()),
                        nonce: u64::from_be_bytes(nonce),
                        gas_limit: u64::from_be_bytes(gas_limit),
                        max_fee_per_gas: u128::from_be_bytes(
                            max_fee_per_gas[16..].try_into().unwrap(),
                        ),
                        max_priority_fee_per_gas: u128::from_be_bytes(
                            max_priority_fee_per_gas[16..].try_into().unwrap(),
                        ),
                        to: to.into(),
                        value: alloy::primitives::U256::from_be_bytes(value),
                        access_list: access_list.into(),
                        blob_versioned_hashes: blob_hashes.into_iter().map(|h| h.into()).collect(),
                        max_fee_per_blob_gas: u128::from_be_bytes(
                            max_fee_per_blob_gas[16..].try_into().unwrap(),
                        ),
                        input: data.into(),
                    })
                    .into_signed(alloy::signers::Signature::new(
                        alloy::primitives::U256::from_be_bytes(r),
                        alloy::primitives::U256::from_be_bytes(s),
                        v == V_PARITY,
                    )),
                ),
            },
        };

        Self {
            inner: alloy::consensus::transaction::Recovered::new_unchecked(
                transaction,
                Default::default(),
            ),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            effective_gas_price: None,
        }
    }
}

#[derive(Debug)]
pub struct TransactionLegacy {
    pub nonce: U64,
    pub gas_price: U256,
    pub gas_limit: U64,
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

#[derive(Debug)]
pub struct TransactionDynamicFee {
    pub chain_id: U256,
    pub nonce: U64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas_limit: U64,
    pub destination: Address,
    pub amount: U256,
    pub data: Vec<u8>,
    pub access_list: AccessList,
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

#[derive(Debug)]
pub struct TransactionAccessList {
    pub chain_id: U256,
    pub nonce: U64,
    pub gas_price: U256,
    pub gas_limit: U64,
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub access_list: AccessList,
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

#[derive(Debug)]
pub struct TransactionBlob {
    pub chain_id: U256,
    pub nonce: U64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas_limit: U64,
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub access_list: AccessList,
    pub max_fee_per_blob_gas: U256,
    pub blob_hashes: Vec<U256>,
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

#[derive(Debug)]
pub struct AccessList(Vec<AccessListItem>);

#[derive(Debug)]
pub struct AccessListItem {
    pub address: Address,
    pub storage_keys: Vec<U256>,
}

impl AccessList {
    pub fn decode(bytes: &[u8]) -> Result<AccessList, RlpError> {
        let mut cursor = 0;
        if bytes.is_empty() {
            Ok(AccessList(vec![]))
        } else {
            let length = Rlp::decode_list(bytes, &mut cursor)?;
            let start_cursor = cursor;
            let mut access_list = Vec::new();
            while cursor < start_cursor + length as usize {
                let address = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                let length = Rlp::decode_list(bytes, &mut cursor)?;
                let start_cursor = cursor;
                let mut storage_keys = Vec::new();
                while cursor < start_cursor + length as usize {
                    let storage_key = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                    storage_keys.push(storage_key);
                }
                if cursor != start_cursor + length as usize {
                    return Err(RlpError::InvalidFormat);
                }
                access_list.push(AccessListItem {
                    address,
                    storage_keys,
                });
            }
            if cursor != start_cursor + length as usize {
                return Err(RlpError::InvalidFormat);
            }
            Ok(AccessList(access_list))
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, RlpError> {
        let Self(access_list) = self;
        if access_list.is_empty() {
            return Ok(vec![]);
        }
        let mut buf = Vec::new();
        for item in &self.0 {
            Rlp::encode_normalized_string(&mut buf, &item.address)?;
            let mut storage_keys = Vec::new();
            for storage_key in &item.storage_keys {
                Rlp::encode_normalized_string(&mut storage_keys, storage_key)?;
            }
            Rlp::encode_list(&mut buf, &storage_keys)?;
        }
        let mut result = Vec::new();
        Rlp::encode_list(&mut result, &buf)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::unpacker::StatelessBlock;
    // use pretty_assertions::assert_eq;
    use std::fmt::Debug;
    use std::fs;

    const TEST_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/");

    fn can_parse<PARSEF, UNPARSEF, OK, ERR>(dir: &str, parse: PARSEF, unparse: UNPARSEF)
    where
        PARSEF: Fn(&[u8]) -> Result<OK, ERR>,
        UNPARSEF: Fn(&OK) -> Result<Vec<u8>, ERR>,
        ERR: Debug,
    {
        let test_dir = TEST_DIR.to_owned() + dir;
        for entry in fs::read_dir(test_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() && path.extension().unwrap() == "bin" {
                let file_name = path.file_name().unwrap().to_str().unwrap();
                println!("{}....", file_name);
                let bytes = fs::read(&path).unwrap();
                assert!(!bytes.is_empty());

                let res = parse(&bytes).unwrap();
                let ret_bytes = unparse(&res).unwrap();
                assert_eq!(ret_bytes, bytes);
                println!("ok!");
            } else {
                panic!("invalid path {:?}", path);
            }
        }
    }

    #[test]
    fn can_parse_block() {
        can_parse("blocks", StatelessBlock::unpack, StatelessBlock::pack);
    }

    // #[test]
    // fn can_parse_transaction() {
    //     can_parse("txs", Transaction::decode);
    // }
}
