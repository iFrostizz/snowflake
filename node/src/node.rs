use crate::id::{Id, NodeId};
use crate::message::{mail_box::MailBox, SubscribableMessage};
use crate::net::{
    node::NodeError,
    queue::{ConnectionData, ConnectionQueue},
    HandshakeInfos, Network, Peer, PeerMessage,
};
use crate::server::peers::{PeerInfo, PeerLessInfo, PeerSender};
use crate::stats::{self, Metrics};
use crate::utils::{
    bloom::{Filter, ReadFilter, ViewFilter},
    constants,
    ip::{ip_from_octets, ip_octets},
};
use flume::{Receiver, Sender};
use futures::future;
use indexmap::IndexMap;
use prost::Message as _;
use proto_lib::p2p::{
    message::Message, AppGossip, BloomFilter, ClaimedIpPort, GetPeerList, PeerList,
};
use rand::random;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{self};
use tokio_rustls::TlsStream;

pub struct Node {
    pub(crate) network: Arc<Network>,
    connection_queue: ConnectionQueue,
    // TODO for now, we only use it for subscribable messages, but they are not currently supported.
    // TODO We should maybe handle ping / pong messages in order to build peer the latency ranking.
    mail_box: Arc<MailBox>,
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

impl Node {
    pub fn new(network: Arc<Network>, max_concurrent: usize, max_latency_records: usize) -> Self {
        let mail_box = MailBox::new(max_latency_records);

        Self {
            network,
            connection_queue: ConnectionQueue::new(max_concurrent),
            mail_box: Arc::new(mail_box),
        }
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
            node.connection_queue.watch_connections(&node, rx2).await;
            Ok(())
        });

        let node = self.clone();
        let rx2 = rx.resubscribe();
        let net = tokio::spawn(async move { node.loop_node_messages(rx2).await });

        let node = self.clone();
        let rx2 = rx.resubscribe();
        let watch =
            tokio::spawn(async move { node.watch_sent_transactions(transaction_rx, rx2).await });

        let node = self.clone();
        let rx2 = rx.resubscribe();
        let mbox = tokio::spawn(async move {
            node.mail_box.start(rx2).await;
            Ok(())
        });

        let out_pipeline = self.network.out_pipeline.clone();
        let pip = tokio::spawn(async move {
            out_pipeline.start(rx).await;
            Ok(())
        });

