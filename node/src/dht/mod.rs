mod block;
mod kademlia;

use crate::id::NodeId;
use ruint::Uint;
use std::cmp::Ordering;

pub type Bucket = Uint<160, 3>;

pub trait ConcreteDht<IN> {
    fn from_node_id(node_id: NodeId, k: Bucket) -> Self;
    fn is_desired_bucket(&self, bucket: Bucket) -> bool;
    fn in_to_bucket(val: IN) -> Bucket;
}

/// BITS is the numbers of bits that encode all buckets.
/// B_SIZE is the size in bytes of buckets (BITS == B_SIZE * 8).
/// LIMBS is the limbs of the internal uint (LIMBS == ceil(BITS / 64))
/// OFFSET is the byte offset to derive the bucket from the node ID.
pub struct Dht<IN> {
    bucket_lo: Bucket,
    bucket_hi: Bucket,
    _marker: std::marker::PhantomData<IN>,
}

impl<IN> Dht<IN> {
    fn _from_node_id(node_id: NodeId, k: Bucket) -> Self {
        let arr: [u8; 20] = node_id.into();
        let bucket = Bucket::from_be_bytes(arr);
        let (bucket_lo, bucket_hi) = (bucket.wrapping_sub(k), bucket.wrapping_add(k));
        Self {
            bucket_lo,
            bucket_hi,
            _marker: std::marker::PhantomData,
        }
    }

    /// Range of buckets of this node. The left hand is included and the right hand is excluded.
    fn bucket_range(&self) -> (Bucket, Bucket) {
        (self.bucket_lo, self.bucket_hi)
    }

    fn _is_desired_bucket(&self, bucket: Bucket) -> bool {
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
        type MyDht = Dht<()>;

        let num_to_bucket = |num: u16| -> Bucket {
            let mut arr: [u8; 20] = [0; 20];
            arr[18..].copy_from_slice(&num.to_be_bytes());
            Bucket::from_be_bytes(arr)
        };

        let dht = MyDht::_from_node_id(NodeId::default(), num_to_bucket(8));
        let (lo, hi) = dht.bucket_range();
        assert_eq!(
            lo,
            Bucket::from_be_bytes([
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xf8
            ])
        );
        assert_eq!(
            hi,
            Bucket::from_be_bytes({
                let mut arr: [u8; 20] = [0; 20];
                arr[19] = 8;
                arr
            })
        );
        assert!(dht._is_desired_bucket(num_to_bucket(0)));
        assert!(dht._is_desired_bucket(lo));
        assert!(!dht._is_desired_bucket(num_to_bucket(8)));
        assert!(dht._is_desired_bucket(Bucket::from_be_bytes([0xff; 20])));
        assert!(!dht._is_desired_bucket(num_to_bucket(16)));

        let dht = MyDht::_from_node_id(NodeId::default(), Bucket::ZERO);
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, Bucket::from_be_bytes([0; 20]));
        assert_eq!(lo, hi);
        assert!(dht._is_desired_bucket(num_to_bucket(0)));
        assert!(!dht._is_desired_bucket(Bucket::from_be_bytes([0xff; 20])));
        assert!(!dht._is_desired_bucket(num_to_bucket(1)));

        let dht = MyDht::_from_node_id(
            NodeId::from([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8]),
            num_to_bucket(8),
        );
        let (lo, hi) = dht.bucket_range();
        assert_eq!(lo, Bucket::from_be_bytes([0; 20]));
        assert_eq!(
            hi,
            Bucket::from_be_bytes({
                let mut arr: [u8; 20] = [0; 20];
                arr[19] = 16;
                arr
            })
        );
        assert!(dht._is_desired_bucket(num_to_bucket(0)));
        assert!(dht._is_desired_bucket(num_to_bucket(8)));
        assert!(!dht._is_desired_bucket(num_to_bucket(16)));
        assert!(dht._is_desired_bucket(num_to_bucket(15)));
    }
}
