use crate::client::config;
use crate::id::{ChainId, NodeId};
use crate::message::{mail_box::MailBox, pipeline::Pipeline, MiniMessage};
use crate::net::{ip::UnsignedIp, BackoffParams, Intervals, Network, Peer, PeerInfo, PeerMessage};
use crate::server::{
    msg::{DecodingError, OutboundMessage},
    peers::PeerSender,
    tcp::{read_stream_message, write_stream_message},
};
use crate::stats;
use crate::utils::{
    bloom::{BloomError, Filter},
    bls::Bls,
    constants,
    ip::ip_octets,
};
use flume::{Receiver, Sender};
use futures::future;
use indexmap::IndexMap;
use openssl::x509;
use prost::EncodeError;
use proto_lib::p2p::{self};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, oneshot, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{self};
use tokio_rustls::TlsStream;

#[derive(Error, Debug)]
pub enum NodeError {
    #[error("dns conversion failed")]
    Dns,
    #[error("future timeout: {0}")]
    Timeout(#[from] time::error::Elapsed),
    #[error("tcp error: {0}")]
    TcpConnection(#[from] std::io::Error),
    #[error("send error: all receivers have been dropped")]
    SendError,
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
    #[error("unexpected message: {0}")]
    Message(String),
}

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

impl Network {
    /// Initiate the network by specifying this node's IP
    pub fn new(config: NetworkConfig) -> Result<Self, ()> {
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
        let signed_ip = unsigned_ip.sign_with_key(&bls, &config.pem_key_path);

        // TODO https://github.com/iFrostizz/snowflake/issues/13
        let client = p2p::Client {
            name: String::from("avalanchego"),
            major: 1,
            minor: 13,
            patch: 0,
        };

        let bloom_filter = Filter::new(8, 1000).expect("usage of wrong constants");
        let bloom_filter = RwLock::new(bloom_filter);

        let bytes = std::fs::read(&config.cert_path).expect("failed to read cert");
        let x509 = x509::X509::from_pem(&bytes).unwrap();
        let cert = x509.to_der().unwrap();
        let node_id = NodeId::from_cert(cert);

        let out_pipeline = Arc::new(Pipeline::new(
            config.max_throughput,
            config.max_out_queue_size,
            config.bucket_size,
        ));

        let handshake_semaphore = Arc::new(Semaphore::new(config.max_concurrent_handshakes));

        Ok(Self {
            node_id,
            out_pipeline,
            config,
            client,
            client_config,
            peers_infos: RwLock::new(IndexMap::new()),
            signed_ip,
            bloom_filter,
            public_key,
            node_pop,
            handshake_semaphore,
        })
    }

    /// Continuously write messages and return an error on an EOF
    pub async fn schedule_write_messages(
        out_pipeline: Arc<Pipeline>,
        mut write: WriteHalf<TlsStream<TcpStream>>,
        rnp: Receiver<p2p::message::Message>,
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
                            if let Ok(outbound_message) = OutboundMessage::create(message) {
                                let bytes = outbound_message.bytes;
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

    /// Continuously read messages and return an error on an EOF
    pub async fn read_messages(
        node_id: &NodeId,
        mut read: ReadHalf<TlsStream<TcpStream>>,
        sender: &PeerSender,
        mail_box: &MailBox,
        spn: &Sender<PeerMessage>,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        loop {
            log::trace!("read");
            tokio::select! {
                maybe_buf = read_stream_message(&mut read) => {
                    let buf = maybe_buf?;
                    Peer::manage_message(node_id, &buf, sender, mail_box, spn, false).await?;
                }
                _ = rx.recv() => {
                    return Ok(())
                }
            }
        }
    }

    pub async fn add_peer(
        self: &Arc<Network>,
        node_id: NodeId,
        x509_certificate: Vec<u8>,
        snp: PeerSender,
    ) {
        let mut peers = self.peers_infos.write().unwrap();
        if peers.get(&node_id).is_none() {
            peers.insert(
                node_id,
                PeerInfo {
                    x509_certificate,
                    sender: snp, // TODO issue here, the passed snp won't be used if already here
                    infos: None,
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
        let handshake_deadline = Duration::from_millis(constants::DEFAULT_DEADLINE); // s // TODO configure
        self.handshake(sender)?;
        let sleep = tokio::time::sleep(handshake_deadline);
        let network = self.clone();

        let hand_peer = tokio::spawn(async move {
            tokio::select! {
                _ = sleep => {
                    // timeout, we might have received the handshake before though
                    // TODO maybe use a loop to continuously check
                    let peer_infos = network.peers_infos.read().unwrap();
                    if !peer_infos.get(&node_id).is_some_and(|peer| peer.handshook()) {
                        return Err(NodeError::Message("handshake expired".to_string()));
                    }
                }
                _ = rx.recv() => {
                    return Ok(())
                }
            }
            // the handshake was successful, the channel can still stop this thread remotely
            rx.recv().await.unwrap();
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
        sender
            .send(p2p::message::Message::Handshake(handshake))
            .map_err(|_| NodeError::SendError)
    }

    pub fn remove_peers(self: &Arc<Network>, node_ids_errs: &Vec<(NodeId, Option<&NodeError>)>) {
        for (node_id, err) in node_ids_errs {
            if let Some(err) = err {
                log::debug!("removing peer {}, reason: {}", node_id, err);
            } else {
                log::debug!("removing peer {} for an unknown reason", node_id);
            }
        }
        let mut peers_write = self.peers_infos.write().unwrap();

        for (node_id, _) in node_ids_errs {
            if let Some(peer) = peers_write.swap_remove(node_id) {
                if peer.handshook() {
                    stats::handshook_peers::dec();
                }
                stats::connected_peers::dec();
            }
        }
    }

    pub fn disconnect_peer(
        self: &Arc<Network>,
        peer: Peer,
        err: Option<&NodeError>,
    ) -> Result<(), NodeError> {
        let node_id = peer.node_id;

        self.remove_peers(&vec![(node_id, err)]);

        Ok(())
    }

    pub fn has_reached_max_peers(&self, peers_infos: &IndexMap<NodeId, PeerInfo>) -> bool {
        match self.config.max_peers {
            Some(max_peers) => peers_infos.len() >= max_peers,
            None => false,
        }
    }

    pub fn can_add_peer(&self, node_id: &NodeId) -> bool {
        let peers_infos = self.peers_infos.read().unwrap();
        !self.has_reached_max_peers(&peers_infos) && !peers_infos.contains_key(node_id)
    }
}
