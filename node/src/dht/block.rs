use crate::dht::kademlia::LockedMapDb;
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
use alloy::primitives::{keccak256, FixedBytes};
use proto_lib::p2p::message::Message;
use proto_lib::p2p::GetAcceptedFrontier;
use proto_lib::p2p::{Accepted, GetAccepted};
use proto_lib::p2p::{EngineType, GetAncestors};
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
    pub(crate) fn new(node_id: NodeId) -> Self {
        Self {
            node: Mutex::new(None),
            dht: Dht::new(
                node_id,
                Bucket::from(10),
                RwLock::new(DoubleKeyedHashMap::new()),
            ),
            verified_blocks: RwLock::new(FIFOSet::new(10000)),
        }
    }

    pub fn todo_attach_node(&self, node: Arc<Node>) {
        *self.node.lock().unwrap() = Some(node);
    }

    // TODO here me mostly need the network. Rewrite this function.
    async fn sync_process(self: Arc<Self>, node: Arc<Node>) {
        let chain_id = node.network.config.c_chain_id.as_ref().to_vec();
        // TODO instead of a random bootstrapper, we should pick them in a loop.
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
            for (i, container) in message.containers.into_iter().enumerate() {
                let block = StatelessBlock::unpack(&container).unwrap();
                if i == len - 1 {
                    last_container_id = block.id().as_ref().to_vec();
                }
                let number = u64::from_be_bytes(*block.block.header.number());
                let hash = block.block.hash;
                self.dht
                    .store
                    .insert(CompositeKey::Both(number, hash), container);
                bootstrapper = Self::pick_random_bootstrapper(&node.network).await;
            }
        }
    }

    pub async fn sync_headers(self: Arc<Self>, node: Arc<Node>, mut rx: broadcast::Receiver<()>) {
        let dht = self.clone();
        let process = tokio::spawn(dht.sync_process(node));
        tokio::select! {
            _ = process => {},
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
