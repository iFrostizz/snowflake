pub mod block;
pub mod kademlia;

use crate::dht::kademlia::{LockedMapDb, ValueOrNodes};
use crate::id::NodeId;
use crate::net::queue::ConnectionData;
use jsonrpsee::types::ErrorObject;
use proto_lib::sdk;
use ruint::Uint;
use serde::Deserialize;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Eq, Deserialize, PartialEq)]
pub struct DhtBuckets {
    pub block: Bucket,
}

impl From<&DhtBuckets> for sdk::DhtBuckets {
    fn from(buckets: &DhtBuckets) -> Self {
        sdk::DhtBuckets {
            block: buckets.block.to_be_bytes_vec(),
        }
    }
}

pub type Bucket = Uint<160, 3>;

pub trait ConcreteDht {
    fn to_bucket(&self) -> Bucket;
}

#[derive(Debug)]
struct BucketDht {
    k: Bucket,
    bucket_lo: Bucket,
    bucket_hi: Bucket,
}

impl BucketDht {
    pub(crate) fn new(node_id: NodeId, k: Bucket) -> Self {
        let bucket = Bucket::from_be_bytes(node_id.into());
        let two: Bucket = 2.try_into().unwrap();
        let (bucket_lo, bucket_hi) = if k == Bucket::MAX {
            (bucket.wrapping_sub(k / two), bucket.wrapping_sub(k / two))
        } else {
            (bucket.wrapping_sub(k / two), bucket.wrapping_add(k / two))
        };
        Self {
            k,
            bucket_lo,
            bucket_hi,
        }
    }

    /// Range of buckets for this node. The left hand is included and the right hand is excluded.
    fn bucket_range(&self) -> (&Bucket, &Bucket) {
        (&self.bucket_lo, &self.bucket_hi)
    }

    pub fn is_desired_bucket(&self, bucket: &Bucket) -> bool {
        if self.k == Bucket::ZERO {
            return false;
        }
        let (bucket_lo, bucket_hi) = self.bucket_range();
        match bucket_lo.cmp(bucket_hi) {
            Ordering::Less => bucket_lo <= bucket && bucket <= bucket_hi,
            Ordering::Equal => true,
            Ordering::Greater => bucket_lo <= bucket || bucket <= bucket_hi,
        }
    }
}

#[derive(Debug)]
pub struct Dht<DB: LockedMapDb<Vec<u8>>> {
    bucket_dht: BucketDht,
    pub store: DB,
}

impl<DB: LockedMapDb<Vec<u8>>> Dht<DB> {
    pub(crate) fn new(node_id: NodeId, k: Bucket, store: DB) -> Self {
        let bucket_dht = BucketDht::new(node_id, k);
        Self { bucket_dht, store }
    }

    pub fn is_desired_bucket(&self, bucket: &Bucket) -> bool {
        self.bucket_dht.is_desired_bucket(bucket)
    }
}

#[derive(Debug, Deserialize, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum DhtId {
    Block,
    State,
}

impl TryFrom<u32> for DhtId {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Block),
            _ => Err(()),
        }
    }
}

impl From<&DhtId> for u32 {
    fn from(value: &DhtId) -> Self {
        match value {
            DhtId::Block => 0,
            _ => unimplemented!(),
        }
    }
}

impl From<DhtId> for u32 {
    fn from(value: DhtId) -> Self {
        (&value).into()
    }
}

#[derive(Debug)]
pub enum LightMessage {
    NewPeer(DhtBuckets),
    Store(DhtId, Vec<u8>),
    FindNode(Bucket),
    FindValue(DhtId, Bucket),
    Nodes(Vec<ConnectionData>),
}

#[derive(Debug)]
pub struct LightError {
    pub code: i32,
    pub message: &'static str,
}

impl Display for LightError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub mod light_errors {
    use super::LightError;

    pub(crate) const CONTENT_NOT_FOUND: LightError = LightError {
        code: 1,
        message: "Content not found",
    };
    #[allow(unused)]
    pub(crate) const ENCODING_FAILED: LightError = LightError {
        code: 2,
        message: "Invalid encoded value",
    };
    pub(crate) const DECODING_FAILED: LightError = LightError {
        code: 3,
        message: "Invalid encoded value",
    };
    pub(crate) const UNDESIRED_BUCKET: LightError = LightError {
        code: 4,
        message: "This bucket is not desired",
    };
    pub(crate) const INVALID_DHT: LightError = LightError {
        code: 5,
        message: "Unimplemented dht",
    };
    pub(crate) const INVALID_CONTENT: LightError = LightError {
        code: 6,
        message: "Content failed verification",
    };
    pub(crate) const PEER_MISSING: LightError = LightError {
        code: 7,
        message: "The peer is missing",
    };
    pub(crate) const SEND_TO_SELF: LightError = LightError {
        code: 8,
        message: "Cannot send message to self",
    };
}

impl From<LightError> for jsonrpsee::types::ErrorObjectOwned {
    fn from(value: LightError) -> Self {
        ErrorObject::borrowed(value.code, value.message, None)
    }
}

#[derive(Debug)]
pub enum LightValue<T = Vec<u8>> {
    Ok,
    ValueOrNodes(ValueOrNodes<T>),
}

pub type LightResult<T = Vec<u8>> = Result<LightValue<T>, LightError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dht_buckets() {
        let num_to_bucket = |num: u16| -> Bucket {
            let mut arr: [u8; 20] = [0; 20];
            arr[18..].copy_from_slice(&num.to_be_bytes());
            Bucket::from_be_bytes(arr)
        };

        let bytes_with_last = |firsts: u8, last: u8| -> [u8; 20] {
            let mut arr: [u8; 20] = [firsts; 20];
            arr[19] = last;
            arr
        };

        let dht = BucketDht::new(NodeId::default(), num_to_bucket(16));
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, &Bucket::from_be_bytes(bytes_with_last(0xff, 0xf8)));
        assert_eq!(hi, &Bucket::from_be_bytes(bytes_with_last(0, 8)));
        assert!(dht.is_desired_bucket(&num_to_bucket(0)));
        assert!(dht.is_desired_bucket(lo));
        assert!(dht.is_desired_bucket(&num_to_bucket(8)));
        assert!(!dht.is_desired_bucket(&num_to_bucket(9)));
        assert!(dht.is_desired_bucket(&Bucket::from_be_bytes([0xff; 20])));
        assert!(!dht.is_desired_bucket(&num_to_bucket(16)));

        let dht = BucketDht::new(NodeId::default(), Bucket::ZERO);
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, &Bucket::from_be_bytes([0; 20]));
        assert_eq!(lo, hi);
        assert!(!dht.is_desired_bucket(&num_to_bucket(0)));
        assert!(!dht.is_desired_bucket(&Bucket::from_be_bytes([0xff; 20])));
        assert!(!dht.is_desired_bucket(&num_to_bucket(1)));

        let dht = BucketDht::new(NodeId::from(bytes_with_last(0, 8)), num_to_bucket(16));
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, &Bucket::from_be_bytes([0; 20]));
        assert_eq!(hi, &Bucket::from_be_bytes(bytes_with_last(0, 16)));
        assert!(dht.is_desired_bucket(&num_to_bucket(0)));
        assert!(dht.is_desired_bucket(&num_to_bucket(8)));
        assert!(!dht.is_desired_bucket(&num_to_bucket(17)));
        assert!(dht.is_desired_bucket(&num_to_bucket(16)));
    }
}
