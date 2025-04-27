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
}

impl Block {
    pub fn decode(bytes: &[u8]) -> Result<Self, RlpError> {
        let mut cursor = 0;
        // let start_list = cursor;
        let _length = Rlp::decode_list(bytes, &mut cursor)?;
        let (header, hash) = {
            let start_list = cursor;
            let length = Rlp::decode_list(bytes, &mut cursor)?;
            let hash = keccak256(&bytes[start_list..cursor + length as usize]);
            let header = Self::decode_header(&bytes[cursor..cursor + length as usize])?;
            cursor += length as usize;
            (header, hash)
        };
        let transactions = {
            let length = Rlp::decode_list(bytes, &mut cursor)?;
            let transactions = Self::decode_transactions(&bytes[cursor..cursor + length as usize])?;
            // cursor += length as usize;
            #[allow(clippy::let_and_return)]
            transactions
        };
        // TODO decode uncles
        // dbg!(&bytes[cursor..]);
        // if cursor != start_list + length as usize {
        //     return Err(RlpError::InvalidFormat);
        // }

        let block = Block {
            hash,
            size: bytes.len(),
            header,
            transactions,
            uncles: vec![],
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

        let header = if cursor == bytes.len() {
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
        } else {
            let base_fee = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
            if cursor == bytes.len() {
                Header::EIP1559 {
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
                    base_fee,
                }
            } else {
                let withdrawal_root = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                if cursor == bytes.len() {
                    Header::EIP4895 {
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
                        base_fee,
                        withdrawal_root,
                    }
                } else {
                    let ext_data_gas_used = withdrawal_root;
                    if cursor == bytes.len() {
                        let block_gas_cost = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                        Header::Apricot4 {
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
                            base_fee,
                            ext_data_gas_used,
                            block_gas_cost,
                        }
                    } else {
                        let blob_gas_used = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                        let excess_blob_gas = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                        if cursor == bytes.len() {
                            Header::EIP4844 {
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
                                base_fee,
                                withdrawal_root,
                                blob_gas_used,
                                excess_blob_gas,
                            }
                        } else {
                            let parent_beacon_block_root =
                                Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                            if cursor == bytes.len() {
                                Header::EIP4788 {
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
                                    base_fee,
                                    withdrawal_root,
                                    blob_gas_used,
                                    excess_blob_gas,
                                    parent_beacon_block_root,
                                }
                            } else {
                                let ext_data_hash = Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                                let ext_data_gas_used =
                                    Rlp::decode_fixed_bytes(bytes, &mut cursor)?;
                                if cursor != bytes.len() {
                                    return Err(RlpError::InvalidFormat);
                                }
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
                                    base_fee,
                                    withdrawal_root,
                                    blob_gas_used,
                                    excess_blob_gas,
                                    parent_beacon_block_root,
                                    ext_data_hash,
                                    ext_data_gas_used,
                                }
                            }
                        }
                    }
                }
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
        base_fee: U256,
        withdrawal_root: U256,
        blob_gas_used: U64,
        excess_blob_gas: U64,
        parent_beacon_block_root: B32,
        ext_data_hash: B32,
        ext_data_gas_used: U256,
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
            dbg!(&bytes[cursor..]);
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
}

#[cfg(test)]
mod tests {
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

    // #[test]
    // fn can_parse_transaction() {
    //     can_parse("txs", Transaction::decode);
    // }
}