        vec![conn, net, watch, mbox, pip]
    }

    /// A created connection that may create a new peer.
    /// This connection has been started from the initiative of the network either from the bootstrap or from PeerList recommendations of other nodes.
    pub async fn create_connection(
        self: &Arc<Node>,
        semaphore: Arc<Semaphore>,
        data: ConnectionData,
    ) -> Result<(), NodeError> {
        let socket_addr = data.socket_addr;
        log::debug!("adding a new peer at {socket_addr:?}");

        self.connect_new_peer(semaphore, data).await?;

        log::debug!("added {socket_addr:?}");

        Ok(())
    }

    /// Connect to a new peer and register it in the network.
    /// The function will return either if the peer is connected or that the connection failed.
    pub async fn connect_new_peer(
        self: &Arc<Node>,
        semaphore: Arc<Semaphore>,
        data: ConnectionData,
    ) -> Result<(), NodeError> {
        let hs_permit = self.hs_permit().await;

        let mut peer = match Peer::connect_with_back_off(
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
                self.connection_queue.mark_connected(&data.node_id);
                peer
            }
            Err(err) => {
                log::debug!("error on connecting with back off: {err}");
                self.network.remove_peers(&vec![(data.node_id, Some(&err))]);
                return Err(err);
            }
        };

        let node = self.clone();
        tokio::spawn(async move {
            let err = if let Err(err) = node.loop_peer(hs_permit, &mut peer).await {
                log::debug!("error when looping peer {err:?}");
                Some(err)
            } else {
                None
            };

            let _ = node.network.disconnect_peer(peer, err.as_ref());
            node.connection_queue.maybe_add_connection(data);
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
        peer: &mut Peer,
    ) -> Result<(), NodeError> {
        log::trace!("looping a new peer");

        if !self.network.can_add_peer(&peer.node_id) {
            return Err(NodeError::Message("Unwanted peer".to_owned()));
        }

        let (tasks, tx) = self.spawn_peer(peer, hs_permit).await?;

        let (res, ..) = future::select_all(tasks).await;
        log::trace!("one of the peer tasks finished");
        let res = res.unwrap();

        let _ = tx.send(());

        res
    }

    async fn spawn_peer(
        self: &Arc<Node>,
        peer: &mut Peer,
        hs_permit: OwnedSemaphorePermit,
    ) -> Result<
        (
            Vec<JoinHandle<Result<(), NodeError>>>,
            broadcast::Sender<()>,
        ),
        NodeError,
    > {
        let node_id = peer.node_id;

        let (snp, rnp) = flume::unbounded();
        let sender: PeerSender = snp.into();

        self.network
            .add_peer(node_id, peer.x509_certificate.clone(), sender.clone())
            .await;

        let (spn, rpn) = flume::unbounded();
        let (tx, _) = broadcast::channel(1);
        let manage_peer = self.manage_peer(rpn, node_id, hs_permit, tx.subscribe());
        let (read, write) = peer.take_tls();
        let write_peer = self.write_peer(write, rnp, tx.subscribe());
        let read_peer = self.read_peer(&sender, node_id, read, spn, tx.subscribe());

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

        let mut tasks = vec![manage_peer, write_peer, read_peer, hand_peer];

        let sender = sender.clone();
        let peer_less = PeerLessInfo { sender };
        let loop_op = self.loop_messages_peer(peer_less, tx.subscribe());
        tasks.push(loop_op);

        Ok((tasks, tx))
    }

    fn manage_peer(
        self: &Arc<Node>,
        rpn: Receiver<PeerMessage>,
        node_id: NodeId,
        hs_permit: OwnedSemaphorePermit,
        rx: broadcast::Receiver<()>,
    ) -> JoinHandle<Result<(), NodeError>> {
        let node = self.clone();
        tokio::spawn(async move {
            node.execute_peer_operation(rpn, node_id, hs_permit, rx)
                .await;
            Ok(())
        })
    }

    fn write_peer(
        self: &Arc<Node>,
        write: WriteHalf<TlsStream<TcpStream>>,
        rnp: Receiver<Message>,
        disconnection_rx: broadcast::Receiver<()>,
    ) -> JoinHandle<Result<(), NodeError>> {
        log::trace!("write");
        let out_pipeline = self.network.out_pipeline.clone();
        tokio::spawn(async move {
            let res =
                Network::schedule_write_messages(out_pipeline, write, rnp, disconnection_rx).await;
            if res.is_err() {
                log::debug!("error on write");
            }
            res
        })
    }

    fn read_peer(
        self: &Arc<Node>,
        sender: &PeerSender,
        node_id: NodeId,
        read: ReadHalf<TlsStream<TcpStream>>,
        spn: Sender<PeerMessage>,
        rx: broadcast::Receiver<()>,
    ) -> JoinHandle<Result<(), NodeError>> {
        log::trace!("read");
        let peer_sender = sender.clone();
        let mail_box = self.mail_box.clone();
        tokio::spawn(async move {
            let res =
                Network::read_messages(&node_id, read, &peer_sender, &mail_box, &spn, rx).await;

            if res.is_err() {
                log::debug!("error on read");
            }

            match res {
                Err(NodeError::TcpConnection(tcp_err))
                    if tcp_err.kind() == ErrorKind::UnexpectedEof =>
                {
                    Err(NodeError::TcpConnection(tcp_err))
                }
                rest => rest,
            }
        })
    }

    fn loop_messages_peer(
        self: &Arc<Node>,
        peer_less: PeerLessInfo,
        rx: broadcast::Receiver<()>,
    ) -> JoinHandle<Result<(), NodeError>> {
        let node = self.clone();
        tokio::spawn(async move {
            let res = node.loop_messages(&peer_less, rx).await;
            if res.is_err() {
                log::debug!("error on recurring");
            }
            res
        })
    }

    /// Send messages to a peer on a recurring basis
    pub async fn loop_messages(
        &self,
        peer_less: &PeerLessInfo,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        let intervals = &self.network.config.intervals;

        let mut ping_interval = tokio::time::interval(Duration::from_millis(intervals.ping));

        loop {
            tokio::select! {
                _ = ping_interval.tick() => {
                    peer_less.ping()?;
                }
                _ = rx.recv() => {
                    return Ok(());
                }
            }
        }
    }

    pub async fn execute_peer_operation(
        self: Arc<Node>,
        rpn: Receiver<PeerMessage>,
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
                        self.manage_inner_message(&node_id, msg, &mut maybe_hs_permit);
                    }
                }
                _ = rx.recv() => {
                    return;
                }
            }
        }
    }

    fn manage_inner_message(
        self: &Arc<Node>,
        node_id: &NodeId,
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
                    if let Some(PeerInfo { infos, .. }) = peers.get_mut(node_id) {
                        if infos.is_none() {
                            stats::handshook_peers::inc();
                            let gossip_id = peer_infos.gossip_id(node_id);
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
                } else {
                    log::debug!("received NewPeer twice from permit {}", node_id);
                }
            }
        }
    }

    async fn watch_sent_transactions(
        self: &Arc<Node>,
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
        let push_gossip = proto_lib::sdk::PushGossip {
            gossip: vec![signed_tx],
        };

        // prefix the message with the client handler prefix, here it's 0 apparently
        // for more information, check the PrefixMessage function
        let mut app_bytes = Vec::from([constants::APP_PREFIX]);
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
        self.send_to_peers(&peers, &message);

        Ok(())
    }

    async fn loop_node_messages(
        self: &Arc<Node>,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        let intervals = &self.network.config.intervals;

        let mut get_peer_list_interval =
            time::interval(Duration::from_millis(intervals.get_peer_list));

        loop {
            tokio::select! {
                _ = get_peer_list_interval.tick() => {
                    self.get_peer_list();
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
        let (node_id, random_peer) = peers.get_index(random::<usize>() % peers.len()).unwrap();
        let bloom_filter = self.network.bloom_filter.read().unwrap().as_proto();
        if random_peer
            .sender
            .send(Message::GetPeerList(GetPeerList {
                known_peers: Some(bloom_filter),
            }))
            .is_err()
        {
            self.network
                .remove_peers(&vec![(*node_id, Some(&NodeError::SendError))]);
        }
    }

    fn fastest_peers(&self, n: usize) -> Vec<NodeId> {
        let peers_lat = self.mail_box.peers_latency().read().unwrap();
        peers_lat.fastest_n(n).copied().collect::<Vec<_>>()
    }

    fn random_peers(&self, n: usize) -> Vec<NodeId> {
        let peers = self.network.peers_infos.read().unwrap();
        if peers.is_empty() {
            return Vec::new();
        }

        (0..n)
            .fold(HashSet::new(), |mut set, _| {
                let (node_id, _) = peers.get_index(random::<usize>() % peers.len()).unwrap();
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
        let x509_certificate = claimed.x509_certificate;
        let node_id = NodeId::from_cert(x509_certificate.clone());
        if !self.network.can_add_peer(&node_id) {
            log::debug!("peer not desired {node_id}");
        } else {
            match claimed.ip_port.try_into() {
                Ok(port) => match ip_from_octets(claimed.ip_addr) {
                    Ok(ip_addr) => {
                        log::debug!("got peer list ip {ip_addr}");

                        match (ip_addr, port)
                            .to_socket_addrs()
                            .map(|mut socks| socks.next().ok_or("empty socket"))
                        {
                            Ok(Ok(socket_addr)) => {
                                self.connection_queue.add_connection(ConnectionData {
                                    node_id,
                                    socket_addr,
                                    timestamp: claimed.timestamp,
                                    x509_certificate,
                                });
                            }
                            err => {
                                log::error!("{err:?} {node_id}");
                            }
                        }
                    }
                    Err(err) => {
                        log::error!("err when ip {err}");
                    }
                },
                Err(err) => {
                    log::error!("err when converting port {err}");
                }
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

    pub fn send_to_peers(&self, node_ids: &[NodeId], message: &MessageOrSubscribable) {
        let peers = self.network.peers_infos.read().unwrap();
        if peers.is_empty() {
            log::debug!("the set of peers is empty, cannot send to any");
            return;
        }

        let n = node_ids.len();
        let to_remove: Vec<_> = node_ids
            .iter()
            .filter_map(|node_id| match peers.get(node_id) {
                Some(peer) => {
                    if peer.handshook() {
                        let sender = &peer.sender;
                        let res = match message {
                            MessageOrSubscribable::Subscribable(message) => sender
                                .send_with_subscribe(self.mail_box.tx(), *node_id, message.clone()),
                            MessageOrSubscribable::Message(message) => sender.send(message.clone()),
                        };

                        match res {
                            Err(_err) => Some((*node_id, Some(&NodeError::SendError))),
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

        if !to_remove.is_empty() {
            self.network.remove_peers(&to_remove);
        }
    }

    pub async fn hs_permit(&self) -> OwnedSemaphorePermit {
        self.network
            .handshake_semaphore
            .clone()
            .acquire_owned()
            .await
            .unwrap()
    }
}
