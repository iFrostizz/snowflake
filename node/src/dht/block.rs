use crate::dht::kademlia::LockedMapDb;
use crate::dht::{light_errors, Bucket, DhtId, LightError};
use crate::dht::{ConcreteDht, Dht};
use crate::id::{BlockID, NodeId};
use crate::net::light::{DhtCodex, DhtContent};
use crate::utils::twokhashmap::{CompositeKey, DoubleKeyedHashMap};
use crate::utils::unpacker::StatelessBlock;
use crate::utils::FIFOSet;
use alloy::primitives::{keccak256, FixedBytes};
use alloy::primitives::{B256, U256};
use std::cmp::Ordering;
use std::ops::RangeInclusive;
use std::sync::{Mutex, RwLock};

#[derive(Debug)]
pub struct DhtBlocks {
    pub dht: Dht<RwLock<DoubleKeyedHashMap<Bucket, Bucket, Vec<u8>>>>,
    pub verified_blocks: RwLock<FIFOSet<BlockID>>,
    pub min_stored_blocks: Mutex<u64>,
}

impl DhtCodex<StatelessBlock> for DhtBlocks {
    fn id() -> DhtId {
        DhtId::Block
    }

    fn encode(value: StatelessBlock) -> Result<Vec<u8>, LightError> {
        // value.pack().map_err(|_| light_errors::ENCODING_FAILED)
        Ok(value.bytes().to_owned())
    }

    fn decode(bytes: &[u8]) -> Result<StatelessBlock, LightError> {
        StatelessBlock::unpack(bytes.to_owned()).map_err(|_| light_errors::DECODING_FAILED)
    }
}

impl ConcreteDht for FixedBytes<32> {
    fn to_bucket(&self) -> Bucket {
        let arr: [u8; 20] = keccak256(self)[0..20].try_into().unwrap();
        <Bucket>::from_be_bytes(arr)
    }
}

const NUM_BUCKETS: u64 = 10_000_000_000_000_000;

impl ConcreteDht for u64 {
    fn to_bucket(&self) -> Bucket {
        // n % N * (2**160-1) / N
        let block_number = *self;
        let bucket_index = block_number % NUM_BUCKETS;

        let scaled: U256 =
            U256::from(bucket_index) * (U256::from(Bucket::MAX) / U256::from(NUM_BUCKETS));

        let scaled_bytes: [u8; 32] = scaled.to_be_bytes();
        debug_assert!(scaled_bytes[0..12] == [0u8; 12]);
        let arr: [u8; 20] = scaled_bytes[12..32].try_into().unwrap();
        Bucket::from_be_bytes(arr)
    }
}

impl DhtContent<CompositeKey<u64, FixedBytes<32>>, StatelessBlock> for DhtBlocks {
    fn verify(&self, block: &StatelessBlock) -> Result<bool, LightError> {
        let block_id = *block.id();
        let block = &block.block;
        let number = u64::from_be_bytes(*block.header.number());
        let hash = block.hash();
        if self
            .get_from_store(CompositeKey::Both(number, hash))?
            .is_some()
        {
            return Ok(true);
        }
        Ok(self.verified_blocks.read().unwrap().contains(&block_id))
    }

