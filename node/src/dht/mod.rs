pub mod block;
pub mod kademlia;

use crate::dht::kademlia::{KademliaDht, LockedMapDb, ValueOrNodes};
use crate::id::NodeId;
use ruint::Uint;
use serde::Deserialize;
use std::cmp::Ordering;

#[derive(Debug, Clone, Eq, Deserialize, PartialEq)]
pub struct DhtBuckets {
    pub block: Bucket,
}

pub type Bucket = Uint<160, 3>;

pub trait ConcreteDht<K> {
    fn key_to_bucket(val: K) -> Bucket;
}

#[derive(Debug)]
pub struct Dht<DB: LockedMapDb<Bucket, Vec<u8>>> {
    pub k: Bucket,
    bucket_lo: Bucket,
    bucket_hi: Bucket,
    pub kademlia_dht: KademliaDht<Vec<u8>, DB>,
}

impl<DB: LockedMapDb<Bucket, Vec<u8>>> Dht<DB> {
    pub(crate) fn new(node_id: NodeId, k: Bucket, kademlia_dht: KademliaDht<Vec<u8>, DB>) -> Self {
        let arr: [u8; 20] = node_id.into();
        let bucket = Bucket::from_be_bytes(arr);
        let (bucket_lo, bucket_hi) = (bucket.wrapping_sub(k), bucket.wrapping_add(k));
        Self {
            k,
            bucket_lo,
            bucket_hi,
            kademlia_dht,
        }
    }

    /// Range of buckets for this node. The left hand is included and the right hand is excluded.
    fn bucket_range(&self) -> (&Bucket, &Bucket) {
        (&self.bucket_lo, &self.bucket_hi)
    }

    pub fn is_desired_bucket(&self, bucket: &Bucket) -> bool {
        let (bucket_lo, bucket_hi) = self.bucket_range();
        match bucket_lo.cmp(bucket_hi) {
            Ordering::Less => bucket_lo <= bucket && bucket < bucket_hi,
            Ordering::Equal => bucket_lo == bucket,
            Ordering::Greater => bucket_lo <= bucket || bucket < bucket_hi,
        }
    }
}

pub enum DhtId {
    Block,
    State,
}

impl From<u32> for DhtId {
    fn from(val: u32) -> Self {
        match val {
            0 => Self::Block,
            _ => unimplemented!(),
        }
    }
}

pub enum LightMessage {
    Store(DhtId, Vec<u8>),
    FindNode(DhtId, Bucket),
    FindValue(DhtId, Bucket),
}

pub struct LightError {
    pub code: i32,
    pub message: String,
}

pub type LightResult = Result<Option<ValueOrNodes<Vec<u8>>>, LightError>;

pub trait Task {
    fn store(&self, value: Vec<u8>) -> LightResult;
    fn find_node(&self, bucket: Bucket) -> LightResult;
    fn find_value(&self, bucket: Bucket) -> LightResult;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::RwLock;

    #[test]
    fn dht_buckets() {
        type MyDht = Dht<RwLock<HashMap<Bucket, Vec<u8>>>>;

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

        let dht = MyDht::new(NodeId::default(), num_to_bucket(8), Default::default());
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, &Bucket::from_be_bytes(bytes_with_last(0xff, 0xf8)));
        assert_eq!(hi, &Bucket::from_be_bytes(bytes_with_last(0, 8)));
        assert!(dht.is_desired_bucket(&num_to_bucket(0)));
        assert!(dht.is_desired_bucket(lo));
        assert!(!dht.is_desired_bucket(&num_to_bucket(8)));
        assert!(dht.is_desired_bucket(&Bucket::from_be_bytes([0xff; 20])));
        assert!(!dht.is_desired_bucket(&num_to_bucket(16)));

        let dht = MyDht::new(NodeId::default(), Bucket::ZERO, Default::default());
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, &Bucket::from_be_bytes([0; 20]));
        assert_eq!(lo, hi);
        assert!(dht.is_desired_bucket(&num_to_bucket(0)));
        assert!(!dht.is_desired_bucket(&Bucket::from_be_bytes([0xff; 20])));
        assert!(!dht.is_desired_bucket(&num_to_bucket(1)));

        let dht = MyDht::new(
            NodeId::from(bytes_with_last(0, 8)),
            num_to_bucket(8),
            Default::default(),
        );
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, &Bucket::from_be_bytes([0; 20]));
        assert_eq!(hi, &Bucket::from_be_bytes(bytes_with_last(0, 16)));
        assert!(dht.is_desired_bucket(&num_to_bucket(0)));
        assert!(dht.is_desired_bucket(&num_to_bucket(8)));
        assert!(!dht.is_desired_bucket(&num_to_bucket(16)));
        assert!(dht.is_desired_bucket(&num_to_bucket(15)));
    }
}
