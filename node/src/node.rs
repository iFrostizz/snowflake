use crate::dht::block::DhtBlocks;
use crate::dht::kademlia::ValueOrNodes;
use crate::dht::{light_errors, DhtId, LightMessage, LightResult, LightValue};
use crate::id::{Id, NodeId};
use crate::message::SubscribableMessage;
use crate::net::light::DhtCodex;
use crate::net::node::{AddPeerError, NetworkConfig};
use crate::net::{
    light, node::NodeError, queue::ConnectionData, HandshakeInfos, Network, Peer, PeerMessage,
};
use crate::server::msg::AppRequestMessage;
use crate::server::peers::PeerInfo;
use crate::stats::{self, Metrics};
use crate::utils::{
    bloom::{Filter, ReadFilter, ViewFilter},
    constants,
    ip::ip_octets,
};
use flume::Receiver;
use futures::future;
use indexmap::IndexMap;
use openssl::x509;
use prost::Message as _;
use proto_lib::p2p::{
    message::Message, AppGossip, BloomFilter, ClaimedIpPort, GetPeerList, PeerList,
};
use proto_lib::sdk;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{self};

#[derive(Debug)]
pub struct Node {
    pub(crate) network: Arc<Network>,
}

#[derive(Debug, Clone)]
pub enum MessageOrSubscribable {
    Message(Message),
    #[allow(unused)]
    Subscribable(SubscribableMessage),
}

struct PickerConfig {
    fastest: usize,
    random: usize,
}

pub enum SinglePickerConfig {
    Bootstrapper,
    #[allow(unused)]
    Random,
    #[allow(unused)]
    Light,
}

impl Node {
    pub fn new(network_config: NetworkConfig) -> Self {
        let bytes = std::fs::read(&network_config.cert_path).expect("failed to read cert");
        let x509 = x509::X509::from_pem(&bytes).unwrap();
        let cert = x509.to_der().unwrap();
        let node_id = NodeId::from_cert(&cert);

        let peers_infos = Arc::new(RwLock::new(IndexMap::new()));
        let network = Arc::new(Network::new(network_config, node_id, peers_infos.clone()).unwrap());

        Self { network }
    }

    /// Start a node which manages in/out connections
    /// It's a wrapper around the [`Network`] which is an API for node connections
    /// and the [`ConnectionQueue`] which maintains a schedule of connections.
    pub async fn start(
        self: Arc<Node>,
        enable_metrics: bool,
        metrics_port: u16,
        transaction_rx: Receiver<(Vec<u8>, Instant)>,
        rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        log::info!("starting node with ID {}", &self.network.node_id);
        log::info!("public key: 0x{}", hex::encode(&self.network.public_key));
        log::info!("node PoP: 0x{}", hex::encode(&self.network.node_pop));

        if enable_metrics {
            Metrics::start(metrics_port);
        }
        let tasks = self.start_node(transaction_rx, rx);

        let (res, ..) = future::select_all(tasks).await;
        res.expect("thread panicked!")
    }

    fn start_node(
        self: Arc<Node>,
        transaction_rx: Receiver<(Vec<u8>, Instant)>,
        rx: broadcast::Receiver<()>,
    ) -> Vec<JoinHandle<Result<(), NodeError>>> {
        let node = self.clone();
        let rx2 = rx.resubscribe();
        let conn = tokio::spawn(async move {
            node.network
                .connection_queue
                .watch_connections(&node, rx2)
                .await;
            Ok(())
        });

        let node = self.clone();
        let rx2 = rx.resubscribe();
        let net = tokio::spawn(node.loop_node_messages(rx2));

        let node = self.clone();
        let rx2 = rx.resubscribe();
        let watch = tokio::spawn(node.watch_sent_transactions(transaction_rx, rx2));

        let network = self.network.clone();
        let rx2 = rx.resubscribe();
        let light = tokio::spawn(network.start_light_network(rx2));

        let node = self.clone();
        let rx2 = rx.resubscribe();
        let mbox = tokio::spawn(async move {
            node.network.mail_box.start(rx2).await;
            Ok(())
        });

        let node = self.clone();
        let rx2 = rx.resubscribe();
        let verif = tokio::spawn(async move {
            node.start_verification_channel(rx2).await;
            Ok(())
        });

        let out_pipeline = self.network.out_pipeline.clone();
        let pip = tokio::spawn(async move {
            out_pipeline.start(rx).await;
            Ok(())
        });

        vec![conn, net, watch, light, mbox, verif, pip]
    }

