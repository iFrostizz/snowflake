use crate::dht::kademlia::{KademliaDht, LockedMapDb};
use crate::dht::{light_errors, Bucket, DhtId, LightError};
use crate::dht::{ConcreteDht, Dht};
use crate::id::{BlockID, NodeId};
use crate::message::SubscribableMessage;
use crate::net::light::{DhtCodex, DhtContent};
use crate::net::Network;
use crate::node::{MessageOrSubscribable, Node, SinglePickerConfig};
use crate::utils::constants::DEFAULT_DEADLINE;
use crate::utils::twokhashmap::{CompositeKey, DoubleKeyedHashMap};
use crate::utils::unpacker::StatelessBlock;
use crate::utils::FIFOSet;
use crate::Arc;
use alloy::primitives::U256;
use alloy::primitives::{keccak256, FixedBytes};
use proto_lib::p2p::message::Message;
use proto_lib::p2p::GetAcceptedFrontier;
use proto_lib::p2p::{Accepted, GetAccepted};
use proto_lib::p2p::{EngineType, GetAncestors};
use std::cmp::Ordering;
use std::ops::RangeInclusive;
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;

#[derive(Debug)]
pub struct DhtBlocks {
    node: Mutex<Option<Arc<Node>>>,
    pub dht: Dht<RwLock<DoubleKeyedHashMap<Bucket, Bucket, Vec<u8>>>>,
    pub verified_blocks: RwLock<FIFOSet<BlockID>>,
}

impl DhtCodex<StatelessBlock> for DhtBlocks {
    fn id() -> DhtId {
        DhtId::Block
    }

