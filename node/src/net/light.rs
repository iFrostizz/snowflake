use crate::dht::block::DhtBlocks;
use crate::dht::kademlia::{KademliaDht, LockedMapDb, ValueOrNodes};
use crate::dht::{light_errors, DhtBuckets, LightValue};
use crate::dht::{Bucket, ConcreteDht, DhtId, LightMessage, LightResult};
use crate::id::{ChainId, NodeId};
use crate::message::mail_box::Mail;
use crate::net::node::NodeError;
use crate::net::queue::{ConnectionData, ConnectionQueue};
use crate::net::RwLock;
use crate::net::{LightError, Network};
use crate::node::Node;
use crate::server::peers::PeerInfo;
use crate::Arc;
use flume::Sender;
use indexmap::IndexMap;
#[cfg(test)]
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{LockResult, RwLockReadGuard, RwLockWriteGuard};
use tokio::sync::broadcast;
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct LightNetworkConfig {
    pub sync_headers: bool,
    pub max_lookups: usize,
    pub alpha: usize,
}

#[derive(Debug)]
pub struct LightNetwork {
    pub kademlia_dht: KademliaDht,
    pub block_dht: Arc<DhtBlocks>,
    pub light_peers: LightPeers,
    config: LightNetworkConfig,
}

impl LightNetwork {
    pub fn new(
        node_id: NodeId,
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        connection_queue: Arc<ConnectionQueue>,
        mail_tx: Sender<Mail>,
        chain_id: ChainId,
        config: LightNetworkConfig,
        max_light_peers: Option<usize>,
    ) -> Self {
        let block_dht = Arc::new(DhtBlocks::new(node_id));
        let light_peers = LightPeers::new(
            node_id,
            peers_infos.clone(),
            connection_queue,
            max_light_peers,
        );
        let kademlia_dht =
            KademliaDht::new(peers_infos, light_peers.clone(), mail_tx, chain_id, 10);
        Self {
            kademlia_dht,
            block_dht,
            light_peers,
            config,
        }
    }

    pub async fn start(
        &self,
        node: Arc<Node>,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        if self.config.sync_headers {
            let block_dht = self.block_dht.clone();
            let rx = rx.resubscribe();
            tokio::spawn(async move {
                block_dht.sync_headers(node, rx).await;
            });
        }

        let _ = rx.recv().await;
        Ok(())
    }

