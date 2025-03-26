use rlp_types::TransactionEnvelope;
use serde::{Serialize, Serializer};

#[derive(Debug, Serialize)]
pub struct AccessList {
    address: [u8; 20],
    slots: Vec<Vec<u8>>,
}

// TODO blob transactions
#[derive(Debug, Serialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum Transaction {
    Legacy {
        #[serde(serialize_with = "compact_hex_u64")]
        nonce: u64,
        #[serde(serialize_with = "compact_hex_u256")]
        gas_price: [u8; 32],
        #[serde(serialize_with = "compact_hex_u64")]
        #[serde(rename = "gas")]
        gas_limit: u64,
        #[serde(serialize_with = "hex_address")]
        to: [u8; 20],
        #[serde(serialize_with = "compact_hex_u256")]
        value: [u8; 32],
        #[serde(serialize_with = "compact_hex_bytes")]
        #[serde(rename = "input")]
        data: Vec<u8>,
        #[serde(serialize_with = "compact_hex_u256")]
        v: [u8; 32],
        #[serde(serialize_with = "compact_hex_u256")]
        r: [u8; 32],
        #[serde(serialize_with = "compact_hex_u256")]
        s: [u8; 32],
    },
    AccessList {
        #[serde(serialize_with = "compact_hex_u256")]
        chain_id: [u8; 32],
        #[serde(serialize_with = "compact_hex_u64")]
        nonce: u64,
        #[serde(serialize_with = "compact_hex_u256")]
        gas_price: [u8; 32],
        #[serde(serialize_with = "compact_hex_u64")]
        #[serde(rename = "gas")]
        gas_limit: u64,
        #[serde(serialize_with = "hex_address")]
        to: [u8; 20],
        #[serde(serialize_with = "compact_hex_u256")]
        value: [u8; 32],
        #[serde(serialize_with = "compact_hex_bytes")]
        #[serde(rename = "input")]
        data: Vec<u8>,
        access_list: Vec<AccessList>, // TODO
        #[serde(serialize_with = "compact_hex_u256")]
        v: [u8; 32],
        #[serde(serialize_with = "compact_hex_u256")]
        r: [u8; 32],
        #[serde(serialize_with = "compact_hex_u256")]
        s: [u8; 32],
    },
    DynamicFee {
        #[serde(serialize_with = "compact_hex_u256")]
        chain_id: [u8; 32],
        #[serde(serialize_with = "compact_hex_u64")]
        nonce: u64,
        #[serde(serialize_with = "compact_hex_u256")]
        max_priority_fee_per_gas: [u8; 32],
        #[serde(serialize_with = "compact_hex_u256")]
        max_fee_per_gas: [u8; 32],
        #[serde(serialize_with = "compact_hex_u64")]
        #[serde(rename = "gas")]
        gas_limit: u64,
        #[serde(serialize_with = "hex_address")]
        #[serde(rename = "to")]
        destination: [u8; 20],
        #[serde(serialize_with = "compact_hex_u256")]
        #[serde(rename = "value")]
        amount: [u8; 32],
        #[serde(serialize_with = "compact_hex_bytes")]
        #[serde(rename = "input")]
        data: Vec<u8>,
        access_list: Vec<AccessList>, // TODO
        #[serde(serialize_with = "compact_hex_u256")]
        v: [u8; 32],
        #[serde(serialize_with = "compact_hex_u256")]
        r: [u8; 32],
        #[serde(serialize_with = "compact_hex_u256")]
        s: [u8; 32],
    },
}

impl From<TransactionEnvelope> for Transaction {
    fn from(value: TransactionEnvelope) -> Self {
        match value {
            TransactionEnvelope::Legacy(legacy) => Transaction::Legacy {
                nonce: legacy.nonce,
                gas_price: legacy.gas_price.into(),
                gas_limit: legacy.gas_limit,
                to: legacy.to.into(),
                value: legacy.value.into(),
                data: legacy.data,
                v: legacy.v.into(),
                r: legacy.r.into(),
                s: legacy.s.into(),
            },
            TransactionEnvelope::AccessList(access_list) => Transaction::AccessList {
                chain_id: access_list.chain_id.into(),
                nonce: access_list.nonce,
                gas_price: access_list.gas_price.into(),
                gas_limit: access_list.gas_limit,
                to: access_list.to.into(),
                value: access_list.value.into(),
                data: access_list.data,
                // access_list: access_list.access_list.into(),
                access_list: vec![],
                // v: access_list.y_parity.into(), // incorrect but I dgaf
                v: Default::default(), // incorrect but I dgaf
                r: access_list.r.into(),
                s: access_list.s.into(),
            },
            TransactionEnvelope::DynamicFee(dynamic_fees) => Transaction::DynamicFee {
                chain_id: dynamic_fees.chain_id.into(),
                nonce: dynamic_fees.nonce,
                max_priority_fee_per_gas: dynamic_fees.max_priority_fee_per_gas.into(),
                max_fee_per_gas: dynamic_fees.max_fee_per_gas.into(),
                gas_limit: dynamic_fees.gas_limit,
                destination: dynamic_fees.destination.into(),
                amount: dynamic_fees.amount.into(),
                data: dynamic_fees.data,
                access_list: vec![],
                // v: dynamic_fees.y_parity.into(), // incorrect but I dgaf
                v: Default::default(), // incorrect but I dgaf
                r: dynamic_fees.r.into(),
                s: dynamic_fees.s.into(),
            },
            _ => unimplemented!(),
        }
    }
}

fn first_pos<const L: usize>(arr: &[u8; L]) -> usize {
    arr.iter()
        .position(|b| b > &0)
        .unwrap_or(L.saturating_sub(1))
}

fn first_pos_slice(slice: &[u8]) -> usize {
    slice
        .iter()
        .position(|b| b > &0)
        .unwrap_or(slice.len().saturating_sub(1))
}

fn compact_hex<S, const L: usize>(arr: &[u8; L], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let index = first_pos(arr);
    as_hex(arr, index, serializer)
}

fn as_hex<S, const L: usize>(arr: &[u8; L], index: usize, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut as_string = String::from("0x");
    as_string.push_str(&hex::encode(&arr[index..]));
    serializer.serialize_str(&as_string)
}

fn compact_hex_u64<S>(t: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    compact_hex(&t.to_be_bytes(), serializer)
}

fn hex_address<S>(t: &[u8; 20], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    as_hex(t, 0, serializer)
}

fn compact_hex_u256<S>(t: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    compact_hex(t, serializer)
}

fn compact_hex_bytes<S>(t: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let index = first_pos_slice(t);
    let mut as_string = String::from("0x");
    as_string.push_str(&hex::encode(&t[index..]));
    serializer.serialize_str(&as_string)
}
