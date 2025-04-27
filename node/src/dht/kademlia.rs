use crate::dht::{light_errors, Bucket, BucketDht, DhtBuckets, DhtId, LightError};
use crate::id::{ChainId, NodeId};
use crate::message::mail_box::Mail;
use crate::message::SubscribableMessage;
use crate::net::node::NodeError;
use crate::server::msg::InboundMessageExt;
use crate::server::msg::{AppRequestMessage, InboundMessage};
use crate::server::peers::{PeerInfo, PeerSender};
use crate::utils::constants::SNOWFLAKE_HANDLER_ID;
use flume::Sender;
use indexmap::IndexMap;
use proto_lib::{p2p, sdk};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Arc, RwLock, RwLockReadGuard};
use tokio::task::JoinSet;

pub trait LockedMapDb<K, V> {
    fn get(&self, key: &K) -> Option<V>;
    fn insert(&self, key: K, value: V) -> Option<V>;
}

impl<K, V> LockedMapDb<K, V> for RwLock<HashMap<K, V>>
where
    K: Eq + Hash,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        self.read().unwrap().get(key).cloned()
    }

    fn insert(&self, key: K, value: V) -> Option<V> {
        self.write().unwrap().insert(key, value)
    }
}

#[derive(Debug)]
pub struct KademliaDht {
    peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
    light_peers: Arc<RwLock<HashMap<NodeId, DhtBuckets>>>,
    mail_tx: Sender<Mail>,
    chain_id: ChainId,
    /// Maximum number of nodes to return in a `find_node` request.
    max_nodes: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ValueOrNodes<V> {
    Value(V),
    Nodes(Vec<NodeId>),
}

impl KademliaDht {
    pub fn new(
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        light_peers: Arc<RwLock<HashMap<NodeId, DhtBuckets>>>,
        mail_tx: Sender<Mail>,
        chain_id: ChainId,
        max_nodes: usize,
    ) -> Self {
        Self {
            peers_infos,
            light_peers,
            mail_tx,
            chain_id,
            max_nodes,
        }
    }

    fn distance(a: &Bucket, b: Bucket) -> Bucket {
        a ^ b
    }

    /// Find up to `n` unique nodes that are the closest to the `bucket`.
    fn find_closest_nodes(
        &self,
        nodes: RwLockReadGuard<HashMap<NodeId, DhtBuckets>>,
        bucket: &Bucket,
        excluding: &[NodeId],
        mut n: usize,
    ) -> Vec<NodeId> {
        let node_ids = nodes
            .keys()
            .filter(|node_id| !excluding.contains(node_id))
            .collect::<Vec<_>>();

        if node_ids.is_empty() {
            return vec![];
        }

        if n >= node_ids.len() {
            n = node_ids.len() - 1;
        }

        let distances: Vec<Bucket> = node_ids
            .iter()
            .map(|node_id| Self::distance(bucket, Bucket::from_be_bytes((**node_id).into())))
            .collect();

        let mut distances2 = distances.clone();
        let closest_buckets = if n == 0 {
            vec![distances2.into_iter().min().unwrap()]
        } else {
            let (closest_buckets, ..) = distances2.select_nth_unstable(n);
            closest_buckets.to_vec()
        };

        closest_buckets
            .into_iter()
            .map(|bucket| {
                let i = distances.iter().position(|d| *d == bucket).unwrap();
                *node_ids[i]
            })
            .collect()
    }

    /// Find nodes that potentially hold the content at bucket because their k spans it
    /// and complete the list with closest nodes.
    fn find_content_and_closest_nodes(
        &self,
        dht_id: &DhtId,
        bucket: &Bucket,
        excluding: &[NodeId],
        max: usize,
    ) -> Vec<NodeId> {
        let nodes = self.light_peers.read().unwrap();
        let mut nodes_with_content: Vec<_> = nodes
            .iter()
            .filter(|(node_id, _)| !excluding.contains(node_id))
            .filter_map(|(node_id, buckets)| {
                let k = match dht_id {
                    DhtId::Block => buckets.block,
                    _ => todo!(),
                };
                let dht = BucketDht::new(*node_id, k);
                if dht.is_desired_bucket(bucket) {
                    Some(*node_id)
                } else {
                    None
                }
            })
            .take(max)
            .collect();
        if nodes_with_content.len() < max {
            let closest = self
                .find_closest_nodes(nodes, bucket, excluding, max * 2)
                .into_iter()
                .take(max - nodes_with_content.len());
            nodes_with_content.extend(closest);
        }
        nodes_with_content
    }

