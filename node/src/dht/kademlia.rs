use crate::dht::block::DhtBlocks;
use crate::dht::{light_errors, Bucket, BucketDht, ConcreteDht, DhtBuckets, DhtId, LightError};
use crate::id::{ChainId, NodeId};
use crate::message::mail_box::Mail;
use crate::message::SubscribableMessage;
use crate::net::light::DhtCodex;
use crate::net::light::LightPeers;
use crate::net::queue::ConnectionData;
use crate::server::msg::AppRequestMessage;
use crate::server::peers::{PeerInfo, PeerSender};
use crate::utils::constants;
use crate::utils::twokhashmap::{CompositeKey, DoubleKeyedHashMap};
use crate::utils::unpacker::StatelessBlock;
use alloy::primitives::FixedBytes;
use flume::Sender;
use indexmap::IndexMap;
use proto_lib::{p2p, sdk};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock, RwLockReadGuard};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinSet;

pub trait LockedMapDb<V> {
    type Key;
    fn get(&self, key: &Self::Key) -> Option<V>;
    fn get_bucket(&self, bucket: &Bucket) -> Option<V>;
    fn insert(&self, key: Self::Key, value: V);
}

impl ConcreteDht for CompositeKey<u64, FixedBytes<32>> {
    fn to_bucket(&self) -> Bucket {
        match self {
            CompositeKey::First(k1) => k1.to_bucket(),
            CompositeKey::Second(k2) => k2.to_bucket(),
            CompositeKey::Both(..) => panic!("invalid key"),
        }
    }
}

impl<V> LockedMapDb<V> for RwLock<HashMap<Bucket, V>>
where
    V: Clone,
{
    type Key = u64;

    fn get(&self, key: &Self::Key) -> Option<V> {
        self.get_bucket(&key.to_bucket())
    }

    fn get_bucket(&self, bucket: &Bucket) -> Option<V> {
        self.read().unwrap().get(bucket).cloned()
    }

    fn insert(&self, key: Self::Key, value: V) {
        self.write().unwrap().insert(key.to_bucket(), value);
    }
}

impl<V> LockedMapDb<V> for RwLock<DoubleKeyedHashMap<Bucket, Bucket, V>>
where
    V: Clone,
{
    type Key = CompositeKey<u64, FixedBytes<32>>;

    fn get(&self, key: &Self::Key) -> Option<V> {
        let read = self.read().unwrap();
        let value = match key {
            CompositeKey::First(k1) => read.get1(&k1.to_bucket()),
            CompositeKey::Second(k2) => read.get2(&k2.to_bucket()),
            CompositeKey::Both(k1, k2) =>
            {
                #[allow(clippy::manual_map)]
                if let Some(v) = read.get1(&k1.to_bucket()) {
                    Some(v)
                } else if let Some(v) = read.get2(&k2.to_bucket()) {
                    Some(v)
                } else {
                    None
                }
            }
        };
        value.cloned()
    }

    fn get_bucket(&self, key: &Bucket) -> Option<V> {
        let read = self.read().unwrap();
        read.get1(key).or_else(|| read.get2(key)).cloned()
    }

    fn insert(&self, key: Self::Key, value: V) {
        let CompositeKey::Both(k1, k2) = key else {
            panic!("invalid key")
        };
        self.write()
            .unwrap()
            .insert(k1.to_bucket(), k2.to_bucket(), value)
    }
}

#[derive(Debug, Clone)]
pub struct KademliaDht {
    pub peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
    light_peers: LightPeers,
    pub mail_tx: Sender<Mail>,
    pub verification_tx: Sender<(StatelessBlock, oneshot::Sender<bool>)>,
    pub chain_id: ChainId,
    /// Maximum number of nodes to return in a `find_node` request.
    max_nodes: usize,
    node_id: NodeId,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ValueOrNodes<V> {
    Value(V),
    Nodes(Vec<ConnectionData>),
}

impl KademliaDht {
    pub fn new(
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        light_peers: LightPeers,
        mail_tx: Sender<Mail>,
        verification_tx: Sender<(StatelessBlock, oneshot::Sender<bool>)>,
        chain_id: ChainId,
        max_nodes: usize,
        node_id: NodeId,
    ) -> Self {
        Self {
            peers_infos,
            light_peers,
            mail_tx,
            verification_tx,
            chain_id,
            max_nodes,
            node_id,
        }
    }

