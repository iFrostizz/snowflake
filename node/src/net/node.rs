use std::path::Path;
use crate::client::config;
use crate::id::{ChainId, NodeId};
use crate::message::mail_box::MailBox;
use crate::message::{pipeline::Pipeline, MiniMessage};
use crate::net::queue::ConnectionQueue;
use crate::net::{ip::SignedIp, ip::UnsignedIp, BackoffParams, Intervals, Network, PeerInfo};
use crate::server::{
    msg::{DecodingError, OutboundMessage},
    peers::PeerSender,
    tcp::write_stream_message,
};
use crate::stats;
use crate::utils::{
    bloom::{BloomError, Filter},
    bls::Bls,
    ip::ip_octets,
};
use flume::{Receiver, Sender};
use futures::future;
use indexmap::IndexMap;
use openssl::pkey::PKey;
use prost::EncodeError;
use proto_lib::p2p::message::Message;
use proto_lib::p2p::{self};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::io::WriteHalf;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, oneshot, Semaphore};
use tokio::task::JoinHandle;
use tokio::time;
use tokio_rustls::TlsStream;
use tracing::instrument;

fn validate_credentials(config: &NetworkConfig) -> Result<(), NodeError> {
    let cert_bytes = std::fs::read(&config.cert_path)?;
    let cert = openssl::x509::X509::from_pem(&cert_bytes)?;
    let cert_pub = cert.public_key()?.public_key_to_der()?;

    let key_bytes = std::fs::read(&config.pem_key_path)?;
    let key = PKey::private_key_from_pem(&key_bytes)
        .or_else(|_| PKey::private_key_from_pkcs8(&key_bytes))?;
    let key_pub = key.public_key_to_der()?;

    if cert_pub != key_pub {
        return Err(NodeError::Message(
            "certificate does not match private key".to_string(),
        ));
    }

    let bls_bytes = std::fs::read(&config.bls_key_path)?;
    if bls_bytes.len() != 32 {
        return Err(NodeError::Message(format!(
            "invalid bls key length: expected 32 bytes, got {}",
            bls_bytes.len()
        )));
    }

    Ok(())
}

fn verify_signed_ip(cert_path: &Path, signed_ip: &SignedIp) -> Result<(), NodeError> {
    let cert_bytes = std::fs::read(cert_path)?;
    let cert = openssl::x509::X509::from_pem(&cert_bytes)?;
    let public_key = cert.public_key()?;
    let valid = signed_ip
        .unsigned_ip
        .verify(&signed_ip.ip_sig, &public_key)?;
    if !valid {
        return Err(NodeError::Message(
            "invalid signed IP: certificate signature mismatch".to_string(),
        ));
    }
    Ok(())
}

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
    pub bootstrappers: HashSet<NodeId>,
    pub max_latency_records: usize,
    pub max_out_connections: usize,
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
        validate_credentials(&config)?;
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
        verify_signed_ip(&config.cert_path, &signed_ip)?;

        // TODO https://github.com/iFrostizz/snowflake/issues/13
        let client = p2p::Client {
            name: String::from("avalanchego"),
            major: 1,
            minor: 14,
            patch: 1,
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
        })
    }

    /// Continuously write messages and return an error on an EOF
    pub async fn schedule_write_messages(
        node_id: NodeId,
        out_pipeline: Arc<Pipeline>,
        write: WriteHalf<TlsStream<TcpStream>>,
        rnp: Receiver<Message>,
        mut disconnection_rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        let (ptx, prx) = flume::unbounded();

        let (write_tx, rx) = oneshot::channel();
        let write_messages = tokio::spawn(async move {
            Self::write_messages(node_id, write, prx, rx).await
        });

        let (queue_tx, rx) = oneshot::channel();
        let queue_messages = tokio::spawn(async move {
            Self::queue_messages(node_id, out_pipeline.clone(), rnp.clone(), ptx, rx).await
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

    #[instrument(skip_all, fields(node_id = %node_id))]
    async fn write_messages(
        node_id: NodeId,
        mut write: WriteHalf<TlsStream<TcpStream>>,
        prx: Receiver<Vec<u8>>,
        mut rx: oneshot::Receiver<()>,
    ) -> Result<(), NodeError> {
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
    }

    #[instrument(skip_all, fields(node_id = %node_id))]
    async fn queue_messages(
        node_id: NodeId,
        out_pipeline: Arc<Pipeline>,
        rnp: Receiver<Message>,
        ptx: Sender<Vec<u8>>,
        mut rx: oneshot::Receiver<()>,
    ) -> Result<(), NodeError> {
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

    #[instrument(skip_all, fields(node_id = %node_id))]
    pub fn handshake_peer(
        self: &Arc<Network>,
        sender: &PeerSender,
        node_id: NodeId,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<JoinHandle<Result<(), NodeError>>, NodeError> {
        self.handshake(sender)?;

        let network = self.clone();
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
            // TODO sync with networks
            upgrade_time: 1763568000,
            ip_signing_time: network.signed_ip.unsigned_ip.timestamp,
            ip_node_id_sig: network.signed_ip.ip_sig.clone(),
            tracked_subnets: Vec::new(),
            client: Some(network.client.clone()),
            // supported_acps: vec![23, 24, 25, 30, 31, 41, 62],
            supported_acps: vec![],
            objected_acps: Vec::new(),
            known_peers: Some(bloom_filter),
            ip_bls_sig: network.signed_ip.ip_bls_sig.clone(),
            all_subnets: true,
        };

        log::trace!("handshaking the peer");
        sender.send(Message::Handshake(handshake))
    }

    pub fn remove_peers(
        peers_infos: Arc<RwLock<IndexMap<NodeId, PeerInfo>>>,
        node_ids_errs: Vec<(NodeId, Option<NodeError>)>,
    ) {
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

    pub fn is_bootstrapper(&self, node_id: &NodeId) -> bool {
        self.bootstrappers.read().unwrap().contains(node_id)
    }
}
