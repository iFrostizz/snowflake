use crate::dht::kademlia::ValueOrNodes;
use crate::dht::LightError;
use crate::dht::{Bucket, DhtId, LightMessage, LightResult};
use crate::dht::{DhtBuckets, LightValue};
use crate::id::{ChainId, NodeId};
use crate::message::mail_box::Mail;
use crate::message::{mail_box::MailBox, pipeline::Pipeline, MiniMessage, SubscribableMessage};
use crate::net::sdk::{FindNode, FindValue, LightHandshake};
use crate::net::{
    ip::SignedIp,
    node::{NetworkConfig, NodeError},
};
use crate::server::msg::{AppResponseMessage, InboundMessageExt};
use crate::server::tcp::read_stream_message;
use crate::server::{
    msg::InboundMessage,
    peers::{PeerInfo, PeerSender},
};
use crate::utils::bls::Bls;
use crate::utils::constants::SNOWFLAKE_HANDLER_ID;
use crate::utils::{bloom::Filter, ip::ip_from_octets, packer::Packer};
use async_recursion::async_recursion;
use flume::{Receiver, Sender};
use indexmap::IndexMap;
use proto_lib::p2p::{
    self, message::Message, AppError, BloomFilter, Client, GetPeerList, Handshake,
};
use proto_lib::sdk;
use ripemd::Digest;
use rustls::ClientConfig;
use rustls_pki_types::ServerName;
use sha2::Sha256;
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, ErrorKind};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{split, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio_rustls::{TlsConnector, TlsStream};

pub mod ip;
pub mod latency;
pub mod light;
pub mod node;
pub mod queue;

#[derive(Debug)]
pub struct BackoffParams {
    pub initial_duration: Duration,
    pub muln: u32,
    pub max_retries: usize,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct HandshakeInfos {
    pub ip_signing_time: u64,
    pub network_id: u32,
    pub sock_addr: SocketAddr,
    pub ip_node_id_sig: Vec<u8>,
    pub client: Option<Client>,
    pub tracked_subnets: Vec<Vec<u8>>,
    pub supported_acps: Vec<u32>,
    pub objected_acps: Vec<u32>,
}

/// A network is a list of peers
#[derive(Debug)]
pub struct Network {
    pub config: NetworkConfig,
    pub node_id: NodeId,
    pub signed_ip: SignedIp,
    pub client: Client,
    pub client_config: Arc<ClientConfig>,
    /// All peers discovered by the node
    pub peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>, // TODO can we find a way to do it lock-less ?
    pub bootstrappers: RwLock<HashMap<NodeId, Option<DhtBuckets>>>,
    pub out_pipeline: Arc<Pipeline>,
    /// The canonically sorted validators map
    pub bloom_filter: RwLock<Filter>,
    pub public_key: [u8; Bls::PUBLIC_KEY_BYTES],
    pub node_pop: Vec<u8>,
    pub handshake_semaphore: Arc<Semaphore>,
}

/// Intervals of operations in milliseconds
#[derive(Debug, Clone)]
pub struct Intervals {
    pub ping: u64,
    pub get_peer_list: u64,
    pub find_nodes: u64,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum PeerMessage {
    NewPeer {
        infos: HandshakeInfos,
    },
    ObserveUptime(p2p::Ping),
    PeerList(p2p::PeerList),
    GetPeerList {
        sender: PeerSender,
        known_peers: Option<BloomFilter>,
    },
}

impl HandshakeInfos {
    pub fn gossip_id(&self, node_id: &NodeId) -> [u8; 32] {
        let mut packer = Packer::new_with_capacity(NodeId::LEN + u64::BITS as usize / 8);
        packer.pack_fixed_bytes(node_id.as_ref());
        packer.pack_long(&self.ip_signing_time);
        let bytes = packer.finish();
        let mut hasher = <Sha256 as Digest>::new();
        hasher.update(bytes);
        hasher.finalize().into()
    }
}

/// Represents the network connection state of a peer
#[derive(Debug)]
pub struct PeerConnection {
    tls: Option<TlsStream<TcpStream>>,
    #[allow(unused)]
    sock_addr: SocketAddr,
    #[allow(unused)]
    timestamp: u64,
}

/// Represents the identity of a peer
#[derive(Debug, Clone)]
pub struct PeerIdentity {
    node_id: NodeId,
    x509_certificate: Vec<u8>,
}

/// Represents the messaging channels for a peer
#[derive(Debug)]
pub struct PeerChannels {
    spn: Sender<PeerMessage>,
    rpn: Receiver<PeerMessage>,
    sender: PeerSender,
    rnp: Receiver<Message>,
    spl: Sender<(LightMessage, Option<oneshot::Sender<LightResult>>)>,
    rpl: Receiver<(LightMessage, Option<oneshot::Sender<LightResult>>)>,
}

/// A peer bidirectional connection
#[derive(Debug)]
pub struct Peer {
    identity: PeerIdentity,
    connection: PeerConnection,
    channels: PeerChannels,
}

impl Peer {
    pub fn new(
        node_id: NodeId,
        x509_certificate: Vec<u8>,
        sock_addr: SocketAddr,
        timestamp: u64,
        tls: TlsStream<TcpStream>,
    ) -> Self {
        let (spn, rpn) = flume::unbounded();
        let (spl, rpl) = flume::unbounded();
        let (snp, rnp) = flume::unbounded();
        let sender: PeerSender = snp.into();

        Self {
            identity: PeerIdentity {
                node_id,
                x509_certificate,
            },
            connection: PeerConnection {
                tls: Some(tls),
                sock_addr,
                timestamp,
            },
            channels: PeerChannels {
                spn,
                rpn,
                sender,
                rnp,
                spl,
                rpl,
            },
        }
    }

    pub fn node_id(&self) -> &NodeId {
        &self.identity.node_id
    }

    pub fn sender(&self) -> &PeerSender {
        &self.channels.sender
    }

    pub fn x509_certificate(&self) -> &[u8] {
        &self.identity.x509_certificate
    }

    pub fn rpn(&self) -> &Receiver<PeerMessage> {
        &self.channels.rpn
    }

    pub fn rpl(&self) -> &Receiver<(LightMessage, Option<oneshot::Sender<LightResult>>)> {
        &self.channels.rpl
    }

    fn take_tls(
        &mut self,
    ) -> (
        ReadHalf<TlsStream<TcpStream>>,
        WriteHalf<TlsStream<TcpStream>>,
    ) {
        split(self.connection.tls.take().expect("missing tls field"))
    }

    pub(crate) async fn connect_with_back_off(
        semaphore: Arc<Semaphore>,
        node_id: NodeId,
        socket_addr: SocketAddr,
        config: &Arc<ClientConfig>,
        back_off: &BackoffParams,
    ) -> Result<Self, NodeError> {
        let mut duration = back_off.initial_duration;
        let mut sleep = tokio::time::sleep(duration);
        let mut errs = Vec::new();

        let mut retries = 0;
        while retries < back_off.max_retries {
            let permit = semaphore.acquire().await.unwrap();

            let err = match tokio::time::timeout(
                Duration::from_secs(5),
                Self::connect(&socket_addr, config),
            )
            .await
            {
                Ok(Ok(tls)) => {
                    let server_connection = tls.get_ref().1;
                    let certs = server_connection
                        .peer_certificates()
                        .ok_or(NodeError::Message("missing TLS certificates".to_owned()))?;
                    let mut certs = certs.iter();
                    let cert = certs.next().ok_or(NodeError::Message(
                        "need at least 1 TLS certificate".to_owned(),
                    ))?;
                    if certs.next().is_some() {
                        return Err(NodeError::Message(
                            "need at most 1 TLS certificate".to_owned(),
                        ));
                    }

                    return Ok(Self::new(node_id, cert.to_vec(), socket_addr, 0, tls));
                }
                Ok(Err(err)) => err,
                Err(err) => NodeError::Timeout(err),
            };
            errs.push(err);

            drop(permit);

            sleep.await;

            retries += 1;
            duration *= back_off.muln;
            sleep = tokio::time::sleep(duration);
        }

        Err(NodeError::Failed(errs))
    }

    #[allow(clippy::type_complexity)]
    pub fn communicate(
        mut self,
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        intervals: Intervals,
        out_pipeline: Arc<Pipeline>,
        mail_box: Arc<MailBox>,
        chain_id: ChainId,
        disconnection_rx: broadcast::Receiver<()>,
    ) -> (
        JoinHandle<Result<(), NodeError>>,
        JoinHandle<Result<(), NodeError>>,
        JoinHandle<Result<(), NodeError>>,
    ) {
        let mail_tx = mail_box.tx().clone();
        let (read, write) = self.take_tls();

        let disconnection_rx2 = disconnection_rx.resubscribe();
        let rnp = self.channels.rnp.clone();
        let write = tokio::spawn(Self::write_peer(
            out_pipeline,
            write,
            rnp,
            disconnection_rx2,
        ));

        let peer = Arc::new(self);
        let sender = peer.channels.sender.clone();

        let peer2 = peer.clone();
        let disconnection_rx2 = disconnection_rx.resubscribe();
        let read =
            tokio::spawn(peer2.read_peer(read, mail_box, sender, chain_id, disconnection_rx2));

        let recurring = tokio::spawn(peer.loop_messages_peer(
            peers_infos,
            intervals,
            mail_tx,
            disconnection_rx,
        ));

        (write, read, recurring)
    }

    async fn connect(
        sock_addr: &SocketAddr,
        config: &Arc<ClientConfig>,
    ) -> Result<TlsStream<TcpStream>, NodeError> {
        let dns_name = ServerName::try_from(sock_addr.ip().to_string())
            .map_err(|_| NodeError::Dns)?
            .to_owned();

        let sock =
            tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(sock_addr)).await??;

        let config = TlsConnector::from(config.clone());
        let tls = config.connect(dns_name, sock).await?;

        Ok(TlsStream::Client(tls))
    }

    async fn write_peer(
        out_pipeline: Arc<Pipeline>,
        write: WriteHalf<TlsStream<TcpStream>>,
        rnp: Receiver<Message>,
        disconnection_rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        log::trace!("write");
        let res =
            Network::schedule_write_messages(out_pipeline, write, rnp, disconnection_rx).await;
        if res.is_err() {
            log::debug!("error on write");
        }
        res
    }

    async fn read_peer(
        self: Arc<Peer>,
        read: ReadHalf<TlsStream<TcpStream>>,
        mail_box: Arc<MailBox>,
        sender: PeerSender,
        c_chain_id: ChainId,
        disconnection_rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        log::trace!("read");
        let res = Self::read_messages(
            &self,
            read,
            &c_chain_id,
            sender,
            &mail_box,
            disconnection_rx,
        )
        .await;

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
    }

    /// Send messages to a peer on a recurring basis
    async fn loop_messages_peer(
        self: Arc<Peer>,
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        intervals: Intervals,
        mail_tx: Sender<Mail>,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        let mut ping_interval = tokio::time::interval(Duration::from_millis(intervals.ping));
        // let mut find_nodes_interval = tokio::time::interval(Duration::from_millis(intervals.find_nodes));
        let node_id = self.identity.node_id;

        loop {
            tokio::select! {
                _ = ping_interval.tick() => {
                    let Some(peer_info) = peers_infos.read().unwrap().get(&node_id).cloned() else {
                        continue;
                    };
                    peer_info.ping(node_id, &mail_tx).await?;
                }
                // _ = find_nodes_interval.tick() => {
                //     if let Some(peer_info) = peers_infos.read().unwrap().get(&node_id) {
                //         peer_info.find_nodes(node_id, &mail_tx).await?;
                //     }
                // }
                _ = rx.recv() => {
                    return Ok(());
                }
            }
        }
    }

    /// Continuously read messages and return an error on an EOF
    async fn read_messages(
        &self,
        mut read: ReadHalf<TlsStream<TcpStream>>,
        c_chain_id: &ChainId,
        sender: PeerSender,
        mail_box: &MailBox,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        loop {
            log::trace!("read");
            tokio::select! {
                maybe_buf = read_stream_message(&mut read) => {
                    let buf = maybe_buf?;
                    self.manage_message(c_chain_id, &buf, &sender, mail_box, false).await?;
                }
                _ = rx.recv() => {
                    return Ok(())
                }
            }
        }
    }

    /// What should we do when receiving a message from another peer?
    #[async_recursion]
    async fn manage_message<'a>(
        &self,
        c_chain_id: &ChainId,
        buf: &[u8],
        sender: &PeerSender,
        mail_box: &MailBox,
        recursed: bool,
    ) -> Result<(), NodeError> {
        let decoded = InboundMessage::decode(buf).map_err(NodeError::Decoding)?;

        let mini = MiniMessage::from(&decoded);
        if !matches!(decoded, Message::CompressedZstd(_)) {
            log::trace!("received message {}", &mini);
            mini.inc_recv(buf.len() as u64);
        }

        let sent_message =
            if let Some(request_id) = SubscribableMessage::response_request_id(&decoded) {
                mail_box.mark_mail_received(&self.identity.node_id, request_id, decoded.clone())
            } else {
                None
            };

        // TODO if this node holds a stake, here is the minimum amount of messages to handle since
        //   they are registered and will get the node benched:
        //   AppRequest, PullQuery, PushQuery, Get, GetAncestors, GetAccepted, GetAcceptedFrontier, GetAcceptedStateSummary, GetStateSummaryFrontier
        log::trace!("new incoming message {decoded:?}");
        match decoded {
            Message::CompressedZstd(ref comp) => {
                if recursed {
                    return Err(NodeError::Message(format!(
                        "got nested decompression recursion for message {}",
                        mini
                    )));
                }
                let buf_read = BufReader::new(&comp[..]);
                let decoded_buf = zstd::stream::decode_all(buf_read)?;

                self.manage_message(c_chain_id, &decoded_buf, sender, mail_box, true)
                    .await?;

                return Ok(());
            }
            Message::Ping(ping) => {
                self.channels
                    .spn
                    .send(PeerMessage::ObserveUptime(ping))
                    .map_err(|_| NodeError::SendError)?;

                // TODO track uptime
                sender
                    .send(Message::Pong(p2p::Pong {
                        uptime: 100,
                        subnet_uptimes: Vec::new(),
                    }))
                    .map_err(|_| NodeError::SendError)?;
            }
            Message::Handshake(handshake) => {
                let Handshake {
                    network_id,
                    my_time: _, // TODO reject if too early
                    ip_addr,
                    ip_port,
                    ip_signing_time,
                    ip_node_id_sig,
                    tracked_subnets,
                    client,
                    supported_acps,
                    objected_acps,
                    known_peers,
                    ..
                } = handshake;

                // send a PeerList message according to their filter
                self.channels
                    .spn
                    .send(PeerMessage::GetPeerList {
                        sender: sender.clone(),
                        known_peers,
                    })
                    .map_err(|_| NodeError::SendError)?;

                let ip = ip_from_octets(ip_addr)
                    .map_err(|_| NodeError::Message("failed to serialize IP".to_string()))?;
                let port = ip_port
                    .try_into()
                    .map_err(|_| NodeError::Message("failed to convert port".to_string()))?;
                let sock_addr = SocketAddr::new(ip, port);

                // TODO check if the peer is already known because this will lead to message
                //  amplification if connecting to snowflake clients.
                //  Also, the PeerList should only be sent in that case.
                self.channels
                    .spn
                    .send(PeerMessage::NewPeer {
                        infos: HandshakeInfos {
                            ip_signing_time,
                            network_id,
                            sock_addr,
                            ip_node_id_sig,
                            client,
                            tracked_subnets,
                            supported_acps,
                            objected_acps,
                        },
                    })
                    .map_err(|_| NodeError::SendError)?;
            }
            Message::PeerList(peer_list) => {
                self.channels
                    .spn
                    .send(PeerMessage::PeerList(peer_list))
                    .map_err(|_| NodeError::SendError)?;
            }
            Message::GetPeerList(GetPeerList { known_peers }) => {
                self.channels
                    .spn
                    .send(PeerMessage::GetPeerList {
                        sender: sender.clone(),
                        known_peers,
                    })
                    .map_err(|_| NodeError::SendError)?;
            }
            Message::AppRequest(app_request) => {
                if app_request.chain_id != c_chain_id.as_ref() {
                    return Ok(());
                }
                let bytes = app_request.app_bytes;
                let (app_id, bytes) = unsigned_varint::decode::u64(&bytes)
                    .map_err(|_| NodeError::Message("failed to decode app ID".to_string()))?;
                if app_id != SNOWFLAKE_HANDLER_ID {
                    return Ok(());
                }
                let light_message = InboundMessage::decode(bytes).map_err(NodeError::Decoding)?;
                Self::manage_light_request(
                    c_chain_id,
                    app_request.request_id,
                    &self.channels.spl,
                    sender,
                    light_message,
                )
                .await?;
            }
            Message::AppResponse(app_response) => {
                if app_response.chain_id != c_chain_id.as_ref() {
                    return Ok(());
                }
                let bytes = app_response.app_bytes;
                let (app_id, response_bytes) = unsigned_varint::decode::u64(&bytes)
                    .map_err(|_| NodeError::Message("failed to decode app ID".to_string()))?;
                if app_id != SNOWFLAKE_HANDLER_ID {
                    return Ok(());
                }
                if let Some(Message::AppRequest(app_request)) = sent_message {
                    if app_request.chain_id != c_chain_id.as_ref() {
                        return Ok(());
                    }
                    let bytes = app_request.app_bytes;
                    let (app_id, request_bytes) = unsigned_varint::decode::u64(&bytes)
                        .map_err(|_| NodeError::Message("failed to decode app ID".to_string()))?;
                    if app_id != SNOWFLAKE_HANDLER_ID {
                        return Ok(());
                    }
                    let light_request =
                        InboundMessage::decode(request_bytes).map_err(NodeError::Decoding)?;

                    let light_response =
                        InboundMessage::decode(response_bytes).map_err(NodeError::Decoding)?;
                    Self::manage_light_response(&self.channels.spl, light_request, light_response)
                        .await?;
                }
            }
            Message::Pong(_pong) => {}
            _ => log::trace!("unsupported message {} {}", mini, self.identity.node_id),
        };

        Ok(())
    }

    async fn manage_light_request(
        c_chain_id: &ChainId,
        request_id: u32,
        spl: &Sender<(LightMessage, Option<oneshot::Sender<LightResult>>)>,
        sender: &PeerSender,
        light_message: sdk::light_request::Message,
    ) -> Result<(), NodeError> {
        log::trace!("received light message {light_message:?}");
        let chain_id = c_chain_id.as_ref().to_vec();
        let res = match light_message {
            sdk::light_request::Message::LightHandshake(LightHandshake { buckets }) => {
                if let Some(buckets) = buckets {
                    let bucket_arr: [u8; 20] = buckets
                        .block
                        .try_into()
                        .map_err(|_| NodeError::Message("invalid bucket".to_string()))?;
                    let bucket = Bucket::from_be_bytes(bucket_arr);
                    let _ = spl.send((LightMessage::NewPeer(DhtBuckets { block: bucket }), None));
                    None
                } else {
                    return Err(NodeError::Message("no buckets in handshake".to_string()));
                }
            }
            sdk::light_request::Message::FindValue(FindValue { dht_id, bucket }) => {
                let dht_id = match dht_id {
                    0 => DhtId::Block,
                    _ => return Err(NodeError::Message("unsupported DHT".to_string())),
                };
                let bucket_arr: [u8; 20] = bucket
                    .try_into()
                    .map_err(|_| NodeError::Message("invalid bucket".to_string()))?;
                let bucket = Bucket::from_be_bytes(bucket_arr);
                let (tx, rx) = oneshot::channel();
                spl.send((LightMessage::FindValue(dht_id, bucket), Some(tx)))
                    .map_err(|_| NodeError::SendError)?;
                Some(rx.await?)
            }
            sdk::light_request::Message::FindNode(FindNode { bucket }) => {
                let bucket_arr: [u8; 20] = bucket
                    .try_into()
                    .map_err(|_| NodeError::Message("invalid bucket".to_string()))?;
                let bucket = Bucket::from_be_bytes(bucket_arr);
                let (tx, rx) = oneshot::channel();
                spl.send((LightMessage::FindNode(bucket), Some(tx)))
                    .map_err(|_| NodeError::SendError)?;
                Some(rx.await?)
            }
        };

        match res {
            Some(Ok(light_value)) => match light_value {
                LightValue::ValueOrNodes(value_or_nodes) => {
                    let response: sdk::light_response::Message = match value_or_nodes {
                        ValueOrNodes::Value(value) => sdk::Value { value }.into(),
                        ValueOrNodes::Nodes(data) => p2p::PeerList {
                            claimed_ip_ports: data.into_iter().map(Into::into).collect(),
                        }
                        .into(),
                    };
                    let message = AppResponseMessage::encode(c_chain_id, response, request_id)
                        .map_err(|_| NodeError::Message("failed to encode".to_string()))?;
                    let _ = sender.send(message);
                }
                LightValue::Ok => {
                    let response: sdk::light_response::Message = sdk::Ack {}.into();
                    let message = AppResponseMessage::encode(c_chain_id, response, request_id)
                        .map_err(|_| NodeError::Message("failed to encode".to_string()))?;
                    let _ = sender.send(message);
                }
            },
            Some(Err(LightError { code, message })) => {
                let _ = sender.send(Message::AppError(AppError {
                    chain_id,
                    request_id,
                    error_code: code,
                    error_message: message.to_string(),
                }));
            }
            _ => (),
        }

        Ok(())
    }

    /// Responses handled at a lower level which updates the state if interesting for us.
    async fn manage_light_response(
        spl: &Sender<(LightMessage, Option<oneshot::Sender<LightResult>>)>,
        light_request: sdk::light_request::Message,
        light_response: sdk::light_response::Message,
    ) -> Result<(), NodeError> {
        log::trace!("received light response {light_response:?}");

        match light_response {
            sdk::light_response::Message::Ack(_) => {}
            sdk::light_response::Message::Nodes(p2p::PeerList { claimed_ip_ports }) => {
                if !matches!(
                    light_request,
                    sdk::light_request::Message::FindValue(_)
                        | sdk::light_request::Message::FindNode(_)
                ) {
                    return Err(NodeError::Message("invalid request".to_string()));
                }
                if claimed_ip_ports.len() > 10 {
                    // disconnect and decrease reputation
                    return Err(NodeError::Message("too many nodes".to_string()));
                }
                let nodes: HashSet<_> = claimed_ip_ports
                    .into_iter()
                    .filter_map(|claimed_ip_port| claimed_ip_port.try_into().ok())
                    .collect();
                let message = LightMessage::Nodes(nodes.into_iter().collect());
                spl.send((message, None))
                    .map_err(|_| NodeError::SendError)?;
            }
            sdk::light_response::Message::Value(sdk::Value { value }) => {
                let sdk::light_request::Message::FindValue(FindValue { dht_id, .. }) =
                    light_request
                else {
                    return Err(NodeError::Message("invalid request".to_string()));
                };
                let dht_id = dht_id
                    .try_into()
                    .map_err(|_| NodeError::Message("invalid DHT".to_string()))?;
                let message = LightMessage::Store(dht_id, value);
                spl.send((message, None))
                    .map_err(|_| NodeError::SendError)?;
            }
        }

        Ok(())
    }
}
