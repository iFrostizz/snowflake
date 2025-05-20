use crate::client::config;
use crate::dht::{light_errors, DhtBuckets, LightError};
use crate::id::{ChainId, NodeId};
use crate::message::mail_box::MailBox;
use crate::message::{pipeline::Pipeline, MiniMessage, SubscribableMessage};
use crate::net::light::{DhtContent, LightNetwork, LightNetworkConfig};
use crate::net::queue::ConnectionQueue;
use crate::net::{ip::UnsignedIp, BackoffParams, Intervals, Network, PeerInfo};
use crate::node::{MessageOrSubscribable, SinglePickerConfig};
use crate::server::msg::AppRequestMessage;
use crate::server::{
    msg::{DecodingError, OutboundMessage},
    peers::PeerSender,
    tcp::write_stream_message,
};
use crate::stats;
use crate::utils::constants::DEFAULT_DEADLINE;
use crate::utils::twokhashmap::CompositeKey;
use crate::utils::unpacker::StatelessBlock;
use crate::utils::{
    bloom::{BloomError, Filter},
    bls::Bls,
    ip::ip_octets,
};
use flume::{Receiver, Sender};
use futures::future;
use indexmap::IndexMap;
use prost::EncodeError;
use proto_lib::p2p::message::Message;
use proto_lib::p2p::{
    self, Accepted, EngineType, Get, GetAccepted, GetAcceptedFrontier, GetAncestors,
};
use proto_lib::sdk;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::RwLockWriteGuard;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::io::WriteHalf;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, oneshot, Semaphore};
use tokio::task::JoinHandle;
use tokio::time;
use tokio_rustls::TlsStream;

