mod block;
mod kademlia;

use crate::id::NodeId;
use ruint::Uint;
use std::cmp::Ordering;

pub trait ConcreteDht<NUM> {
    fn from_node_id(node_id: NodeId, k: NUM) -> Self;
    fn is_desired_bucket(&self, bucket: NUM) -> bool;
}

/// BITS is the numbers of bits that encode all buckets.
/// B_SIZE is the size in bytes of buckets (BITS == B_SIZE * 8).
/// LIMBS is the limbs of the internal uint (LIMBS == ceil(BITS / 64))
/// OFFSET is the byte offset to derive the bucket from the node ID.
pub struct Dht<const BITS: usize, const B_SIZE: usize, const LIMBS: usize, const OFFSET: usize> {
    bucket_lo: Uint<BITS, LIMBS>,
    bucket_hi: Uint<BITS, LIMBS>,
}

impl<const BITS: usize, const B_SIZE: usize, const LIMBS: usize, const OFFSET: usize>
    Dht<BITS, B_SIZE, LIMBS, OFFSET>
{
    fn _from_node_id(node_id: NodeId, k: Uint<BITS, LIMBS>) -> Self {
        assert!(B_SIZE > 0);
        assert_eq!(LIMBS, BITS.div_ceil(64));
        // cannot use generic from outer item so this is not a compile-time check.
        assert_eq!(BITS, B_SIZE * 8);
        assert!(OFFSET + B_SIZE <= 20);
        let arr: [u8; 20] = node_id.into();
        let be_bytes: [u8; B_SIZE] = arr[OFFSET..OFFSET + B_SIZE].try_into().unwrap();
        let bucket = Uint::from_be_bytes(be_bytes);
        let (bucket_lo, bucket_hi) = (bucket.wrapping_sub(k), bucket.wrapping_add(k));
        Self {
            bucket_lo,
            bucket_hi,
        }
    }

    /// Range of buckets of this node. The left hand is included and the right hand is excluded.
    fn bucket_range(&self) -> (Uint<BITS, LIMBS>, Uint<BITS, LIMBS>) {
        (self.bucket_lo, self.bucket_hi)
    }

    fn _is_desired_bucket(&self, bucket: Uint<BITS, LIMBS>) -> bool {
        let (bucket_lo, bucket_hi) = self.bucket_range();
        match bucket_lo.cmp(&bucket_hi) {
            Ordering::Less => bucket_lo <= bucket && bucket < bucket_hi,
            Ordering::Equal => bucket_lo == bucket,
            Ordering::Greater => bucket_lo <= bucket || bucket < bucket_hi,
        }
    }
}

pub trait Task {
    async fn run(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dht_buckets() {
        type MyDht = Dht<16, 2, 1, 0>;
        
        let dht = MyDht::from_node_id(NodeId::default(), 8);
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, Uint::from_be_bytes([255, 248]));
        assert_eq!(hi, Uint::from_be_bytes([0, 8]));
        assert!(dht.is_desired_bucket(0));
        assert!(dht.is_desired_bucket(65528));
        assert!(!dht.is_desired_bucket(8));
        assert!(dht.is_desired_bucket(65535));
        assert!(!dht.is_desired_bucket(16));

        let dht = MyDht::from_node_id(NodeId::default(), 0);
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, Uint::from_be_bytes([0, 0]));
        assert_eq!(lo, hi);
        assert!(dht.is_desired_bucket(0));
        assert!(!dht.is_desired_bucket(65535));
        assert!(!dht.is_desired_bucket(1));

        let dht = MyDht::from_node_id(
            NodeId::from([0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            8,
        );
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, Uint::from_be_bytes([0, 0]));
        assert_eq!(hi, Uint::from_be_bytes([0, 16]));
        assert!(dht.is_desired_bucket(0));
        assert!(dht.is_desired_bucket(8));
        assert!(!dht.is_desired_bucket(16));
        assert!(dht.is_desired_bucket(15));
    }
}