    /// A created connection that may create a new peer.
    /// This connection has been started from the initiative of the network
    /// either from the bootstrap or from recommendations of other nodes.
    pub async fn create_connection(
        self: &Arc<Node>,
        semaphore: Arc<Semaphore>,
        data: ConnectionData,
        connected_tx: Option<oneshot::Sender<bool>>,
    ) -> Result<(), NodeError> {
        let socket_addr = data.socket_addr;
        if self.is_my_socket(&socket_addr) {
            log::debug!("cannot add self as peer");
            return Err(NodeError::Message("Cannot add self as peer".to_owned()));
        }
        log::debug!("adding a new peer at {socket_addr:?}");

        self.connect_new_peer(semaphore, data, connected_tx).await?;

        log::debug!("added {socket_addr:?}");

        Ok(())
    }

    /// Connect to a new peer and register it in the network.
    /// The function will return either if the peer is connected or that the connection failed.
    pub async fn connect_new_peer(
        self: &Arc<Node>,
        semaphore: Arc<Semaphore>,
        data: ConnectionData,
        connected_tx: Option<oneshot::Sender<bool>>,
    ) -> Result<(), NodeError> {
        self.network.check_add_peer(&data.node_id)?;

        let hs_permit = self.hs_permit().await;

        let peer = match Peer::connect_with_back_off(
            semaphore,
            data.node_id,
            data.socket_addr,
            &self.network.client_config,
            &self.network.config.back_off,
        )
        .await
        {
            Ok(peer) => {
                log::debug!("peer {} at {:?} connected!", data.node_id, data.socket_addr);
                self.network.connection_queue.mark_connected(&data.node_id);
                peer
            }
            Err(err) => {
                log::debug!(
                    "error on connecting to {} with back off: {err}",
                    data.node_id
                );
                return Err(err);
            }
        };

        let node = self.clone();
        let peers_infos = self.network.peers_infos.clone();
        let light_peers = self.network.light_network.light_peers.clone();
        tokio::spawn(async move {
            let node_id = *peer.node_id();
            let err = match node.loop_peer(hs_permit, peer, connected_tx).await {
                Err(NodeError::UnwantedPeer(AddPeerError::AlreadyConnected)) => {
                    // timing issue; should not disconnect in this case.
                    return;
                }
                Err(err) => {
                    log::debug!("error when looping peer {:?} {err:?}", node_id);
                    Some(err)
                }
                _ => None,
            };

            // remove peer and try to reconnect
            Network::remove_peers(
                peers_infos,
                &mut light_peers.write().map,
                vec![(node_id, err)],
            );
            node.network.connection_queue.add_connection(data);
        });

        Ok(())
    }

    /// Main loop that reads and write messages for a peer
    /// # Panics
    ///
    /// The function panics if the tls connection is not attached to the peer
    pub async fn loop_peer(
        self: &Arc<Node>,
        hs_permit: OwnedSemaphorePermit,
        peer: Peer,
        connected_tx: Option<oneshot::Sender<bool>>,
    ) -> Result<(), NodeError> {
        log::trace!("looping a new peer");

        self.network.check_add_peer(peer.node_id())?;

        match self.spawn_peer(peer, hs_permit).await {
            Ok((tasks, tx)) => {
                connected_tx.map(|tx| tx.send(true));
                let (res, ..) = future::select_all(tasks).await;
                log::trace!("one of the peer tasks finished");
                let res = res.unwrap();

                let _ = tx.send(());

                res
            }
            Err(err) => {
                connected_tx.map(|tx| tx.send(false));
                Err(err)
            }
        }
    }