    fn get_from_store(
        &self,
        key: CompositeKey<u64, FixedBytes<32>>,
    ) -> Result<Option<StatelessBlock>, LightError> {
        match self.dht.store.get(&key) {
            Some(block_bytes) => {
                let block = Self::decode(&block_bytes)?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    fn insert_to_store(&self, bytes: Vec<u8>) -> Result<(), LightError> {
        let decoded = Self::decode(&bytes)?;
        let block = &decoded.block;
        let number = u64::from_be_bytes(*block.header.number());
        let hash = block.hash();
        if self.is_desired_bucket(number, hash) {
            if !self.verify(&decoded)? {
                return Err(light_errors::INVALID_CONTENT);
            }
            if self
                .get_from_store(CompositeKey::Both(number, hash))?
                .is_none()
            {
                let n = self.next_block_to_store()?;
                if n == number {
                    *self.min_stored_blocks.lock().unwrap() += 1;
                }
                self.dht
                    .store
                    .insert(CompositeKey::Both(number, hash), bytes);
            }
            Ok(())
        } else {
            Err(light_errors::UNDESIRED_BUCKET)
        }
    }
}

impl DhtBlocks {
    pub(crate) fn new(node_id: NodeId, k: Bucket) -> Self {
        Self {
            dht: Dht::new(node_id, k, RwLock::new(DoubleKeyedHashMap::new())),
            verified_blocks: RwLock::new(FIFOSet::new(10000)),
            min_stored_blocks: Mutex::new(0),
        }
    }

    pub fn is_desired_bucket(&self, number: u64, hash: B256) -> bool {
        self.dht.is_desired_bucket(&number.to_bucket())
            || self.dht.is_desired_bucket(&hash.to_bucket())
    }

    pub(crate) fn store_block_if_desired(
        &self,
        stateless_block: StatelessBlock,
    ) -> Result<(), LightError> {
        self.insert_to_store(stateless_block.bytes().to_owned())
    }

    pub(crate) fn next_block_to_store(&self) -> Result<u64, LightError> {
        let mut n = *self.min_stored_blocks.lock().unwrap();
        loop {
            if self.get_from_store(CompositeKey::First(n))?.is_none() {
                return Ok(n);
            }
            n += 1;
        }
    }

    /// Given a bucket and an offset `k`, returns the `k`-th block number that maps to that bucket,
    /// respecting the inverse of the bucket mapping and ring behavior.
    pub fn bucket_to_number(bucket: &Bucket, k: u32) -> u64 {
        let as_bytes: [u8; 20] = bucket.to_be_bytes();
        let mut full_bytes = [0u8; 32];
        full_bytes[12..].copy_from_slice(&as_bytes);
        let bucket_value = U256::from_be_bytes(full_bytes);

        let base = U256::from(k) * U256::from(NUM_BUCKETS);
        let offset = bucket_value / (U256::from(Bucket::MAX) / U256::from(NUM_BUCKETS));
        let n = base + offset;
        n.try_into().unwrap()
    }

    /// Compute an Iterator that lists all desired blocks until `max_block`.
    pub(crate) fn bucket_to_number_iter(&self, max_block: u64) -> impl Iterator<Item = u64> {
        struct BlockIter {
            bucket_range_iter: BucketRangeIter,
            last_range: RangeInclusive<u64>,
            last_block: u64,
        }

        impl BlockIter {
            fn new(bucket_lo: Bucket, bucket_hi: Bucket, max_block: u64) -> Self {
                let iter = BucketRangeIter::new(bucket_lo, bucket_hi, max_block);

                Self {
                    bucket_range_iter: iter,
                    last_block: 0,
                    last_range: 0..=0,
                }
            }
        }

        impl Iterator for BlockIter {
            type Item = u64;

            fn next(&mut self) -> Option<Self::Item> {
                if self.last_block >= *self.last_range.end() {
                    self.last_range = self.bucket_range_iter.next()?;
                    self.last_block = *self.last_range.start();
                    return Some(self.last_block);
                }
                self.last_block += 1;
                Some(self.last_block)
            }
        }

        struct BucketRange {
            bucket_lo: Bucket,
            bucket_hi: Bucket,
        }

        struct BucketRangeIter {
            check_range1: bool,
            range1: BucketRange,
            range2: Option<BucketRange>,
            last_k: u32,
            max_block: u64,
        }

        impl BucketRangeIter {
            fn new(bucket_lo: Bucket, bucket_hi: Bucket, max_block: u64) -> Self {
                let (range1, range2) = match bucket_lo.cmp(&bucket_hi) {
                    Ordering::Less => (
                        BucketRange {
                            bucket_lo,
                            bucket_hi,
                        },
                        None,
                    ),
                    Ordering::Equal => (
                        BucketRange {
                            bucket_lo: Bucket::ZERO,
                            bucket_hi: Bucket::MAX,
                        },
                        None,
                    ),
                    Ordering::Greater => (
                        BucketRange {
                            bucket_lo: Bucket::ZERO,
                            bucket_hi,
                        },
                        Some(BucketRange {
                            bucket_lo,
                            bucket_hi: Bucket::MAX,
                        }),
                    ),
                };

                Self {
                    check_range1: true,
                    range1,
                    range2,
                    last_k: 0,
                    max_block,
                }
            }

            fn block_range1(&self) -> RangeInclusive<u64> {
                let BucketRange {
                    bucket_lo,
                    bucket_hi,
                } = &self.range1;
                let block_lo = DhtBlocks::bucket_to_number(bucket_lo, self.last_k);
                let block_hi = DhtBlocks::bucket_to_number(bucket_hi, self.last_k);
                let block_hi = std::cmp::min(block_hi, self.max_block);
                block_lo..=block_hi
            }

            fn block_range2(&self) -> Option<RangeInclusive<u64>> {
                let range = self.range2.as_ref();
                let (block_lo, block_hi) = range.map(|range| {
                    let BucketRange {
                        bucket_lo,
                        bucket_hi,
                    } = range;
                    (
                        DhtBlocks::bucket_to_number(bucket_lo, self.last_k),
                        DhtBlocks::bucket_to_number(bucket_hi, self.last_k),
                    )
                })?;
                Some(block_lo..=block_hi)
            }
        }

        impl Iterator for BucketRangeIter {
            type Item = RangeInclusive<u64>;

            fn next(&mut self) -> Option<Self::Item> {
                let range = if self.check_range1 {
                    self.check_range1 = false;
                    self.block_range1()
                } else {
                    self.check_range1 = true;
                    self.block_range2().unwrap_or(self.block_range1())
                };
                if *range.start() > self.max_block {
                    return None;
                }
                self.last_k += 1;
                Some(range)
            }
        }

        let (bucket_lo, bucket_hi) = self.dht.bucket_dht.bucket_range();
        BlockIter::new(*bucket_lo, *bucket_hi, max_block)
    }
}

#[cfg(test)]
mod tests {
    use crate::dht::block::{DhtBlocks, NUM_BUCKETS};
    use crate::dht::{Bucket, ConcreteDht};
    use crate::id::NodeId;

    #[test]
    fn block_to_bucket_roundtrip() {
        let cases = [
            0, 1, 10, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000, 59_999_999,
        ];
        for &original in &cases {
            let bucket = original.to_bucket();
            for k in 0..100 {
                let recovered = DhtBlocks::bucket_to_number(&bucket, k);
                assert_eq!(original % NUM_BUCKETS, recovered % NUM_BUCKETS);
            }
        }
    }

    #[test]
    fn block_iter() {
        let dht = DhtBlocks::new(NodeId::default(), Bucket::MAX);
        let blocks = 1000;
        let max_block = blocks - 1;
        let blocks_iter = dht.bucket_to_number_iter(max_block);
        let count = blocks_iter.count() as u64;
        assert_eq!(count, blocks);
    }

    // use test::Bencher;
    // #[bench]
    // fn bench_block_iter(b: &mut Bencher) {
    //     let dht = DhtBlocks::new(NodeId::default(), Bucket::MAX);
    //     let blocks = 100_000_000;
    //     let max_block = blocks - 1;
    //     b.iter(|| {
    //         let blocks_iter = dht.bucket_to_number_iter(max_block);
    //         let count = blocks_iter.count() as u64;
    //         assert_eq!(count, blocks);
    //     });
    // }

    #[test]
    fn wants_all_blocks() {
        let dht = DhtBlocks::new(NodeId::default(), Bucket::MAX);
        assert!(dht.is_desired_bucket(0, Default::default()));
        assert!(dht.is_desired_bucket(1, Default::default()));
        assert!(dht.is_desired_bucket(u64::MAX, Default::default()));
        assert!(dht.is_desired_bucket(231312, Default::default()));
        assert!(dht.is_desired_bucket(1337, Default::default()));
    }
}
