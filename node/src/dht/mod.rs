pub mod block;
pub mod kademlia;

use crate::dht::kademlia::{KademliaDht, LockedMapDb};
use crate::id::NodeId;
use ruint::Uint;
use std::cmp::Ordering;

pub type Bucket = Uint<160, 3>;

pub trait ConcreteDht<K> {
    fn key_to_bucket(val: K) -> Bucket;
}

#[derive(Debug)]
pub struct Dht<K, DB: LockedMapDb<Bucket, Vec<u8>>> {
    bucket_lo: Bucket,
    bucket_hi: Bucket,
    kademlia_dht: KademliaDht<Vec<u8>, DB>,
    _marker: std::marker::PhantomData<K>,
}

impl<K, DB: LockedMapDb<Bucket, Vec<u8>>> Dht<K, DB> {
    pub(crate) fn new(node_id: NodeId, k: Bucket, kademlia_dht: KademliaDht<Vec<u8>, DB>) -> Self {
        let arr: [u8; 20] = node_id.into();
        let bucket = Bucket::from_be_bytes(arr);
        let (bucket_lo, bucket_hi) = (bucket.wrapping_sub(k), bucket.wrapping_add(k));
        Self {
            bucket_lo,
            bucket_hi,
            kademlia_dht,
            _marker: std::marker::PhantomData,
        }
    }

    /// Range of buckets of this node. The left hand is included and the right hand is excluded.
    fn bucket_range(&self) -> (&Bucket, &Bucket) {
        (&self.bucket_lo, &self.bucket_hi)
    }

    fn is_desired_bucket(&self, bucket: &Bucket) -> bool {
        let (bucket_lo, bucket_hi) = self.bucket_range();
        match bucket_lo.cmp(bucket_hi) {
            Ordering::Less => bucket_lo <= bucket && bucket < bucket_hi,
            Ordering::Equal => bucket_lo == bucket,
            Ordering::Greater => bucket_lo <= bucket || bucket < bucket_hi,
        }
    }
}

pub enum LightMessage {
    Store(Vec<u8>),
    FindNode(Bucket, usize),
    FindValue(Bucket, usize),
}

pub trait Task {
    async fn process_message(&mut self, message: &LightMessage);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::RwLock;

    #[test]
    fn dht_buckets() {
        type MyDht = Dht<Bucket, RwLock<HashMap<Bucket, Vec<u8>>>>;

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