    pub async fn manage_message(
        &self,
        node_id: &NodeId,
        message: LightMessage,
        resp: Option<oneshot::Sender<LightResult>>,
    ) {
        let res = match message {
            LightMessage::NewPeer(buckets) => {
                self.light_peers.write().insert(*node_id, buckets);
                None
            }
            LightMessage::Store(dht_id, value) => {
                let res = match dht_id {
                    DhtId::Block => self
                        .block_dht
                        .insert_block(value)
                        .await
                        .map(|_| LightValue::Ok),
                    _ => Err(light_errors::INVALID_DHT),
                };
                Some(res)
            }
            LightMessage::FindNode(bucket) => {
                let res = Ok(LightValue::ValueOrNodes(ValueOrNodes::Nodes(
                    self.kademlia_dht.find_node(&bucket),
                )));
                Some(res)
            }
            LightMessage::FindValue(dht_id, bucket) => {
                let res = match dht_id {
                    DhtId::Block => self.find_value(&self.block_dht.number_dht.store, &bucket),
                    _ => Err(light_errors::INVALID_DHT),
                };
                Some(res)
            }
            LightMessage::Nodes(node_ids) => {
                self.light_peers.potentially_add_nodes(node_ids);
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

    /// Lookup locally for a value or nodes spanning the bucket.
    fn find_value<DB>(&self, db: &DB, bucket: &Bucket) -> LightResult
    where
        DB: LockedMapDb<Bucket, Vec<u8>>,
    {
        let value_or_nodes = match db.get(bucket) {
            Some(value) => ValueOrNodes::Value(value),
            None => ValueOrNodes::Nodes(self.kademlia_dht.find_node(bucket)),
        };
        Ok(LightValue::ValueOrNodes(value_or_nodes))
    }

    /// Check if the value is stored locally.
    /// If not, lookup the DHT for it.
    pub async fn find_content<DHT, K, V>(&self, dht: &Arc<DHT>, key: K) -> Result<V, LightError>
    where
        DHT: ConcreteDht<K> + DhtContent<K, V>,
        K: Copy,
    {
        match dht
            .get_from_store(key)
            .expect("should not be stored if ill-formed")
        {
            Some(value) => Ok(value),
            None => {
                let bucket = DHT::key_to_bucket(key);
                let value = self
                    .kademlia_dht
                    .search_value(
                        &DHT::id(),
                        &bucket,
                        self.config.max_lookups,
                        self.config.alpha,
                    )
                    .await?;
                DHT::decode(&value)
            }
        }
    }

    pub async fn store<DHT, K, V>(
        &self,
        dht: &Arc<DHT>,
        node_id: NodeId,
        value: V,
    ) -> Result<(), LightError>
    where
        DHT: ConcreteDht<K> + DhtContent<K, V>,
    {
        let encoded = DHT::encode(value)?;
        if node_id == self.light_peers.node_id {
            dht.insert_to_store(encoded).await?;
            Ok(())
        } else {
            self.kademlia_dht.store(node_id, &DHT::id(), encoded).await
        }
    }
}

pub trait DhtCodex<V> {
    /// Verification of the validity of the content.
    /// It is used to check if the content is well-formed.
    /// If the content is ill-formed, it should not be stored.
    async fn verify(&self, value: &V) -> Result<bool, LightError>;
    /// Typed to encoded value
    fn encode(value: V) -> Result<Vec<u8>, LightError>;
    /// Encoded to typed value
    fn decode(bytes: &[u8]) -> Result<V, LightError>;
}

pub trait DhtContent<K, V>: ConcreteDht<K> + DhtCodex<V> {
    /// Get a typed value from the store.
    fn get_from_store(&self, bucket: K) -> Result<Option<V>, LightError>;
    /// Insert an encoded value into the store.
    /// If the value is ill-formed, it should not be stored.
    async fn insert_to_store(&self, bytes: Vec<u8>) -> Result<Option<V>, LightError>;
}

#[derive(Debug, Clone)]
pub struct LightPeers {
    node_id: NodeId,
    light_peers: Arc<RwLock<IndexMap<NodeId, DhtBuckets>>>,
    peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
    connection_queue: Arc<ConnectionQueue>,
    max_light_peers: Option<usize>,
}

// TODO: this structure weirdly looks like the LightPeers one.
#[derive(Debug)]
pub struct WriteLockGuardPeers<'a> {
    node_id: NodeId,
    pub map: RwLockWriteGuard<'a, IndexMap<NodeId, DhtBuckets>>,
    peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
    max_light_peers: Option<usize>,
}

impl WriteLockGuardPeers<'_> {
    pub fn insert(&mut self, node_id: NodeId, buckets: DhtBuckets) {
        self.map.insert(node_id, buckets);
    }

    #[cfg(test)]
    pub fn extend(&mut self, peers: HashMap<NodeId, DhtBuckets>) {
        self.map.extend(peers);
    }
}

pub fn closest_peer(node_id: NodeId, peers: &IndexMap<NodeId, DhtBuckets>) -> Option<NodeId> {
    let bucket = Bucket::from_be_bytes(node_id.into());
    let node_ids = peers.keys().cloned().collect::<Vec<_>>();
    node_ids
        .iter()
        .map(|node_id| {
            let bucket_b = Bucket::from_be_bytes((*node_id).into());
            KademliaDht::distance(&bucket, bucket_b)
        })
        .enumerate()
        .min_by_key(|(_, distance)| *distance)
        .map(|(i, _)| node_ids[i])
}

pub fn furthest_peer(node_id: NodeId, peers: &IndexMap<NodeId, DhtBuckets>) -> Option<NodeId> {
    let bucket = Bucket::from_be_bytes(node_id.into());
    let node_ids = peers.keys().cloned().collect::<Vec<_>>();
    node_ids
        .iter()
        .map(|node_id| {
            let bucket_b = Bucket::from_be_bytes((*node_id).into());
            KademliaDht::distance(&bucket, bucket_b)
        })
        .enumerate()
        .max_by_key(|(_, distance)| *distance)
        .map(|(i, _)| node_ids[i])
}

impl Drop for WriteLockGuardPeers<'_> {
    fn drop(&mut self) {
        if let Some(max_light_peers) = self.max_light_peers {
            let WriteLockGuardPeers { map, .. } = self;
            while map.len() > max_light_peers {
                let furthest = furthest_peer(self.node_id, map).unwrap();
                Network::disconnect_peer(self.peers_infos.clone(), map, furthest, None);
                // TODO add error
            }
        }
    }
}

impl LightPeers {
    pub fn new(
        node_id: NodeId,
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        connection_queue: Arc<ConnectionQueue>,
        max_light_peers: Option<usize>,
    ) -> Self {
        Self {
            node_id,
            light_peers: Default::default(),
            peers_infos,
            connection_queue,
            max_light_peers,
        }
    }