    async fn spawn_peer(
        self: &Arc<Node>,
        peer: Peer,
        hs_permit: OwnedSemaphorePermit,
    ) -> Result<
        (
            Vec<JoinHandle<Result<(), NodeError>>>,
            broadcast::Sender<()>,
        ),
        NodeError,
    > {
        let node_id = *peer.node_id();
        let c_chain_id = self.network.config.c_chain_id;

        let sender = peer.sender().clone();
        let (tx, _) = broadcast::channel(100);
        self.network
            .add_peer(
                node_id,
                peer.x509_certificate().to_owned(),
                sender.clone(),
                tx.clone(),
            )
            .await;

        let manage_peer = self.manage_peer(
            peer.rpn().clone(),
            peer.rpl().clone(),
            node_id,
            hs_permit,
            tx.subscribe(),
        );
        let (write_peer, read_peer, recurring) = peer.communicate(
            self.network.peers_infos.clone(),
            self.network.config.intervals.clone(),
            self.network.out_pipeline.clone(),
            self.network.mail_box.clone(),
            c_chain_id,
            tx.subscribe(),
        );

        let hand_peer = match self
            .network
            .handshake_peer(&sender, node_id, tx.subscribe())
        {
            Ok(ok) => ok,
            Err(err) => {
                // early return
                let _ = tx.send(());
                return Err(err); // propagate to disconnect cleanly
            }
        };

        let tasks = vec![manage_peer, write_peer, read_peer, recurring, hand_peer];

        Ok((tasks, tx))
    }

    fn manage_peer(
        self: &Arc<Node>,
        rpn: Receiver<PeerMessage>,
        rpl: Receiver<(LightMessage, Option<oneshot::Sender<LightResult>>)>,
        node_id: NodeId,
        hs_permit: OwnedSemaphorePermit,
        rx: broadcast::Receiver<()>,
    ) -> JoinHandle<Result<(), NodeError>> {
        let node = self.clone();
        tokio::spawn(async move {
            node.execute_peer_operation(&rpn, rpl, node_id, hs_permit, rx)
                .await;
            Ok(())
        })
    }

    pub async fn execute_peer_operation(
        self: Arc<Node>,
        rpn: &Receiver<PeerMessage>,
        rpl: Receiver<(LightMessage, Option<oneshot::Sender<LightResult>>)>,
        node_id: NodeId,
        hs_permit: OwnedSemaphorePermit,
        mut rx: broadcast::Receiver<()>,
    ) {
        let mut maybe_hs_permit = Some(hs_permit);

        loop {
            log::trace!("execute");
            tokio::select! {
                res = rpn.recv_async() => {
                    if let Ok(msg) = res {
                        self.manage_peer_message(node_id, msg, &mut maybe_hs_permit);
                    }
                }
                res = rpl.recv_async() => {
                    if let Ok((msg, resp)) = res {
                        self.manage_light_message(node_id, msg, resp);
                    }
                }
                _ = rx.recv() => {
                    return;
                }
            }
        }
    }

    fn manage_peer_message(
        self: &Arc<Node>,
        node_id: NodeId,
        message: PeerMessage,
        maybe_hs_permit: &mut Option<OwnedSemaphorePermit>,
    ) {
        log::trace!("received peer-network message {message:?}");
        match message {
            PeerMessage::ObserveUptime(ping) => {
                log::debug!("observe uptime: {}", ping.uptime);
            }
            PeerMessage::PeerList(peer_list) => {
                self.handle_peer_list(peer_list);
            }
            PeerMessage::GetPeerList {
                sender,
                known_peers,
            } => {
                let amount_ip_n = 15;

                let claimed_ip_ports = if let Some(known_peers) = known_peers {
                    self.propose_peers(known_peers, amount_ip_n)
                } else {
                    vec![]
                };

                let _ = sender.send(Message::PeerList(PeerList { claimed_ip_ports }));
            }
            PeerMessage::NewPeer { infos: peer_infos } => {
                if let Some(hs_permit) = maybe_hs_permit.take() {
                    let mut peers = self.network.peers_infos.write().unwrap();
                    if let Some(PeerInfo { infos, .. }) = peers.get_mut(&node_id) {
                        if infos.is_none() {
                            stats::handshook_peers::inc();
                            let gossip_id = peer_infos.gossip_id(&node_id);
                            *infos = Some(peer_infos);
                            let mut bloom_filter = self.network.bloom_filter.write().unwrap();
                            bloom_filter.feed(gossip_id); // we write it to the filter even if it fails to avoid always hearing about it
                            Self::regen_bloom_if_necessary(&peers, &mut bloom_filter);
                        } else {
                            log::debug!("received NewPeer twice {}", node_id);
                        }
                    } else {
                        log::debug!("received NewPeer while the infos were ready {}", node_id);
                    }

                    drop(hs_permit);
                }
            }
        }
    }