    /// Find up to `n` unique nodes that are the closest to the `bucket`.
    pub fn find_node(&self, bucket: &Bucket) -> Vec<NodeId> {
        let nodes = self.light_peers.read().unwrap();
        self.find_closest_nodes(nodes, bucket, &[], self.max_nodes)
    }

    /// Recursively search for a value. Once found, it is verified against publicly untrusted data.
    pub async fn search_value(
        &self,
        dht_id: &DhtId,
        bucket: &Bucket,
        max_lookups: usize,
        alpha: usize,
    ) -> Result<Vec<u8>, LightError> {
        log::debug!("searching for value in dht {dht_id:?} at bucket {bucket}");
        let mut excluding = Vec::new();
        let mut worklist = Vec::new();
        for _ in 0..max_lookups {
            let senders = {
                let reserved = loop {
                    let mut reserved = Vec::new();
                    if worklist.is_empty() || reserved.len() >= alpha {
                        break reserved;
                    }
                    reserved.push(worklist.pop().unwrap());
                };
                let mut node_ids = self.find_content_and_closest_nodes(
                    dht_id,
                    bucket,
                    &excluding,
                    alpha.saturating_sub(reserved.len()),
                );
                node_ids.extend(reserved);
                if node_ids.is_empty() {
                    break;
                }
                excluding.extend(node_ids.clone());
                let peers_infos = self.peers_infos.read().unwrap();
                node_ids
                    .into_iter()
                    .filter_map(|node_id| {
                        peers_infos
                            .get(&node_id)
                            .map(|infos| (node_id, infos.sender.clone()))
                    })
                    .collect()
            };
            if let Ok(value_or_nodes) = self.iterative_lookup(dht_id, senders, bucket).await {
                match value_or_nodes {
                    ValueOrNodes::Value(value) => return Ok(value),
                    ValueOrNodes::Nodes(nodes) => {
                        worklist.extend(nodes.clone());
                        excluding.extend(nodes);
                    }
                }
            }
        }
        Err(light_errors::CONTENT_NOT_FOUND)
    }