#[derive(Debug, Error)]
pub enum NodeError {
    #[error("dns conversion failed")]
    Dns,
    #[error("future timeout: {0}")]
    Timeout(#[from] time::error::Elapsed),
    #[error("tcp error: {0}")]
    TcpConnection(#[from] std::io::Error),
    #[error(transparent)]
    SendError(#[from] SendErrorWrapper),
    #[error("recv error: all sender have been dropped")]
    RecvError(#[from] oneshot::error::RecvError),
    #[error("error when decoding inbound message {0}")]
    Decoding(#[from] DecodingError),
    #[error("error when encoding outbound message {0}")]
    Encoding(#[from] EncodeError),
    #[error("bootstrapping error(s): {0:?}")]
    Bootstrap(Vec<NodeError>),
    #[error("connection failed after retries, reasons: {0:?}")]
    Failed(Vec<NodeError>),
    #[error("bloom filter generation: {0}")]
    Bloom(#[from] BloomError),
    #[error("unwanted peer: reason: {0}")]
    UnwantedPeer(#[from] AddPeerError),
    #[error("openssl error: {0}")]
    OpenSsl(#[from] openssl::error::ErrorStack),
    #[error("unexpected message: {0}")]
    Message(String),
}

#[derive(Debug)]
pub struct SendErrorWrapper;

impl<T> From<flume::SendError<T>> for SendErrorWrapper {
    fn from(_: flume::SendError<T>) -> Self {
        SendErrorWrapper
    }
}

impl std::fmt::Display for SendErrorWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "send error: all receivers have been dropped")
    }
}

impl std::error::Error for SendErrorWrapper {}

#[derive(Debug)]
pub struct NetworkConfig {
    /// This node socket address
    pub socket_addr: SocketAddr,
    pub network_id: u32,
    pub eth_network_id: u64,
    pub c_chain_id: ChainId,
    pub pem_key_path: PathBuf,
    pub bls_key_path: PathBuf,
    pub cert_path: PathBuf,
    pub intervals: Intervals,
    pub back_off: BackoffParams,
    // in B/s
    pub max_throughput: u32,
    pub max_out_queue_size: usize,
    pub bucket_size: usize,
    pub max_concurrent_handshakes: usize,
    pub max_peers: Option<usize>,
    pub max_light_peers: Option<usize>,
    pub bootstrappers: HashMap<NodeId, Option<DhtBuckets>>,
    pub dht_buckets: DhtBuckets,
    pub max_latency_records: usize,
    pub max_out_connections: usize,
    pub sync_headers: bool,
}

#[derive(Debug)]
pub struct WriteMessage(Vec<u8>);

impl WriteMessage {
    pub fn size(&self) -> usize {
        self.0.len()
    }
}

impl From<Vec<u8>> for WriteMessage {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub struct WriteHandler(Sender<Vec<u8>>, MiniMessage);

impl WriteHandler {
    pub async fn handle_message(self, message: WriteMessage) {
        let Self(tx, mini) = self;
        let bytes = message.0;
        mini.inc_sent(bytes.len() as u64);
        let _ = tx.send(bytes);
    }
}

#[derive(Debug, Error)]
pub enum AddPeerError {
    #[error("cannot add self")]
    AddSelf,
    #[error("already connected")]
    AlreadyConnected,
    #[error("max peers reached")]
    MaxPeersReached,
}

impl Network {
    /// Initiate the network by specifying this node's IP
    pub fn new(
        config: NetworkConfig,
        node_id: NodeId,
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
    ) -> Result<Self, NodeError> {
        let client_config = Arc::new(config::client_config(
            &config.cert_path,
            &config.pem_key_path,
        ));

        let bls = Bls::new(&config.bls_key_path);
        let public_key = bls.public_key();
        let node_pop = bls.sign_pop(&public_key);

        let sig_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let unsigned_ip = UnsignedIp::new(
            config.socket_addr.ip(),
            config.socket_addr.port(),
            sig_timestamp,
        );
        let signed_ip = unsigned_ip.sign_with_key(&bls, &config.pem_key_path)?;

        // TODO https://github.com/iFrostizz/snowflake/issues/13
        let client = p2p::Client {
            name: String::from("avalanchego"),
            major: 1,
            minor: 13,
            patch: 0,
        };

        let bloom_filter = Filter::new(8, 1000).expect("usage of wrong constants");
        let bloom_filter = RwLock::new(bloom_filter);

        let out_pipeline = Arc::new(Pipeline::new(
            config.max_throughput,
            config.max_out_queue_size,
            config.bucket_size,
        ));

        let mail_box = Arc::new(MailBox::new(config.max_latency_records));

        let handshake_semaphore = Arc::new(Semaphore::new(config.max_concurrent_handshakes));
        let bootstrappers = RwLock::new(config.bootstrappers.clone());

        let connection_queue = Arc::new(ConnectionQueue::new(config.max_out_connections));
        let (verification_tx, verification_rx) = flume::unbounded();
        let light_network = LightNetwork::new(
            node_id,
            peers_infos.clone(),
            connection_queue.clone(),
            mail_box.tx().clone(),
            verification_tx,
            config.c_chain_id,
            LightNetworkConfig {
                max_lookups: 10,
                alpha: 3,
                dht_buckets: config.dht_buckets.clone(),
                max_light_peers: config.max_light_peers,
            },
        );
        let light_network = Arc::new(light_network);

        Ok(Self {
            node_id,
            out_pipeline,
            connection_queue,
            config,
            client,
            client_config,
            peers_infos,
            bootstrappers,
            signed_ip,
            bloom_filter,
            public_key,
            node_pop,
            handshake_semaphore,
            mail_box,
            light_network,
            verification_rx,
        })
    }

    /// Continuously write messages and return an error on an EOF
    pub async fn schedule_write_messages(
        out_pipeline: Arc<Pipeline>,
        mut write: WriteHalf<TlsStream<TcpStream>>,
        rnp: Receiver<Message>,
        mut disconnection_rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        let (ptx, prx) = flume::unbounded();

        let (write_tx, mut rx) = oneshot::channel();
        let write_messages = tokio::spawn(async move {
            let prx = &prx;
            loop {
                tokio::select! {
                    maybe_bytes = prx.recv_async() => {
                        if let Ok(bytes) = maybe_bytes {
                            write_stream_message(&mut write, bytes).await?;
                        }
                    }
                    _ = &mut rx => {
                        break Ok(())
                    }
                }
            }
        });

        let (queue_tx, mut rx) = oneshot::channel();
        let queue_messages = tokio::spawn(async move {
            let rnp = &rnp;
            let ptx = &ptx;
            loop {
                tokio::select! {
                    maybe_message = rnp.recv_async() => {
                        if let Ok(message) = maybe_message {
                            log::trace!("sending message {message:?}");
                            let mini = MiniMessage::from(&message);
                            if let Ok(bytes) = OutboundMessage::encode(message) {
                                out_pipeline.queue_message(bytes.into(), WriteHandler(ptx.clone(), mini)).await;
                            }
                        }
                    }
                    _ = &mut rx => {
                        break Ok(());
                    }
                }
            }
        });

        let ret = tokio::select! {
            (res, ..) = future::select_all(vec![write_messages, queue_messages]) => {
                res.expect("schedule task panicked!")
            }
            _ = disconnection_rx.recv() => {
                Ok(())
            }
        };

        let _ = write_tx.send(());
        let _ = queue_tx.send(());

        ret
    }

    pub async fn add_peer(
        self: &Arc<Network>,
        node_id: NodeId,
        x509_certificate: Vec<u8>,
        snp: PeerSender,
        tx: broadcast::Sender<()>,
    ) {
        let mut peers = self.peers_infos.write().unwrap();
        if peers.get(&node_id).is_none() {
            peers.insert(
                node_id,
                PeerInfo {
                    x509_certificate,
                    sender: snp, // TODO issue here, the passed snp won't be used if already here
                    infos: None,
                    tx,
                },
            );
            stats::connected_peers::inc();
        } else {
            log::error!("trying to double-add a peer {}", &node_id);
        }
    }

    pub fn handshake_peer(
        self: &Arc<Network>,
        sender: &PeerSender,
        node_id: NodeId,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<JoinHandle<Result<(), NodeError>>, NodeError> {
        self.handshake(sender)?;

        let network = self.clone();
        let sender = sender.clone();
        let hand_peer = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(2000));
            let mut i = 0;

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let peer_infos = network.peers_infos.read().unwrap();
                        let is_handshook = peer_infos.get(&node_id).is_some_and(|peer| peer.handshook());
                        if i < 5 && is_handshook {
                            break;
                        } else if i >= 5 {
                            return Err(NodeError::Message("handshake expired".to_string()));
                        }
                        i += 1;
                    }
                    _ = rx.recv() => {
                        return Ok(())
                    }
                }
            }

            network.light_handshake(&sender)?;

            // the handshake was successful, the channel can still stop this thread remotely
            rx.recv()
                .await
                .map_err(|_| NodeError::Message("recv error".to_string()))?;
            Ok(())
        });

        Ok(hand_peer)
    }

    fn handshake(&self, sender: &PeerSender) -> Result<(), NodeError> {
        let network = &self;
        let bloom_filter = network.bloom_filter.read().unwrap().as_proto();
        let handshake = p2p::Handshake {
            network_id: network.config.network_id,
            my_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            ip_addr: ip_octets(network.config.socket_addr.ip()),
            ip_port: network.config.socket_addr.port().into(),
            ip_signing_time: network.signed_ip.unsigned_ip.timestamp,
            ip_node_id_sig: network.signed_ip.ip_sig.clone(),
            tracked_subnets: Vec::new(),
            client: Some(network.client.clone()),
            supported_acps: vec![23, 24, 25, 30, 31, 41, 62],
            objected_acps: Vec::new(),
            known_peers: Some(bloom_filter),
            ip_bls_sig: network.signed_ip.ip_bls_sig.clone(),
        };

        log::trace!("handshaking the peer");
        sender.send(Message::Handshake(handshake))
    }

    fn light_handshake(&self, sender: &PeerSender) -> Result<(), NodeError> {
        let buckets = (&self.config.dht_buckets).into();
        let message = sdk::light_request::Message::LightHandshake(sdk::LightHandshake {
            buckets: Some(buckets),
        });
        let app_request = AppRequestMessage::encode(&self.config.c_chain_id, message)?;
        sender.send(app_request)
    }

    pub fn remove_peers(
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        light_peers: &mut RwLockWriteGuard<IndexMap<NodeId, DhtBuckets>>,
        node_ids_errs: Vec<(NodeId, Option<NodeError>)>,
    ) {
        {
            for (node_id, _) in &node_ids_errs {
                light_peers.swap_remove(node_id);
            }
        }

        {
            let mut peers_write = peers_infos.write().unwrap();

            for (node_id, _) in &node_ids_errs {
                if let Some(peer) = peers_write.swap_remove(node_id) {
                    let _ = peer.tx.send(());
                    if peer.handshook() {
                        stats::handshook_peers::dec();
                    }
                    stats::connected_peers::dec();
                }
            }
        }

        for (node_id, err) in &node_ids_errs {
            if let Some(err) = err {
                log::debug!("removing peer {}, reason: {}", node_id, err);
            } else {
                log::debug!("removing peer {} for an unknown reason", node_id);
            }
        }
    }

    pub fn disconnect_peer(
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        light_peers: &mut RwLockWriteGuard<IndexMap<NodeId, DhtBuckets>>,
        node_id: NodeId,
        err: Option<NodeError>,
    ) {
        Self::remove_peers(peers_infos, light_peers, vec![(node_id, err)]);
    }

    pub async fn verify_block(
        self: &Arc<Network>,
        stateless_block: &StatelessBlock,
    ) -> Result<bool, LightError> {
        let block = stateless_block.block();
        let number = u64::from_be_bytes(*block.header.number());
        log::debug!("verifying block {}", number);
        let hash = block.hash;
        if self
            .light_network
            .block_dht
            .get_from_store(CompositeKey::Both(number, hash))?
            .is_some()
        {
            return Ok(true);
        }
        let block_id = *stateless_block.id();
        if self
            .light_network
            .block_dht
            .verified_blocks
            .read()
            .unwrap()
            .contains(&block_id)
        {
            return Ok(true);
        }
        let bootstrapper = self.pick_random_bootstrapper().await;
        let message = SubscribableMessage::GetAccepted(GetAccepted {
            chain_id: self.config.c_chain_id.as_ref().to_vec(),
            request_id: rand::random(),
            deadline: DEFAULT_DEADLINE,
            container_ids: vec![block_id.as_ref().to_vec()],
        });
        let res = dbg!(self
            .send_to_peer(&MessageOrSubscribable::Subscribable(message), bootstrapper)
            .await);
        if let Some(Message::Accepted(Accepted {
            chain_id,
            container_ids,
            ..
        })) = res
        {
            if chain_id != self.config.c_chain_id.as_ref().to_vec() {
                return Err(light_errors::INVALID_CONTENT);
            }
            if container_ids == vec![block_id.as_ref().to_vec()] {
                self.light_network
                    .block_dht
                    .verified_blocks
                    .write()
                    .unwrap()
                    .insert(block_id);
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(light_errors::INVALID_CONTENT)
        }
    }

    pub fn has_reached_max_peers(&self, peers_infos: &IndexMap<NodeId, PeerInfo>) -> bool {
        match self.config.max_peers {
            Some(max_peers) => peers_infos.len() >= max_peers,
            None => false,
        }
    }

    pub fn check_add_peer(&self, node_id: &NodeId) -> Result<(), NodeError> {
        if &self.node_id == node_id {
            return Err(AddPeerError::AddSelf.into());
        }

        let peers_infos = self.peers_infos.read().unwrap();
        if peers_infos.contains_key(node_id) {
            return Err(AddPeerError::AlreadyConnected.into());
        }

        if self.is_bootstrapper(node_id) {
            return Ok(());
        }

        if self.has_reached_max_peers(&peers_infos) {
            return Err(AddPeerError::MaxPeersReached.into());
        }
        Ok(())
    }

    pub(crate) async fn pick_random_bootstrapper(self: &Arc<Network>) -> NodeId {
        let mut maybe_bootstrapper = Network::pick_peer(
            &self.peers_infos,
            &self.bootstrappers,
            SinglePickerConfig::Bootstrapper,
        );
        while maybe_bootstrapper.is_none() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            maybe_bootstrapper = Network::pick_peer(
                &self.peers_infos,
                &self.bootstrappers,
                SinglePickerConfig::Bootstrapper,
            );
        }
        maybe_bootstrapper.unwrap()
    }

    pub async fn start_light_network(
        self: Arc<Network>,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        let network = self.clone();
        let peer_bootstrap_process = tokio::spawn(network.sync_blocks());

        if self.config.sync_headers {
            let sync_handle = tokio::spawn(self.bootstrap_headers());

            tokio::select! {
                _ = sync_handle => {},
                _ = peer_bootstrap_process => {},
                _ = rx.recv() => {},
            }
        } else {
            tokio::select! {
                _ = peer_bootstrap_process => {},
                _ = rx.recv() => {},
            }
        }

        Ok(())
    }

    pub async fn bootstrap_headers(self: Arc<Network>) {
        let chain_id = self.config.c_chain_id.as_ref().to_vec();

        loop {
            let mut bootstrapper = self.pick_random_bootstrapper().await;

            let message = SubscribableMessage::GetAcceptedFrontier(GetAcceptedFrontier {
                chain_id: chain_id.clone(),
                request_id: rand::random(),
                deadline: DEFAULT_DEADLINE,
            });
            let Some(Message::AcceptedFrontier(message)) = self
                .send_to_peer(
                    &MessageOrSubscribable::Subscribable(message.clone()),
                    bootstrapper,
                )
                .await else {
                continue;
            };

            let mut last_container_id = dbg!(message.container_id);
            'outer: loop {
                let message = SubscribableMessage::GetAncestors(GetAncestors {
                    chain_id: chain_id.clone(),
                    request_id: rand::random(),
                    deadline: DEFAULT_DEADLINE,
                    container_id: last_container_id.clone(),
                    engine_type: EngineType::Snowman.into(),
                });
                let Some(Message::Ancestors(message)) = self
                    .send_to_peer(
                        &MessageOrSubscribable::Subscribable(message.clone()),
                        bootstrapper,
                    )
                    .await else {
                    continue;
                };

                let len = message.containers.len();
                log::debug!("Syncing {} containers", len);
                if len == 0 {
                    break 'outer;
                }

                for (i, container) in message.containers.into_iter().enumerate() {
                    match StatelessBlock::unpack(container) {
                        Ok(block) => {
                            dbg!(&block.block().header.number());
                            let block_dht = &self.light_network.block_dht;
                            block_dht
                                .verified_blocks
                                .write()
                                .unwrap()
                                .insert(*block.id());
                            if let Err(err) = block_dht.insert_to_store(block.bytes().to_vec()) {
                                log::error!("Failed to store block: {:?}", err);
                            }
                            if block_dht.next_block_to_store().is_ok_and(|n| &n.to_be_bytes() == block.block().header.number()) {
                                break 'outer;
                            } else if i == len - 1 {
                                if block.block().header.number() == &[0; 8] {
                                    break 'outer;
                                }
                                last_container_id = block.id().as_ref().to_vec();
                            }
                            bootstrapper = self.pick_random_bootstrapper().await;
                        }
                        Err(err) => log::error!("error deserializing block: {:?}", err),
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    /// Sync headers from peers using the Kademlia DHT
    pub(crate) async fn sync_blocks(self: Arc<Self>) {
        loop {
            if let Ok(last_block) = self.latest_block().await {
                let light_network = &self.light_network;
                let blocks = light_network.block_dht.bucket_to_number_iter(last_block);
                let n = *light_network.block_dht.min_stored_blocks.lock().unwrap();
                for number in blocks {
                    if number >= n {
                        if let Ok(block) = light_network
                            .find_content(&light_network.block_dht, CompositeKey::First(number))
                            .await
                        {
                            log::debug!("Found block {}", number);
                            if let Err(err) = light_network.block_dht.store_block_if_desired(block)
                            {
                                log::error!("Failed to store block: {}", err);
                            }
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    pub async fn latest_block(self: &Arc<Self>) -> Result<u64, NodeError> {
        let chain_id = self.config.c_chain_id.as_ref().to_vec();
        let bootstrapper = self.pick_random_bootstrapper().await;
        let message = SubscribableMessage::GetAcceptedFrontier(GetAcceptedFrontier {
            chain_id: chain_id.clone(),
            request_id: rand::random(),
            deadline: DEFAULT_DEADLINE,
        });
        let Some(Message::AcceptedFrontier(res)) = self
            .send_to_peer(
                &MessageOrSubscribable::Subscribable(message.clone()),
                bootstrapper,
            )
            .await
        else {
            return Err(NodeError::Message("invalid message received".to_string()));
        };

        let message = SubscribableMessage::Get(Get {
            chain_id,
            request_id: rand::random(),
            deadline: DEFAULT_DEADLINE,
            container_id: res.container_id,
        });
        let Some(Message::Put(res)) = self
            .send_to_peer(
                &MessageOrSubscribable::Subscribable(message.clone()),
                bootstrapper,
            )
            .await
        else {
            return Err(NodeError::Message("invalid message received".to_string()));
        };
        let block = StatelessBlock::unpack(res.container)
            .map_err(|_| NodeError::Message("invalid container received".to_string()))?;
        let block = block.block();
        Ok(u64::from_be_bytes(*block.header.number()))
    }

    pub async fn send_to_peer(
        &self,
        message: &MessageOrSubscribable,
        node_id: NodeId,
    ) -> Option<Message> {
        let peer_opt = {
            let peers = self.peers_infos.read().unwrap();
            if peers.is_empty() {
                log::debug!("the set of peers is empty, cannot send to any");
                return None;
            }
            peers.get(&node_id).cloned()
        };

        let (remove_peer, err) = if let Some(peer) = peer_opt {
            if peer.handshook() {
                match self
                    .send_this_message_to_rename(&peer.sender, message)
                    .await
                {
                    Ok(maybe_message) => return maybe_message,
                    Err((remove_peer, err)) => (remove_peer, Some(err)),
                }
            } else {
                (true, None)
            }
        } else {
            (true, None)
        };

        let is_bootstrapper = self.is_bootstrapper(&node_id);
        if !is_bootstrapper && remove_peer {
            Network::remove_peers(
                self.peers_infos.clone(),
                &mut self.light_network.light_peers.write().map,
                vec![(node_id, err)],
            );
        }

        None
    }

    async fn send_this_message_to_rename(
        &self,
        sender: &PeerSender,
        message: &MessageOrSubscribable,
    ) -> Result<Option<Message>, (bool, NodeError)> {
        match message {
            MessageOrSubscribable::Subscribable(message) => {
                match sender.send_and_response(self.mail_box.tx(), message.clone()) {
                    Ok(handle) => handle
                        .await
                        .map(Some)
                        .map_err(|_| (true, SendErrorWrapper.into())),
                    Err(_err) => Err((true, _err)),
                }
            }
            MessageOrSubscribable::Message(message) => sender
                .send(message.clone())
                .map(|_| None)
                .map_err(|_err| (true, _err)),
        }
    }

    pub fn is_bootstrapper(&self, node_id: &NodeId) -> bool {
        self.bootstrappers.read().unwrap().contains_key(node_id)
    }

    pub fn pick_peer(
        peers_infos: &Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        bootstrappers: &RwLock<HashMap<NodeId, Option<DhtBuckets>>>,
        config: SinglePickerConfig,
    ) -> Option<NodeId> {
        match config {
            SinglePickerConfig::Bootstrapper => {
                let peers = peers_infos.read().unwrap();
                let available_peers: HashSet<_> = peers.keys().collect();
                let bootstrappers = bootstrappers.read().unwrap();
                let bootstrappers: HashSet<_> = bootstrappers
                    .iter()
                    .filter_map(|(node_id, buckets)| {
                        if buckets.is_none() {
                            Some(node_id)
                        } else {
                            None
                        }
                    })
                    .collect();
                let inter: Vec<_> = bootstrappers.intersection(&available_peers).collect();
                if inter.is_empty() {
                    None
                } else {
                    let i = (rand::random::<u64>() % inter.len() as u64) as usize;
                    Some(**inter[i])
                }
            }
            _ => todo!(),
        }
    }
}