    pub fn manage_light_message(
        &self,
        node_id: NodeId,
        message: LightMessage,
        resp: Option<oneshot::Sender<LightResult>>,
    ) {
        let res = match message {
            LightMessage::NewPeer(buckets) => {
                self.network
                    .light_network
                    .light_peers
                    .write()
                    .insert(node_id, buckets);
                None
            }
            LightMessage::Store(dht_id, value) => {
                let res = match dht_id {
                    DhtId::Block => match DhtBlocks::decode(&value) {
                        Ok(block) => {
                            let (tx, _) = oneshot::channel();
                            if self
                                .network
                                .light_network
                                .kademlia_dht
                                .verification_tx
                                .send((block, tx))
                                .is_err()
                            {
                                return;
                            }
                            Ok(LightValue::Ok)
                        }
                        Err(err) => Err(err),
                    },
                    _ => Err(light_errors::INVALID_DHT),
                };
                Some(res)
            }
            LightMessage::FindNode(bucket) => {
                let res = Ok(LightValue::ValueOrNodes(ValueOrNodes::Nodes(
                    self.network.light_network.kademlia_dht.find_node(&bucket),
                )));
                Some(res)
            }
            LightMessage::FindValue(dht_id, bucket) => {
                let res = match dht_id {
                    DhtId::Block => self
                        .network
                        .light_network
                        .find_value(&self.network.light_network.block_dht.dht.store, &bucket),
                    _ => Err(light_errors::INVALID_DHT),
                };
                Some(res)
            }
            LightMessage::Nodes(cds) => {
                let cds = cds
                    .into_iter()
                    .filter(|c| c.node_id != self.network.node_id)
                    .collect::<Vec<_>>();
                if !cds.is_empty() {
                    self.network
                        .light_network
                        .light_peers
                        .potentially_add_nodes(cds);
                }
                None
            }
        };

        match (res, resp) {
            (Some(res), Some(resp)) => {
                let _ = resp.send(res);
            }
            (None, None) | (Some(Ok(LightValue::Ok)), None) => (),
            (res, resp) => {
                log::error!("unexpected state for res and resp values");
                log::error!("this is a logic bug. please report it.");
                log::error!("res: {:?} resp: {:?}", res, resp);
                unreachable!();
            }
        }
    }

    pub async fn start_verification_channel(&self, mut rx: broadcast::Receiver<()>) {
        let verification_rx = self.network.verification_rx.clone();
        let pool = Arc::new(Semaphore::new(10));
        loop {
            tokio::select! {
                maybe_block = verification_rx.recv_async() => {
                    if let Ok((block, tx)) = maybe_block {
                        let network = self.network.clone();
                        let pool = pool.clone();
                        // TODO preferably the semaphore should be acquired before spawning the task
                        tokio::spawn(async move {
                            let r = pool.acquire().await.unwrap();
                            let res = network.verify_block(&block).await.is_ok_and(|res| res);
                            let _ = tx.send(res);
                            if res {
                                let _ = network.light_network.block_dht.store_block_if_desired(block);
                            }
                            drop(r);
                        });
                    }
                },
                _ = rx.recv() => return
            }
        }
    }

    async fn watch_sent_transactions(
        self: Arc<Node>,
        transaction_rx: Receiver<(Vec<u8>, Instant)>,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        loop {
            tokio::select! {
                tx_data = transaction_rx.recv_async() => {
                    if let Ok((signed_tx, instant)) = tx_data {
                        log::info!(
                            "send signed transaction {signed_tx:?} {}ns",
                            Instant::now().duration_since(instant).as_nanos()
                        );
                        self.send_tx(signed_tx).await?;
                    }
                }
                _ = rx.recv() => {
                    return Ok(());
                }
            }
        }
    }

    async fn send_tx(self: &Arc<Node>, signed_tx: Vec<u8>) -> Result<(), NodeError> {
        let chain_id = self.network.config.c_chain_id.as_ref().to_vec();
        let push_gossip = sdk::PushGossip {
            gossip: vec![signed_tx],
        };

        // prefix the message with the client handler prefix,
        // for more information, check the PrefixMessage function
        let mut app_bytes = unsigned_varint::encode::u64(
            constants::AVALANCHEGO_HANDLER_ID,
            &mut unsigned_varint::encode::u64_buffer(),
        )
        .to_vec();
        push_gossip
            .encode(&mut app_bytes)
            .expect("the buffer capacity should be dynamically updated");
        let message = Message::AppGossip(AppGossip {
            chain_id,
            app_bytes,
        });
        let message = MessageOrSubscribable::Message(message);

        let peers = self.pick_peers(PickerConfig {
            fastest: 10,
            random: 10,
        });
        self.send_to_peers(&peers, &message).await;

        Ok(())
    }