    async fn verify(&self, block: &StatelessBlock) -> Result<bool, LightError> {
        let number = u64::from_be_bytes(*block.block.header.number());
        let block_id = block.block.hash;
        if self
            .get_from_store(CompositeKey::Both(number, block_id))?
            .is_some()
        {
            return Ok(true);
        }
        if self.verified_blocks.read().unwrap().contains(block.id()) {
            return Ok(true);
        }
        let node = self.node.lock().unwrap().clone().unwrap();
        let bootstrapper = Self::pick_random_bootstrapper(&node.network).await;
        let message = SubscribableMessage::GetAccepted(GetAccepted {
            chain_id: node.network.config.c_chain_id.as_ref().to_vec(),
            request_id: rand::random(),
            deadline: DEFAULT_DEADLINE,
            container_ids: vec![block.id().as_ref().to_vec()],
        });
        let res = node
            .send_to_peer(
                &MessageOrSubscribable::Subscribable(message.clone()),
                bootstrapper,
            )
            .await;
        if let Some(Message::Accepted(Accepted {
            chain_id,
            request_id: _request_id,
            container_ids,
        })) = res
        {
            if chain_id != node.network.config.c_chain_id.as_ref().to_vec() {
                return Err(light_errors::INVALID_CONTENT);
            }
            if container_ids == vec![block.id().as_ref().to_vec()] {
                self.verified_blocks.write().unwrap().insert(*block.id());
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(light_errors::INVALID_CONTENT)
        }
    }

    fn encode(value: StatelessBlock) -> Result<Vec<u8>, LightError> {
        value.pack().map_err(|_| light_errors::ENCODING_FAILED)
    }

    fn decode(bytes: &[u8]) -> Result<StatelessBlock, LightError> {
        StatelessBlock::unpack(bytes).map_err(|_| light_errors::DECODING_FAILED)
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
    fn get_from_store(
        &self,
        key: CompositeKey<u64, FixedBytes<32>>,
    ) -> Result<Option<StatelessBlock>, LightError> {
        match key {
            CompositeKey::Both(k1, k2) => {
                if let Some(block_bytes) = self.dht.store.get_bucket(&k1.to_bucket()) {
                    let block = Self::decode(&block_bytes)?;
                    return Ok(Some(block));
                }
                match self.dht.store.get_bucket(&k2.to_bucket()) {
                    Some(block_bytes) => {
                        let block = Self::decode(&block_bytes)?;
                        Ok(Some(block))
                    }
                    None => Ok(None),
                }
            }
            key => match self.dht.store.get(&key) {
                Some(block_bytes) => {
                    let block = Self::decode(&block_bytes)?;
                    Ok(Some(block))
                }
                None => Ok(None),
            },
        }
    }

    async fn insert_to_store(&self, bytes: Vec<u8>) -> Result<Option<StatelessBlock>, LightError> {
        let decoded = Self::decode(&bytes)?;
        let number = u64::from_be_bytes(*decoded.block.header.number());
        let block_id = decoded.block.hash;
        if self.dht.is_desired_bucket(&number.to_bucket())
            || self.dht.is_desired_bucket(&block_id.to_bucket())
        {
            if !self.verify(&decoded).await? {
                return Err(light_errors::INVALID_CONTENT);
            }
            match self
                .dht
                .store
                .insert(CompositeKey::Both(number, block_id), bytes)
            {
                Some(ret_bytes) => Ok(Some(Self::decode(&ret_bytes)?)),
                None => Ok(None),
            }
        } else {
            Err(light_errors::UNDESIRED_BUCKET)
        }
    }
}

impl DhtBlocks {
    pub(crate) fn new(node_id: NodeId, k: Bucket) -> Self {
        Self {
            node: Mutex::new(None),
            dht: Dht::new(node_id, k, RwLock::new(DoubleKeyedHashMap::new())),
            verified_blocks: RwLock::new(FIFOSet::new(10000)),
        }
    }

    pub fn todo_attach_node(&self, node: Arc<Node>) {
        *self.node.lock().unwrap() = Some(node);
    }

    // TODO here we mostly need the network. Rewrite this function.
    async fn sync_process(self: Arc<Self>, node: Arc<Node>) {
        let chain_id = node.network.config.c_chain_id.as_ref().to_vec();
        let mut bootstrapper = Self::pick_random_bootstrapper(&node.network).await;

        let message = SubscribableMessage::GetAcceptedFrontier(GetAcceptedFrontier {
            chain_id: chain_id.clone(),
            request_id: rand::random(),
            deadline: DEFAULT_DEADLINE,
        });
        let message = loop {
            if let Some(Message::AcceptedFrontier(res)) = node
                .send_to_peer(
                    &MessageOrSubscribable::Subscribable(message.clone()),
                    bootstrapper,
                )
                .await
            {
                break res;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };

        let mut last_container_id = message.container_id;
        loop {
            let message = SubscribableMessage::GetAncestors(GetAncestors {
                chain_id: chain_id.clone(),
                request_id: rand::random(),
                deadline: DEFAULT_DEADLINE,
                container_id: last_container_id.clone(),
                engine_type: EngineType::Snowman.into(),
            });
            let message = loop {
                if let Some(Message::Ancestors(res)) = node
                    .send_to_peer(
                        &MessageOrSubscribable::Subscribable(message.clone()),
                        bootstrapper,
                    )
                    .await
                {
                    break res;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            };

            let len = message.containers.len();
            log::debug!("Syncing {} containers", len);
            for (i, container) in message.containers.into_iter().enumerate() {
                let block = StatelessBlock::unpack(&container).unwrap();
                if i == len - 1 {
                    last_container_id = block.id().as_ref().to_vec();
                }
                self.store_block(block);
                bootstrapper = Self::pick_random_bootstrapper(&node.network).await;
            }
        }
    }

    fn store_block(&self, block: StatelessBlock) {
        let number = u64::from_be_bytes(*block.block.header.number());
        let hash = block.block.hash;
        if self.dht.is_desired_bucket(&number.to_bucket()) {
            self.dht.store.insert(
                CompositeKey::Both(number, hash),
                Self::encode(block).unwrap(),
            );
        }
    }

    /// Sync headers from peers using the Kademlia DHT
    pub(crate) async fn sync_blocks(self: Arc<Self>, kademlia_dht: KademliaDht) {
        // let last_block = 70000000; // TODO request from bootstrap node
        let last_block = 62000000;
        let blocks = self.bucket_to_number_iter2(last_block);
        for block_key in blocks {
            log::debug!("Syncing block {}", block_key);
            if block_key > 61000000 {
                // TODO should just use the find_content function.
                if let Ok(block_bytes) = kademlia_dht
                    .search_value(&DhtId::Block, &block_key.to_bucket(), 10, 3)
                    .await
                {
                    log::debug!("Found block {}", block_key);
                    let block = Self::decode(&block_bytes).unwrap(); // TODO !
                    self.store_block(block);
                }
            }
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

    fn bucket_to_number_iter2(&self, max_block: u64) -> impl Iterator<Item = u64> {
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

        struct BucketRangeIter {
            range1: bool,
            bucket_lo1: Bucket,
            bucket_hi1: Bucket,
            bucket_lo2: Option<Bucket>,
            bucket_hi2: Option<Bucket>,
            last_k: u32,
            max_block: u64,
        }

        impl BucketRangeIter {
            fn new(bucket_lo: Bucket, bucket_hi: Bucket, max_block: u64) -> Self {
                let (bucket_lo1, bucket_hi1, bucket_lo2, bucket_hi2) =
                    match bucket_lo.cmp(&bucket_hi) {
                        Ordering::Less => (bucket_lo, bucket_hi, None, None),
                        Ordering::Equal => (Bucket::ZERO, Bucket::MAX, None, None),
                        Ordering::Greater => {
                            (Bucket::ZERO, bucket_hi, Some(bucket_lo), Some(Bucket::MAX))
                        }
                    };

                Self {
                    range1: true,
                    bucket_lo1,
                    bucket_hi1,
                    bucket_lo2,
                    bucket_hi2,
                    last_k: 0,
                    max_block,
                }
            }

            fn block_range1(&self) -> RangeInclusive<u64> {
                let block_lo = DhtBlocks::bucket_to_number(&self.bucket_lo1, self.last_k);
                let block_hi = DhtBlocks::bucket_to_number(&self.bucket_hi1, self.last_k);
                let block_hi = std::cmp::min(block_hi, self.max_block);
                block_lo..=block_hi
            }

            fn block_range2(&self) -> Option<RangeInclusive<u64>> {
                let block_lo = self
                    .bucket_lo2
                    .map(|bucket| DhtBlocks::bucket_to_number(&bucket, self.last_k))?;
                let block_hi = self
                    .bucket_hi2
                    .map(|bucket| DhtBlocks::bucket_to_number(&bucket, self.last_k))?;
                Some(block_lo..=block_hi)
            }
        }

        impl Iterator for BucketRangeIter {
            type Item = RangeInclusive<u64>;

            fn next(&mut self) -> Option<Self::Item> {
                let range = if self.range1 {
                    self.range1 = false;
                    self.block_range1()
                } else {
                    self.range1 = true;
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

    pub async fn sync_headers(self: Arc<Self>, node: Arc<Node>, mut rx: broadcast::Receiver<()>) {
        let dht = self.clone();
        let bootstrap_process = tokio::spawn(dht.sync_process(node));
        tokio::select! {
            _ = bootstrap_process => {},
            _ = rx.recv() => {},
        }
    }

    async fn pick_random_bootstrapper(network: &Arc<Network>) -> NodeId {
        let mut maybe_bootstrapper = Network::pick_peer(
            &network.peers_infos,
            &network.bootstrappers,
            SinglePickerConfig::Bootstrapper,
        );
        while maybe_bootstrapper.is_none() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            maybe_bootstrapper = Network::pick_peer(
                &network.peers_infos,
                &network.bootstrappers,
                SinglePickerConfig::Bootstrapper,
            );
        }
        maybe_bootstrapper.unwrap()
    }

    pub async fn insert_block(&self, bytes: Vec<u8>) -> Result<(), LightError> {
        let decoded = Self::decode(&bytes)?;

        let number = u64::from_be_bytes(*decoded.block.header.number());
        let hash = decoded.block.hash;
        if self.dht.is_desired_bucket(&number.to_bucket())
            || self.dht.is_desired_bucket(&hash.to_bucket())
        {
            if !self.verify(&decoded).await? {
                return Err(light_errors::INVALID_CONTENT);
            }
            self.dht
                .store
                .insert(CompositeKey::Both(number, hash), bytes);
            Ok(())
        } else {
            Err(light_errors::UNDESIRED_BUCKET)
        }
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
        let blocks_iter = dht.bucket_to_number_iter2(max_block);
        let count = blocks_iter.count() as u64;
        assert_eq!(count, blocks);
    }
}