    pub fn distance(a: &Bucket, b: Bucket) -> Bucket {
        a ^ b
    }

    /// Find up to `n` unique nodes that are the closest to the `bucket`.
    fn find_closest_nodes(
        &self,
        nodes: RwLockReadGuard<IndexMap<NodeId, DhtBuckets>>,
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

    /// Lookup locally for nodes spanning the bucket.
    /// Find up to `n` unique nodes that are the closest to the `bucket`.
    pub fn find_node(&self, bucket: &Bucket) -> Vec<ConnectionData> {
        let nodes = self.light_peers.read().unwrap();
        let node_ids = self.find_closest_nodes(nodes, bucket, &[], self.max_nodes);
        self.map_to_connection_data(node_ids)
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
        let mut lookups_remaining = max_lookups;
        while lookups_remaining > 0 {
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
            if let Ok(value_or_nodes) = self
                .iterative_lookup(dht_id, senders, bucket, &mut lookups_remaining)
                .await
            {
                match value_or_nodes {
                    ValueOrNodes::Value(value) => return Ok(value),
                    ValueOrNodes::Nodes(connections_data) => {
                        let nodes: Vec<_> = connections_data
                            .into_iter()
                            .filter_map(|c| {
                                if c.node_id != self.node_id {
                                    Some(c)
                                } else {
                                    None
                                }
                            })
                            .map(|c| c.node_id)
                            .collect();
                        worklist.extend(nodes.clone());
                        excluding.extend(nodes);
                    }
                }
            }
        }
        Err(light_errors::CONTENT_NOT_FOUND)
    }

    pub async fn store(
        &self,
        node_id: NodeId,
        dht_id: &DhtId,
        value: Vec<u8>,
    ) -> Result<(), LightError> {
        let sender = self
            .peers_infos
            .read()
            .unwrap()
            .get(&node_id)
            .map(|infos| infos.sender.tx.clone())
            .ok_or(light_errors::PEER_MISSING)?;
        let message = AppRequestMessage::encode(
            &self.chain_id,
            sdk::Store {
                dht_id: dht_id.into(),
                value,
            },
        )
        .unwrap();
        sender.send(message).map_err(|_| light_errors::PEER_MISSING)
    }

    async fn iterative_lookup(
        &self,
        dht_id: &DhtId,
        senders: Vec<(NodeId, PeerSender)>,
        bucket: &Bucket,
        lookups_remaining: &mut usize,
    ) -> Result<ValueOrNodes<Vec<u8>>, LightError> {
        let mut set = JoinSet::new();
        let dht_id: u32 = dht_id.into();
        let bucket = bucket.to_be_bytes::<20>();

        for (_, sender) in senders {
            if *lookups_remaining > 0 {
                let bucket = bucket.to_vec();
                if let Ok(p2p::message::Message::AppRequest(app_request)) =
                    AppRequestMessage::encode(&self.chain_id, sdk::FindValue { dht_id, bucket })
                {
                    let mail_tx = self.mail_tx.clone();
                    let chain_id = self.chain_id;
                    set.spawn(async move {
                        sender
                            .send_and_app_response(
                                chain_id,
                                constants::SNOWFLAKE_HANDLER_ID,
                                &mail_tx,
                                SubscribableMessage::AppRequest(app_request),
                            )
                            .await
                    });
                    *lookups_remaining -= 1;
                }
            } else {
                break;
            }
        }

        let mut nodes = HashSet::new();
        while let Some(result) = set.join_next().await {
            let Ok(Ok(light_message)) = result else {
                continue;
            };
            match light_message {
                sdk::light_response::Message::Value(sdk::Value { value }) => {
                    if let Ok(block) = DhtBlocks::decode(&value) {
                        let (tx, rx) = oneshot::channel();
                        self.verification_tx.send((block, tx)).unwrap();
                        if rx.await.unwrap() {
                            return Ok(ValueOrNodes::Value(value));
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                sdk::light_response::Message::Nodes(p2p::PeerList { claimed_ip_ports }) => {
                    // TODO: only pick nodes that are getting us closer to the bucket.
                    //  For that, don't only wait for the connection but also the light handshake.
                    if claimed_ip_ports.len() > 10 {
                        // disconnect and decrease reputation
                        continue;
                    }
                    let cds = claimed_ip_ports
                        .into_iter()
                        .filter_map(|claimed_ip_port| claimed_ip_port.try_into().ok())
                        .collect();
                    // TODO this will be ineffective if the peer max is reached.
                    //  We should allow either going over the limit for a short time and disconnect
                    //  right away or replace other peers while making sure that we are not
                    //  in the middle of a conversation. We can achieve this by attaching some
                    //  kind of mutex on peers. If we do this, it will play well with the timeout
                    //  because it could wait for ending the conversation with the first furthest
                    //  peers before disconnecting from it.
                    let cds = self.connect_to_light_nodes(cds).await;
                    nodes.extend(cds);
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

    async fn connect_to_light_nodes(
        &self,
        cds: HashSet<ConnectionData>,
    ) -> HashSet<ConnectionData> {
        let timeout = Duration::from_secs(5);
        tokio::time::timeout(timeout, async move {
            let mut set = JoinSet::new();
            for cd in cds {
                let connection_queue = self.light_peers.connection_queue.clone();
                set.spawn(async move {
                    (
                        cd.clone(),
                        connection_queue.wait_for_connection(cd, 0).await,
                    )
                });
            }
            let mut res = HashSet::new();
            while let Some(data) = set.join_next().await {
                if let Ok((cd, connected)) = data {
                    if connected {
                        res.insert(cd);
                    }
                }
            }
            res
        })
        .await
        .unwrap_or_else(|_| HashSet::new())
    }

    fn map_to_connection_data(&self, node_ids: Vec<NodeId>) -> Vec<ConnectionData> {
        let peers_infos = self.peers_infos.read().unwrap();
        node_ids
            .into_iter()
            .filter_map(|node_id| {
                peers_infos.get(&node_id).and_then(|peer_info| {
                    peer_info.infos.as_ref().map(|infos| ConnectionData {
                        node_id,
                        socket_addr: infos.sock_addr,
                        timestamp: infos.ip_signing_time,
                        x509_certificate: peer_info.x509_certificate.clone(),
                    })
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast;
    use crate::net::queue::ConnectionQueue;
    use crate::net::HandshakeInfos;
    use std::collections::HashSet;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

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
        HashMap<NodeId, DhtBuckets>,
    ) {
        let node_ids = buckets
            .into_iter()
            .map(extend_to_node_id)
            .collect::<Vec<_>>();
        let peer_infos: IndexMap<_, _> = node_ids
            .clone()
            .into_iter()
            .map(|node_id| {
                (
                    node_id,
                    PeerInfo {
                        x509_certificate: vec![],
                        sender: {
                            let (tx, _) = flume::unbounded();
                            PeerSender { tx, node_id }
                        },
                        infos: Some(HandshakeInfos {
                            ip_signing_time: 0,
                            network_id: 0,
                            sock_addr: SocketAddr::new(
                                IpAddr::from(Ipv4Addr::from([0, 0, 0, 0])),
                                0,
                            ),
                            ip_node_id_sig: vec![],
                            client: None,
                            tracked_subnets: vec![],
                            supported_acps: vec![],
                            objected_acps: vec![],
                        }),
                        tx: {
                            let (tx, _) = broadcast::channel(1);
                            tx
                        },
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
        (Arc::new(RwLock::new(peer_infos)), light_peers)
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
        let (peer_infos, light_peers_data) = node_ids_to_infos(buckets.to_vec());
        let (mail_tx, _) = flume::unbounded();
        let light_peers = LightPeers::new(
            Default::default(),
            peer_infos.clone(),
            Arc::new(ConnectionQueue::new(0)),
            Some(light_peers_data.len()),
        );
        light_peers.write().extend(light_peers_data);
        let (verification_tx, _) = flume::unbounded();
        let dht: KademliaDht = KademliaDht::new(
            peer_infos,
            light_peers,
            mail_tx,
            verification_tx,
            ChainId::from([0; 32]),
            3,
            Default::default(),
        );
        let closest = dht.find_node(&extend_to_bucket(buckets[4]));
        assert_eq!(closest.len(), 3);
        assert_eq!(
            closest
                .into_iter()
                .map(|infos| infos.node_id)
                .collect::<HashSet<_>>(),
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