    async fn loop_node_messages(
        self: Arc<Node>,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        let intervals = &self.network.config.intervals;

        let mut get_peer_list_interval =
            time::interval(Duration::from_millis(intervals.get_peer_list));
        let mut find_nodes_interval = time::interval(Duration::from_millis(intervals.find_nodes));

        loop {
            tokio::select! {
                _ = get_peer_list_interval.tick() => {
                    self.get_peer_list();
                }
                _ = find_nodes_interval.tick() => {
                    self.find_nodes();
                }
                _ = rx.recv() => {
                    return Ok(())
                }
            }
        }
    }

    fn get_peer_list(self: &Arc<Node>) {
        let peers = self.network.peers_infos.read().unwrap();
        if peers.is_empty() || self.network.has_reached_max_peers(&peers) {
            return;
        }
        let (node_id, random_peer) = peers
            .get_index((rand::random::<u64>() % peers.len() as u64) as usize)
            .unwrap();
        let bloom_filter = self.network.bloom_filter.read().unwrap().as_proto();
        if let Err(err) = random_peer.sender.send(Message::GetPeerList(GetPeerList {
            known_peers: Some(bloom_filter),
        })) {
            Network::remove_peers(
                self.network.peers_infos.clone(),
                &mut self.network.light_network.light_peers.write().map,
                vec![(*node_id, Some(err))],
            );
        }
    }

    fn find_nodes(self: &Arc<Node>) {
        let light_peers = self.network.light_network.light_peers.read().unwrap();
        let Some(node_id) = light::closest_peer(self.network.node_id, &light_peers) else {
            return;
        };
        let peers_infos = self.network.peers_infos.read().unwrap();
        let Some(random_peer) = peers_infos.get(&node_id) else {
            return;
        };
        if let Err(err) = random_peer.sender.send(
            AppRequestMessage::encode(
                &self.network.config.c_chain_id,
                sdk::FindNode {
                    bucket: self.network.node_id.into(),
                },
            )
            .unwrap(),
        ) {
            Network::remove_peers(
                self.network.peers_infos.clone(),
                &mut self.network.light_network.light_peers.write().map,
                vec![(node_id, Some(err))],
            );
        }
    }

    fn fastest_peers(&self, n: usize) -> Vec<NodeId> {
        let peers_lat = self.network.mail_box.peers_latency().read().unwrap();
        peers_lat.fastest_n(n).copied().collect::<Vec<_>>()
    }

    fn random_peers(&self, n: usize) -> Vec<NodeId> {
        let peers = self.network.peers_infos.read().unwrap();
        if peers.is_empty() {
            return Vec::new();
        }

        (0..n)
            .fold(HashSet::new(), |mut set, _| {
                let (node_id, _) = peers
                    .get_index((rand::random::<u64>() % peers.len() as u64) as usize)
                    .unwrap();
                set.insert(*node_id);
                set
            })
            .into_iter()
            .collect()
    }

    fn regen_bloom_if_necessary(peers: &IndexMap<NodeId, PeerInfo>, bloom_filter: &mut Filter) {
        if bloom_filter.is_sub_optimal() {
            let connected_peers = peers.iter().filter_map(|(node_id, peer_infos)| {
                peer_infos.infos.as_ref().map(|infos| (node_id, infos))
            });
            let tracked = connected_peers.clone().count() as u64;
            log::debug!("regenerating bloom filter, tracking {tracked}");
            // multiply by 2 to allow peers to have a new IP in a filter reset
            let tracked = std::cmp::max(tracked * 2, 128);
            match Filter::new_optimal(tracked) {
                Ok(filter) => *bloom_filter = filter,
                Err(err) => log::error!("error when generating new bloom filter {err}"),
            }

            for (node_id, infos) in connected_peers {
                bloom_filter.feed(infos.gossip_id(node_id));
            }
        }
    }

    // TODO keep a local store of the peers so we can more efficiently reconnect to them.
    fn handle_peer_list(self: &Arc<Node>, peer_list: PeerList) {
        for claimed in peer_list.claimed_ip_ports {
            self.try_connect_from_claimed(claimed);
        }
    }

