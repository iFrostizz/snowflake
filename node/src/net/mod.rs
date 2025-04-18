use crate::id::NodeId;
use crate::message::{mail_box::MailBox, pipeline::Pipeline, MiniMessage, SubscribableMessage};
use crate::net::{
    ip::SignedIp,
    node::{NetworkConfig, NodeError},
};
use crate::server::{
    msg::InboundMessage,
    peers::{PeerInfo, PeerSender},
};
use crate::utils::bls::Bls;
use crate::utils::{bloom::Filter, ip::ip_from_octets, packer::Packer};
use async_recursion::async_recursion;
use flume::Sender;
use indexmap::IndexMap;
use proto_lib::p2p::{self, message::Message, BloomFilter, Client, GetPeerList, Handshake};
use ripemd::Digest;
use rustls::ClientConfig;
use rustls_pki_types::ServerName;
use sha2::Sha256;
use std::io::BufReader;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{split, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio_rustls::{TlsConnector, TlsStream};

pub mod ip;
pub mod latency;
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
    pub peers_infos: RwLock<IndexMap<NodeId, PeerInfo>>, // TODO can we find a way to do it lock-less ?
    pub out_pipeline: Arc<Pipeline>,
    /// The canonically sorted validators map
    pub bloom_filter: RwLock<Filter>,
    pub public_key: [u8; Bls::PUBLIC_KEY_BYTES],
    pub node_pop: Vec<u8>,
    pub handshake_semaphore: Arc<Semaphore>,
}

/// Intervals of operations in milliseconds
#[derive(Debug)]
pub struct Intervals {
    pub ping: u64,
    pub get_peer_list: u64,
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

/// A peer bi-directional connection. It can either be initiated by the node or by a distant peer.
#[derive(Debug)]
pub struct Peer {
    pub node_id: NodeId,
    pub x509_certificate: Vec<u8>,
    #[allow(unused)]
    pub sock_addr: SocketAddr,
    #[allow(unused)]
    pub timestamp: u64,
    tls: Option<TlsStream<TcpStream>>,
}

impl Peer {
    pub fn new(
        node_id: NodeId,
        x509_certificate: Vec<u8>,
        sock_addr: SocketAddr,
        timestamp: u64,
        tls: TlsStream<TcpStream>,
    ) -> Self {
        Self {
            node_id,
            x509_certificate,
            sock_addr,
            timestamp,
            tls: Some(tls),
        }
    }

    pub fn take_tls(
        &mut self,
    ) -> (
        ReadHalf<TlsStream<TcpStream>>,
        WriteHalf<TlsStream<TcpStream>>,
    ) {
        split(self.tls.take().expect("missing tls field"))
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

    /// What should we do when receiving a message from another peer ?
    #[async_recursion]
    pub async fn manage_message<'a>(
        node_id: &NodeId,
        buf: &[u8],
        sender: &PeerSender,
        mail_box: &MailBox,
        spn: &Sender<PeerMessage>,
        recursed: bool,
    ) -> Result<(), NodeError> {
        let decoded = InboundMessage::decode(buf).map_err(NodeError::Decoding)?;

        let mini = MiniMessage::from(&decoded);
        if !matches!(decoded, Message::CompressedZstd(_)) {
            log::trace!("received message {}", &mini);
            mini.inc_recv(buf.len() as u64);
        }

        if let Some(request_id) = SubscribableMessage::response_request_id(&decoded) {
            mail_box.mark_mail_received(node_id, request_id)
        } else {
            None
        };

        // TODO if this node holds a stake, here is the minimum amount of messages to handle since
        // TODO they are registered and will get the node benched:
        // TODO AppRequest, PullQuery, PushQuery, Get, GetAncestors, GetAccepted, GetAcceptedFrontier, GetAcceptedStateSummary, GetStateSummaryFrontier
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

                Self::manage_message(node_id, &decoded_buf, sender, mail_box, spn, true).await?;

                return Ok(());
            }
            Message::Ping(ping) => {
                spn.send(PeerMessage::ObserveUptime(ping))
                    .map_err(|_| NodeError::SendError)?;

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
                spn.send(PeerMessage::GetPeerList {
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

                spn.send(PeerMessage::NewPeer {
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
                spn.send(PeerMessage::PeerList(peer_list))
                    .map_err(|_| NodeError::SendError)?;
            }
            Message::GetPeerList(GetPeerList { known_peers }) => {
                spn.send(PeerMessage::GetPeerList {
                    sender: sender.clone(),
                    known_peers,
                })
                .map_err(|_| NodeError::SendError)?;
            }
            Message::Pong(_pong) => {}
            _ => log::debug!("unsupported message {} {node_id}", mini),
        };

        Ok(())
    }
}
