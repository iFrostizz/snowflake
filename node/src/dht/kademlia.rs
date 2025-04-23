use crate::dht::{Bucket, BucketDht, DhtBuckets, DhtId, LightError};
use crate::id::{ChainId, NodeId};
use crate::message::mail_box::Mail;
use crate::message::SubscribableMessage;
use crate::net::node::NodeError;
use crate::server::peers::PeerInfo;
use crate::utils::constants;
use flume::Sender;
use indexmap::IndexMap;
use prost::Message;
use proto_lib::{p2p, sdk};
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, RwLock, RwLockReadGuard};
use tokio::task::JoinSet;
use crate::server::msg::{InLightMessage2};

pub trait LockedMapDb<K, V> {
    fn insert(&self, key: K, value: V) -> Option<V>;
    fn get(&self, key: &K) -> Option<V>;
}

impl<K, V> LockedMapDb<K, V> for RwLock<HashMap<K, V>>
where
    K: Eq + Hash,
    V: Clone,
{
    fn insert(&self, key: K, value: V) -> Option<V> {
        self.write().unwrap().insert(key, value)
    }

    fn get(&self, key: &K) -> Option<V> {
        self.read().unwrap().get(key).cloned()
    }
}

#[derive(Debug)]
pub struct KademliaDht {
    peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
    light_peers: Arc<RwLock<HashMap<NodeId, DhtBuckets>>>,
    mail_tx: Sender<Mail>,
    chain_id: ChainId,
    n: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ValueOrNodes<V> {
    Value(V),
    Nodes(Vec<NodeId>),
}

impl ValueOrNodes<Vec<u8>> {
    pub fn encode(self, buf: &mut Vec<u8>) -> Result<(), prost::EncodeError> {
        match self {
            ValueOrNodes::Value(value) => sdk::Value::encode(&sdk::Value { value }, buf),
            ValueOrNodes::Nodes(nodes) => sdk::Nodes::encode(
                &sdk::Nodes {
                    node_ids: nodes
                        .into_iter()
                        .map(|node_id| node_id.as_ref().to_vec())
                        .collect(),
                },
                buf,
            ),
        }
    }
}

impl KademliaDht {
    pub fn new(
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        light_peers: Arc<RwLock<HashMap<NodeId, DhtBuckets>>>,
        mail_tx: Sender<Mail>,
        chain_id: ChainId,
        n: usize,
    ) -> Self {
        Self {
            peers_infos,
            light_peers,
            mail_tx,
            chain_id,
            n,
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
        mut n: usize,
    ) -> Vec<NodeId> {
        if nodes.is_empty() {
            return vec![];
        }

        if n >= nodes.len() {
            n = nodes.len() - 1;
        }

        let distances: Vec<Bucket> = nodes
            .keys()
            .map(|node_id| Self::distance(bucket, Bucket::from_be_bytes((*node_id).into())))
            .collect();

        let mut distances2 = distances.clone();
        let (closest_buckets, ..) = distances2.select_nth_unstable(n);
        let closest_buckets: Vec<_> = closest_buckets.to_vec();

        closest_buckets
            .into_iter()
            .map(|bucket| bucket.to_be_bytes().into())
            .collect()
    }

    /// Find nodes that potentially hold the content at bucket because their k spans it
    /// and complete the list with closest nodes.
    fn find_content_and_closest_nodes(&self, dht_id: &DhtId, bucket: &Bucket) -> Vec<NodeId> {
        let nodes = self.light_peers.read().unwrap();
        let mut nodes_with_content: Vec<_> = nodes
            .iter()
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
            .take(self.n)
            .collect();
        if nodes_with_content.len() < self.n {
            let closest = self
                .find_closest_nodes(nodes, bucket, self.n * 2)
                .into_iter()
                .take(self.n - nodes_with_content.len());
            nodes_with_content.extend(closest);
        }
        nodes_with_content
    }

    /// Find up to `n` unique nodes that are the closest to the `bucket`.
    pub fn find_node(&self, bucket: &Bucket) -> Vec<NodeId> {
        let nodes = self.light_peers.read().unwrap();
        self.find_closest_nodes(nodes, bucket, self.n)
    }

    pub async fn search_value(
        &self,
        dht_id: DhtId,
        bucket: &Bucket,
    ) -> Result<Vec<u8>, LightError> {
        // TODO should allow to exclude already-queried nodes in the find_content_and_closest_nodes function
        let mut set = JoinSet::new();

        {
            let nodes = self.find_content_and_closest_nodes(&dht_id, bucket);
            let peers_infos = self.peers_infos.read().unwrap();

            for node_id in nodes {
                if let Some(info) = peers_infos.get(&node_id) {
                    let sender = info.sender.clone();
                    let chain_id = self.chain_id.as_ref().to_vec();
                    let mut bytes = unsigned_varint::encode::u64_buffer();
                    let bytes =
                        unsigned_varint::encode::u64(constants::SNOWFLAKE_HANDLER_ID, &mut bytes);
                    let mut app_bytes = bytes.to_vec();
                    let dht: u32 = (&dht_id).into();
                    let bucket = bucket.to_be_bytes::<20>().to_vec();
                    let find_value = sdk::FindValue { dht, bucket };
                    find_value.encode(&mut app_bytes).expect("should not fail");
                    let message = p2p::AppRequest {
                        chain_id,
                        request_id: rand::random(),
                        deadline: constants::DEFAULT_DEADLINE,
                        app_bytes,
                    };
                    let mail_tx = self.mail_tx.clone();
                    set.spawn(async move {
                        let handle = sender.send_and_response(
                            &mail_tx,
                            node_id,
                            SubscribableMessage::AppRequest(message),
                        )?;
                        Result::<p2p::message::Message, NodeError>::Ok(handle.await?)
                    });
                }
            }
        }

        while let Some(result) = set.join_next().await {
            let Ok(Ok(p2p::message::Message::AppResponse(response))) = result else {
                continue;
            };
            if response.chain_id != self.chain_id.as_ref().to_vec() {
                continue;
            }
            let Ok(light_message) = InLightMessage2::decode(&response.app_bytes) else {
                continue;
            };
            match light_message {
                sdk::light_response::Message::Value(sdk::Value { value }) => {
                    return Ok(value);
                }
                sdk::light_response::Message::Nodes(sdk::Nodes {node_ids}) => {
                    // recursively search value
                    todo!()
                }
                sdk::light_response::Message::Ack(_) => {
                    continue;
                }
            }
        }

        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn extend_to_bytes<const N: usize>(arr: [u8; N]) -> [u8; 20] {
        let mut out = [0u8; 20];
        out[..arr.len()].copy_from_slice(&arr);
        out
    }

    fn extend_to_bucket<const N: usize>(arr: [u8; N]) -> Bucket {
        Bucket::from_be_bytes(extend_to_bytes(arr))
    }

    fn extend_to_node_id<const N: usize>(arr: [u8; N]) -> NodeId {
        NodeId::from(extend_to_bytes(arr))
    }

    #[test]
    fn find_node() {
        let buckets = [
            [0, 0b00000000],
            [0, 0b10000001],
            [0, 0b11000011],
            [0, 0b11100111],
            [0, 0b11111111],
            [0, 0b11110111],
            [0, 0b11000111],
            [0, 0b00000111],
            [0, 0b0000001],
        ];
        let node_ids = buckets
            .into_iter()
            .map(extend_to_node_id)
            .collect::<Vec<_>>();
        let dht: KademliaDht = KademliaDht::new(node_ids, 3);
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

    #[test]
    fn find_value() {
        let dht = KademliaDht::new(vec![], 3);
        let key = [5, 6];
        let value = vec![1, 2, 3, 4];

        let uint_key = extend_to_bucket(key);
        assert!(matches!(dht.find_value(&uint_key), ValueOrNodes::Nodes(_)));
        dht.store(uint_key, value.clone());
        assert_eq!(dht.find_value(&uint_key), ValueOrNodes::Value(value));
    }
}