    fn try_connect_from_claimed(self: &Arc<Node>, claimed: ClaimedIpPort) {
        let connection_data: Result<ConnectionData, _> = claimed.try_into();
        if let Ok(connection_data) = connection_data {
            let node_id = &connection_data.node_id;
            match self.network.check_add_peer(node_id) {
                Ok(()) => {
                    self.network
                        .connection_queue
                        .add_connection_without_retries(connection_data, None);
                }
                Err(err) => log::debug!("{err} {node_id}"),
            }
        }
    }

    fn propose_peers(
        self: &Arc<Node>,
        known_peers: BloomFilter,
        amount_ip_n: usize,
    ) -> Vec<ClaimedIpPort> {
        match ReadFilter::try_from(known_peers.filter.as_slice()) {
            Ok(filter) => {
                let mut ips = Vec::with_capacity(amount_ip_n);
                let peers_info = self.network.peers_infos.read().unwrap();
                for (node_id, peer_info) in peers_info.iter() {
                    if ips.len() >= amount_ip_n {
                        break;
                    }

                    if let Some(handshake_infos) = &peer_info.infos {
                        if !filter.contains(handshake_infos.gossip_id(node_id)) {
                            let HandshakeInfos {
                                ip_signing_time: timestamp,
                                sock_addr,
                                ip_node_id_sig: signature,
                                ..
                            } = handshake_infos;

                            ips.push(ClaimedIpPort {
                                x509_certificate: peer_info.x509_certificate.clone(),
                                ip_addr: ip_octets(sock_addr.ip()),
                                ip_port: sock_addr.port() as u32,
                                timestamp: *timestamp,
                                signature: signature.clone(),
                                tx_id: Id::<32>::default().as_slice().to_vec(),
                            });
                        }
                    }
                }

                log::debug!("proposing {} ips", ips.len());

                ips
            }
            Err(err) => {
                log::error!("error when converting to filter {err}");
                vec![]
            }
        }
    }

    fn pick_peers(&self, config: PickerConfig) -> Vec<NodeId> {
        let mut peers = HashSet::new();
        for p in self.fastest_peers(config.fastest) {
            peers.insert(p);
        }
        for p in self.random_peers(config.random) {
            peers.insert(p);
        }
        peers.into_iter().collect()
    }

    pub async fn send_to_peers(
        &self,
        node_ids: &[NodeId],
        message: &MessageOrSubscribable,
    ) -> Vec<Message> {
        let (to_remove, handles) = {
            let peers = self.network.peers_infos.read().unwrap();
            if peers.is_empty() {
                log::debug!("the set of peers is empty, cannot send to any");
                return vec![];
            }

            let n = node_ids.len();
            let mut handles = Vec::new();
            let to_remove: Vec<_> = node_ids
                .iter()
                .filter_map(|node_id| match peers.get(node_id) {
                    Some(peer) => {
                        if peer.handshook() {
                            let sender = &peer.sender;
                            let res = match message {
                                MessageOrSubscribable::Subscribable(message) => {
                                    match sender.send_and_response(
                                        self.network.mail_box.tx(),
                                        message.clone(),
                                    ) {
                                        Ok(handle) => {
                                            handles.push(handle);
                                            Ok(())
                                        }
                                        Err(err) => Err(err),
                                    }
                                }
                                MessageOrSubscribable::Message(message) => {
                                    sender.send(message.clone())
                                }
                            };

                            match res {
                                Err(_err) => Some((*node_id, Some(_err))),
                                Ok(_) => None,
                            }
                        } else {
                            None
                        }
                    }
                    None => Some((*node_id, None)),
                })
                .collect();
            drop(peers);

            log::debug!("sending to {} peers", n);

            (to_remove, handles)
        };

        let handles = handles.into_iter().map(|handle| tokio::spawn(handle));

        if !to_remove.is_empty() {
            Network::remove_peers(
                self.network.peers_infos.clone(),
                &mut self.network.light_network.light_peers.write().map,
                to_remove,
            );
        }

        let mut messages = Vec::new();
        for handle in handles {
            if let Ok(Ok(message)) = handle.await {
                messages.push(message);
            }
        }
        messages
    }

    pub async fn hs_permit(&self) -> OwnedSemaphorePermit {
        self.network
            .handshake_semaphore
            .clone()
            .acquire_owned()
            .await
            .unwrap()
    }

    fn is_my_socket(self: &Arc<Node>, socket_addr: &SocketAddr) -> bool {
        let my_socket = &self.network.config.socket_addr;
        (my_socket == socket_addr)
            || (socket_addr.ip().is_loopback() && my_socket.port() == socket_addr.port())
    }
}
