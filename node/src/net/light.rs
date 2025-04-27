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
use std::collections::{BTreeMap, HashMap};
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
            LightMessage::Nodes(node_ids) => {
                todo!(); // TODO: store node_ids which are interesting to us.
                // TODO: look at potentially_add_nodes function in LightPeers.
                None
            }
        };

        match (res, resp) {
            (Some(res), Some(resp)) => {
                let _ = resp.send(res);
            }
            (None, None) => (),
            _ => {
                log::error!("unexpected state for res and resp values");
                log::error!("this is a logic bug. please report it.");
                // unreachable!();
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
    #[allow(unused)]
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

pub struct LightPeers {
    node_id: NodeId,
    light_peers: Arc<RwLock<HashMap<NodeId, DhtBuckets>>>,
}

impl LightPeers {
    pub fn new(node_id: NodeId, light_peers: Arc<RwLock<HashMap<NodeId, DhtBuckets>>>) -> Self {
        Self {
            node_id,
            light_peers,
        }
    }

    /// Find a maximum of `n` light peers which are the closest to the `bucket`.
    pub fn closest(&self, bucket: &Bucket, n: usize) -> Vec<NodeId> {
        // TODO this function could be cached if bucket is the same and n is <=.
        let light_peers = self.light_peers.read().unwrap();
        // TODO provide From / Into impl for NodeId to Bucket and implement distance function over
        //  Into<Bucket> generics.
        let distances: BTreeMap<_, _> = light_peers
            .keys()
            .map(|node_id| {
                let distance =
                    KademliaDht::distance(bucket, Bucket::from_be_bytes((*node_id).into()));
                (*node_id, distance)
            })
            .collect();
        drop(light_peers);
        distances
            .into_iter()
            .take(n)
            .map(|(node_id, _)| node_id)
            .collect()
    }

    pub fn check_interesting_node_ids(&self, node_ids: &[NodeId]) -> Vec<NodeId> {
        let my_bucket = Bucket::from_be_bytes(self.node_id.into());
        let distances_from_us: Vec<_> = node_ids
            .iter()
            .map(|node_id| {
                let bucket_b = Bucket::from_be_bytes((*node_id).into());
                KademliaDht::distance(&my_bucket, bucket_b)
            })
            .collect();
        let mut distances_rev = {
            let mut distances_from_us_clone: Vec<_> =
                distances_from_us.clone().into_iter().enumerate().collect();
            distances_from_us_clone.sort_by_key(|(_, distance)| *distance);
            distances_from_us_clone
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
        };
        let mut closest_indexes = Vec::new();
        {
            let light_peers = self.light_peers.read().unwrap();
            for light_peer in light_peers.keys() {
                if closest_indexes.len() >= node_ids.len() {
                    break;
                }
                let i = {
                    let mut i = 0;
                    loop {
                        if i >= distances_rev.len() {
                            break None;
                        }
                        let (idx, distance) = distances_rev.get(i).unwrap();
                        let light_peer_bucket = Bucket::from_be_bytes((*light_peer).into());
                        let distance_with_peer = KademliaDht::distance(&my_bucket, light_peer_bucket);
                        if distance < &distance_with_peer {
                            let idx = *idx;
                            distances_rev.remove(i);
                            break Some(idx);
                        }
                        i += 1;
                    }
                };
                if let Some(i) = i {
                    closest_indexes.push(i);
                }
            }
        }
        closest_indexes
            .into_iter()
            .map(|index| node_ids[index])
            .collect()
    }

    pub fn potentially_add_nodes(&self, node_ids: &[NodeId]) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_check_interesting_node_ids() {
        // Create a base node ID (our node)
        let our_node_id = NodeId::from([0u8; 20]);

        // Create some peer nodes that are at varying distances
        let far_peer = NodeId::from([0xF0; 20]);  // Very far peer
        let medium_peer = NodeId::from([0x80; 20]); // Medium distance peer
        
        // Create the light peers map
        let light_peers = Arc::new(RwLock::new(HashMap::from([
            (far_peer, DhtBuckets { block: Default::default() }),
            (medium_peer, DhtBuckets { block: Default::default() }),
        ])));

        let light_peers_struct = LightPeers::new(our_node_id, light_peers);

        // Test case 1: Node closer than all peers
        let close_node = NodeId::from([0x10; 20]); // Very close to our node
        let result = light_peers_struct.check_interesting_node_ids(&[close_node]);
        assert_eq!(result, vec![close_node], "Should return the closer node");

        // Test case 2: Multiple nodes, some closer some farther
        let very_close_node = NodeId::from([0x05; 20]);
        let very_far_node = NodeId::from([0xFF; 20]);
        let nodes_to_check = vec![very_far_node, very_close_node];
        let result = light_peers_struct.check_interesting_node_ids(&nodes_to_check);
        assert_eq!(result, vec![very_close_node], "Should only return the closer node");

        // Test case 3: All nodes farther than peers
        let far_node_1 = NodeId::from([0xFF; 20]);
        let far_node_2 = NodeId::from([0xFE; 20]);
        let result = light_peers_struct.check_interesting_node_ids(&[far_node_1, far_node_2]);
        assert!(result.is_empty(), "Should return empty vec when no nodes are closer than peers");

        // Test case 4: Empty input
        let result = light_peers_struct.check_interesting_node_ids(&[]);
        assert!(result.is_empty(), "Should handle empty input gracefully");

        // Test case 5: Multiple interesting nodes
        let close_node_1 = NodeId::from([0x01; 20]);
        let close_node_2 = NodeId::from([0x02; 20]);
        let nodes_to_check = vec![close_node_1, close_node_2];
        let result = light_peers_struct.check_interesting_node_ids(&nodes_to_check);
        assert_eq!(result.len(), 2, "Should return both close nodes");
        assert!(result.contains(&close_node_1));
        assert!(result.contains(&close_node_2));
    }

    #[test]
    fn test_closest() {
        // Create a base node ID (our node)
        let our_node_id = NodeId::from([0u8; 20]);

        // Create peers at different distances
        let very_close_peer = NodeId::from([0x01; 20]);
        let close_peer = NodeId::from([0x10; 20]);
        let medium_peer = NodeId::from([0x80; 20]);
        let far_peer = NodeId::from([0xFF; 20]);
        
        // Create the light peers map
        let light_peers = Arc::new(RwLock::new(HashMap::from([
            (very_close_peer, DhtBuckets { block: Default::default() }),
            (close_peer, DhtBuckets { block: Default::default() }),
            (medium_peer, DhtBuckets { block: Default::default() }),
            (far_peer, DhtBuckets { block: Default::default() }),
        ])));
        
        // Create the LightPeers struct
        let light_peers_struct = LightPeers::new(our_node_id, light_peers.clone());

        // Create a target bucket
        let target_bucket = Bucket::from_be_bytes([0u8; 20]);

        // Test case 1: Get single closest peer
        let result = light_peers_struct.closest(&target_bucket, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], very_close_peer);

        // Test case 2: Get two closest peers
        let result = light_peers_struct.closest(&target_bucket, 2);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&very_close_peer));
        assert!(result.contains(&close_peer));
        assert!(!result.contains(&medium_peer));
        assert!(!result.contains(&far_peer));

        // Test case 3: Request more peers than available
        let result = light_peers_struct.closest(&target_bucket, 10);
        assert_eq!(result.len(), 4); // Should return all
    }
}