    async fn iterative_lookup(
        &self,
        dht_id: &DhtId,
        senders: Vec<(NodeId, PeerSender)>,
        bucket: &Bucket,
    ) -> Result<ValueOrNodes<Vec<u8>>, LightError> {
        let mut set = JoinSet::new();
        let dht_id: u32 = dht_id.into();
        let bucket = bucket.to_be_bytes::<20>();

        for (node_id, sender) in senders {
            let bucket = bucket.to_vec();
            if let Ok(p2p::message::Message::AppRequest(app_request)) =
                AppRequestMessage::encode(&self.chain_id, sdk::FindValue { dht_id, bucket })
            {
                let mail_tx = self.mail_tx.clone();
                set.spawn(async move {
                    let handle = sender.send_and_response(
                        &mail_tx,
                        node_id,
                        SubscribableMessage::AppRequest(app_request),
                    )?;
                    let message = handle.await?;
                    Result::<p2p::message::Message, NodeError>::Ok(message)
                });
            }
        }

        let mut nodes = HashSet::new();
        while let Some(result) = set.join_next().await {
            let Ok(Ok(p2p::message::Message::AppResponse(app_response))) = result else {
                continue;
            };
            if app_response.chain_id != self.chain_id.as_ref().to_vec() {
                continue;
            }
            let bytes = app_response.app_bytes;
            let Ok((app_id, bytes)) = unsigned_varint::decode::u64(&bytes) else {
                continue;
            };
            if app_id != SNOWFLAKE_HANDLER_ID {
                continue;
            }
            let Ok(light_message) = InboundMessage::decode(bytes) else {
                continue;
            };
            match light_message {
                sdk::light_response::Message::Value(sdk::Value { value }) => {
                    // TODO verify data using publicly available data and return if successful.
                    // if not successful, disconnect from node and decrease reputation
                    return Ok(ValueOrNodes::Value(value));
                }
                sdk::light_response::Message::Nodes(sdk::Nodes { node_ids }) => {
                    // TODO: only pick nodes that are getting us closer to the bucket
                    if node_ids.len() > 10 {
                        // disconnect and decrease reputation
                        continue;
                    }
                    let node_ids: HashSet<_> = node_ids
                        .into_iter()
                        .filter_map(|node_id| {
                            let arr: Option<[u8; 20]> = node_id.try_into().ok();
                            arr.map(NodeId::from)
                        })
                        .collect();
                    nodes.extend(node_ids);
                }
                sdk::light_response::Message::Ack(_) => {
                    // unexpected response
                    // TODO: disconnect from node and decrease reputation
                    continue;
                }
            }
        }

        if !nodes.is_empty() {
            Ok(ValueOrNodes::Nodes(nodes.into_iter().collect()))
        } else {
            Err(light_errors::CONTENT_NOT_FOUND)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn extend_to_bytes<const N: usize>(arr: [u8; N]) -> [u8; 20] {
        let mut out = [0u8; 20];
        out[20 - arr.len()..].copy_from_slice(&arr);
        out
    }

    fn extend_to_bucket<const N: usize>(arr: [u8; N]) -> Bucket {
        Bucket::from_be_bytes(extend_to_bytes(arr))
    }

    fn extend_to_node_id<const N: usize>(arr: [u8; N]) -> NodeId {
        NodeId::from(extend_to_bytes(arr))
    }

    #[allow(clippy::type_complexity)]
    fn node_ids_to_infos(
        buckets: Vec<[u8; 2]>,
    ) -> (
        Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        Arc<RwLock<HashMap<NodeId, DhtBuckets>>>,
    ) {
        let node_ids = buckets.into_iter().map(extend_to_node_id).collect::<Vec<_>>();
        let peer_infos: IndexMap<_, _> = node_ids
            .iter()
            .map(|node_id| {
                (
                    *node_id,
                    PeerInfo {
                        x509_certificate: vec![],
                        sender: {
                            let (tx, _) = flume::unbounded();
                            tx.into()
                        },
                        infos: None,
                    },
                )
            })
            .collect();
        let light_peers = node_ids
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
        (
            Arc::new(RwLock::new(peer_infos)),
            Arc::new(RwLock::new(light_peers)),
        )
    }

    #[test]
    fn find_node() {
        let buckets = [
            [0, 0b10000000],
            [0, 0b11100000],
            [0, 0b11111000],
            [0, 0b11111110],
            [0, 0b11111111],
            [0, 0b11111100],
            [0, 0b11110000],
            [0, 0b11000000],
            [0, 0b00000010],
        ];
        let (peer_infos, light_peers) = node_ids_to_infos(buckets.to_vec());
        let (mail_tx, _) = flume::unbounded();
        let dht: KademliaDht = KademliaDht::new(peer_infos, light_peers, mail_tx, ChainId::from([0; 32]), 3);
        let closest = dht.find_node(&extend_to_bucket(buckets[4]));
        assert_eq!(closest.len(), 3);
        assert_eq!(
            closest.into_iter().collect::<HashSet<_>>(),
            HashSet::from([
                extend_to_node_id(buckets[3]),
                extend_to_node_id(buckets[4]),
                extend_to_node_id(buckets[5]),
            ])
        );
    }

    // #[test]
    // fn find_value() {
    //     let dht = KademliaDht::new(vec![], 3);
    //     let key = [5, 6];
    //     let value = vec![1, 2, 3, 4];
    //
    //     let uint_key = extend_to_bucket(key);
    //     assert!(matches!(dht.find_value(&uint_key), ValueOrNodes::Nodes(_)));
    //     dht.store(uint_key, value.clone());
    //     assert_eq!(dht.find_value(&uint_key), ValueOrNodes::Value(value));
    // }
}
