use crate::dht::block::DhtBlocks;
use crate::dht::kademlia::{KademliaDht, LockedMapDb, ValueOrNodes};
use crate::dht::{light_errors, DhtBuckets, LightValue};
use crate::dht::{Bucket, ConcreteDht, DhtId, LightMessage, LightResult};
use crate::id::{ChainId, NodeId};
use crate::message::mail_box::Mail;
use crate::net::node::NodeError;
use crate::net::LightError;
use crate::net::RwLock;
use crate::node::Node;
use crate::server::peers::PeerInfo;
use crate::utils::rlp::Block;
use crate::utils::unpacker::StatelessBlock;
use crate::Arc;
use flume::Sender;
use indexmap::IndexMap;
use std::collections::HashMap;
use tokio::sync::broadcast;
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct LightNetwork {
    pub kademlia_dht: KademliaDht,
    pub block_dht: Arc<DhtBlocks>,
    pub light_peers: Arc<RwLock<HashMap<NodeId, DhtBuckets>>>,
    sync_headers: bool,
    max_lookups: usize,
    alpha: usize,
}

impl LightNetwork {
    pub fn new(
        node_id: NodeId,
        peer_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        mail_tx: Sender<Mail>,
        chain_id: ChainId,
        sync_headers: bool,
        max_lookups: usize,
        alpha: usize,
    ) -> Self {
        let store = RwLock::new(HashMap::new());
        let block_dht = Arc::new(DhtBlocks::new(node_id, Bucket::from(10), store));
        let light_peers = Arc::new(RwLock::new(Default::default()));
        let kademlia_dht = KademliaDht::new(peer_infos, light_peers.clone(), mail_tx, chain_id, 10);
        Self {
            kademlia_dht,
            block_dht,
            light_peers,
            sync_headers,
            max_lookups,
            alpha,
        }
    }

    pub async fn start(
        &self,
        node: Arc<Node>,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        if self.sync_headers {
            let block_dht = self.block_dht.clone();
            let rx = rx.resubscribe();
            tokio::spawn(async move {
                block_dht.sync_headers(node, rx).await;
            });
        }

        let _ = rx.recv().await;
        Ok(())
    }

    pub fn manage_message(
        &self,
        node_id: &NodeId,
        message: LightMessage,
        resp: Option<oneshot::Sender<LightResult>>,
    ) {
        let res = match message {
            LightMessage::NewPeer(buckets) => {
                self.light_peers.write().unwrap().insert(*node_id, buckets);
                None
            }
            LightMessage::Store(dht_id, value) => {
                let res = match dht_id {
                    DhtId::Block => self
                        .block_dht
                        .insert_to_store(value)
                        .map(|_| LightValue::Ok),
                    _ => Err(light_errors::INVALID_DHT),
                };
                Some(res)
            }
            LightMessage::FindNode(bucket) => {
                let res = Ok(LightValue::ValueOrNodes(ValueOrNodes::Nodes(
                    self.find_node(&bucket),
                )));
                Some(res)
            }
            LightMessage::FindValue(dht_id, bucket) => {
                let res = match dht_id {
                    DhtId::Block => self.find_value(&self.block_dht.store, &bucket),
                    _ => Err(light_errors::INVALID_DHT),
                };
                Some(res)
            }
            LightMessage::Nodes(_node_ids) => {
                todo!(); // TODO: store node_ids which are interesting to us.
            }
        };

        match (res, resp) {
            (Some(res), Some(resp)) => {
                let _ = resp.send(res);
            }
            (None, None) => (),
            _ => {
                log::error!("unexpected state for res and resp values");
            }
        }
    }

    /// Lookup locally for nodes spanning the bucket.
    fn find_node(&self, bucket: &Bucket) -> Vec<NodeId> {
        self.kademlia_dht.find_node(bucket)
    }

    /// Lookup locally for a value or nodes spanning the bucket.
    fn find_value<DB>(&self, db: &DB, bucket: &Bucket) -> LightResult
    where
        DB: LockedMapDb<Bucket, Vec<u8>>,
    {
        let value_or_nodes = match db.get(bucket) {
            Some(value) => ValueOrNodes::Value(value),
            None => ValueOrNodes::Nodes(self.find_node(bucket)),
        };
        Ok(LightValue::ValueOrNodes(value_or_nodes))
    }

    /// Check if the value is stored locally.
    /// If not, lookup the DHT for it.
    async fn find_content<DHT, K, V>(&self, dht: &Arc<DHT>, key: K) -> Result<V, LightError>
    where
        DHT: ConcreteDht<K> + DhtContent<K, V>,
    {
        let bucket = DHT::key_to_bucket(key);
        match dht
            .get_from_store(&bucket)
            .expect("should not be stored if ill-formed")
        {
            Some(value) => Ok(value),
            None => {
                let value = self
                    .kademlia_dht
                    .search_value(&DHT::id(), &bucket, self.max_lookups, self.alpha)
                    .await?;
                DHT::decode(&value)
            }
        }
    }

    pub async fn find_block(&self, number: u64) -> Result<Block, LightError> {
        self.find_content(&self.block_dht, number)
            .await
            .map(|stateless_block| stateless_block.block)
    }
}

pub trait DhtContent<K, V>: ConcreteDht<K> {
    /// Get a typed value from the store.
    fn get_from_store(&self, bucket: &Bucket) -> Result<Option<V>, LightError>;
    /// Insert an encoded value into the store.
    /// If the value is ill-formed, it should not be stored.
    fn insert_to_store(&self, bytes: Vec<u8>) -> Result<Option<V>, LightError>;
    /// Verification of the validity of the content.
    /// It is used to check if the content is well-formed.
    /// If the content is ill-formed, it should not be stored.
    fn verify(&self, value: &V) -> bool;
    /// Typed to encoded value
    fn encode(value: V) -> Result<Vec<u8>, LightError>;
    /// Encoded to typed value
    fn decode(bytes: &[u8]) -> Result<V, LightError>;
}

impl DhtContent<u64, StatelessBlock> for DhtBlocks {
    fn get_from_store(&self, bucket: &Bucket) -> Result<Option<StatelessBlock>, LightError> {
        match self.store.get(bucket) {
            Some(block_bytes) => {
                let block = Self::decode(&block_bytes)?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    fn insert_to_store(&self, bytes: Vec<u8>) -> Result<Option<StatelessBlock>, LightError> {
        let decoded = Self::decode(&bytes)?;
        if !self.verify(&decoded) {
            return Err(light_errors::INVALID_CONTENT);
        }
        let number = u64::from_be_bytes(*decoded.block.header.number());
        let bucket = Self::key_to_bucket(number);
        if !self.is_desired_bucket(&bucket) {
            return Err(light_errors::UNDESIRED_BUCKET);
        }
        match self.store.insert(bucket, bytes) {
            Some(ret_bytes) => Ok(Some(Self::decode(&ret_bytes)?)),
            None => Ok(None),
        }
    }

    fn verify(&self, _block: &StatelessBlock) -> bool {
        // TODO implement robust block verification i.e make sure that the hash matches at least.
        //   Then, send GetAccepted message to bootstrap node if this block doesn't come from a bootstrap node.
        true
    }

    fn encode(_value: StatelessBlock) -> Result<Vec<u8>, LightError> {
        todo!()
    }

    fn decode(bytes: &[u8]) -> Result<StatelessBlock, LightError> {
        StatelessBlock::unpack(bytes).map_err(|_| light_errors::DECODING_FAILED)
    }
}