    pub fn read(&self) -> LockResult<RwLockReadGuard<'_, IndexMap<NodeId, DhtBuckets>>> {
        self.light_peers.read()
    }

    pub fn write(&self) -> WriteLockGuardPeers<'_> {
        WriteLockGuardPeers {
            node_id: self.node_id,
            map: self.light_peers.write().unwrap(),
            peers_infos: self.peers_infos.clone(),
            max_light_peers: self.max_light_peers,
        }
    }

    fn check_interesting_node_ids(&self, mut node_ids: Vec<NodeId>) -> Vec<NodeId> {
        debug_assert!(node_ids.len() == node_ids.clone().into_iter().collect::<HashSet<_>>().len());
        let light_peers = self.light_peers.read().unwrap();
        node_ids.retain(|node_id| !light_peers.contains_key(node_id));

        let my_bucket = Bucket::from_be_bytes(self.node_id.into());
        let distances_from_us: Vec<_> = node_ids
            .iter()
            .map(|node_id| {
                let bucket_b = Bucket::from_be_bytes((*node_id).into());
                KademliaDht::distance(&my_bucket, bucket_b)
            })
            .collect();

        let mut distances_rev = {
            let mut distances_from_us_sort: Vec<_> =
                distances_from_us.clone().into_iter().enumerate().collect();
            distances_from_us_sort.sort_by_key(|(_, distance)| *distance);
            // drop the furthest peers if they would not fit the max_light_peers.
            if let Some(max_light_peers) = self.max_light_peers {
                distances_from_us_sort.truncate(max_light_peers);
            }
            distances_from_us_sort.into_iter().rev().collect::<Vec<_>>()
        };

        let mut closest_indexes = Vec::new();
        {
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
                        let distance_with_peer =
                            KademliaDht::distance(&my_bucket, light_peer_bucket);
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

    /// Check if some of these nodes are closer to what we have in our list.
    /// If so, add them.
    pub fn potentially_add_nodes(&self, nodes: Vec<ConnectionData>) {
        let node_ids = self.check_interesting_node_ids(
            nodes
                .iter()
                .map(|ConnectionData { node_id, .. }| *node_id)
                .collect(),
        );
        if node_ids.len() != node_ids.clone().into_iter().collect::<HashSet<_>>().len() {
            return; // has duplicates. TODO: decrease reputation.
        }
        for node_id in node_ids {
            // Add connections and watch out for reaching the max number of connections.
            // If it is reached, drop the connection with the furthest peers.
            self.connection_queue.add_connection(
                nodes
                    .iter()
                    .find(|node| node.node_id == node_id)
                    .unwrap()
                    .clone(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::peers::PeerSender;
    use std::collections::HashMap;

    #[test]
    fn test_check_interesting_node_ids() {
        // Create a base node ID (our node)
        let our_node_id = NodeId::from([0u8; 20]);

        // Create some peer nodes that are at varying distances
        let far_peer = NodeId::from([0xF0; 20]);
        let medium_peer = NodeId::from([0x80; 20]);

        let peers = [far_peer, medium_peer];

        let peers_light: HashMap<_, _> = peers
            .iter()
            .map(|node_id| {
                (
                    *node_id,
                    DhtBuckets {
                        block: Default::default(),
                    },
                )
            })
            .collect();
        let peers_infos: IndexMap<_, _> = peers
            .iter()
            .map(|node_id| {
                (
                    *node_id,
                    PeerInfo {
                        x509_certificate: vec![],
                        sender: {
                            let (tx, _) = flume::unbounded();
                            PeerSender {
                                tx,
                                node_id: *node_id,
                            }
                        },
                        infos: None,
                        tx: {
                            let (tx, _) = broadcast::channel(1);
                            tx
                        },
                    },
                )
            })
            .collect();
        let light_peers = LightPeers::new(
            our_node_id,
            Arc::new(RwLock::new(peers_infos)),
            Arc::new(ConnectionQueue::new(0)),
            Some(peers.len()),
        );
        light_peers.write().extend(peers_light);

        // Test case 1: Node closer than all peers
        let close_node = NodeId::from([0x10; 20]); // Very close to our node
        let result = light_peers.check_interesting_node_ids(vec![close_node]);
        assert_eq!(result, vec![close_node], "Should return the closer node");

        // Test case 2: Multiple nodes, some closer some farther
        let very_close_node = NodeId::from([0x05; 20]);
        let very_far_node = NodeId::from([0xFF; 20]);
        let result = light_peers.check_interesting_node_ids(vec![very_far_node, very_close_node]);
        assert_eq!(
            result,
            vec![very_close_node],
            "Should only return the closer node"
        );

        // Test case 3: All nodes farther than peers
        let far_node_1 = NodeId::from([0xFF; 20]);
        let far_node_2 = NodeId::from([0xFE; 20]);
        let result = light_peers.check_interesting_node_ids(vec![far_node_1, far_node_2]);
        assert!(
            result.is_empty(),
            "Should return empty vec when no nodes are closer than peers"
        );

        // Test case 4: Empty input
        let result = light_peers.check_interesting_node_ids(vec![]);
        assert!(result.is_empty(), "Should handle empty input gracefully");

        // Test case 5: Multiple interesting nodes
        let close_node_1 = NodeId::from([0x01; 20]);
        let close_node_2 = NodeId::from([0x02; 20]);
        let nodes_to_check = vec![close_node_1, close_node_2];
        let result = light_peers.check_interesting_node_ids(nodes_to_check);
        assert_eq!(result.len(), 2, "Should return both close nodes");
        assert!(result.contains(&close_node_1));
        assert!(result.contains(&close_node_2));
    }
}
