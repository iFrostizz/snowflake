// a TCP listener which dispatch and handles connections

use crate::id::NodeId;
use crate::net::Peer;
use crate::node::Node;
use rustls::ServerConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio_rustls::{TlsAcceptor, TlsStream};

pub struct Listener {
    config: Arc<ServerConfig>,
    tcp: TcpListener,
    max_connections: usize,
    pub network_port: u16,
}

impl Listener {
    /// Instantiates the `Listener`. If the `network_port` is 0, it will be automatically set.
    pub async fn new(
        mut network_port: u16,
        config: Arc<ServerConfig>,
        max_connections: usize,
    ) -> Self {
        let address = format!("{}:{}", "127.0.0.1", network_port);
        let tcp = TcpListener::bind(address)
            .await
            .expect("failed to start tcp server");
        network_port = tcp.local_addr().unwrap().port();

        Self {
            config,
            tcp,
            max_connections,
            network_port,
        }
    }

    pub async fn start(self, node: Arc<Node>) -> ! {
        log::debug!("starting listening server");
        log::debug!("tcp listener bound");
        let tls_acceptor = Arc::new(TlsAcceptor::from(self.config));
        let connections = Arc::new(Semaphore::new(self.max_connections));

        loop {
            let node = node.clone();
            let tls_acceptor = tls_acceptor.clone();
            match self.tcp.accept().await {
                Ok((stream, sock_addr)) => {
                    let handle = connections.try_acquire();
                    if handle.is_ok() {
                        tokio::spawn(async move {
                            Self::manage_tls_incoming(node, tls_acceptor, stream, sock_addr).await;
                        });
                    } else {
                        log::debug!("rejecting before accepting more concurrent connections");
                        continue;
                    }
                }
                Err(err) => log::debug!("on accepting TCP stream {err:?}"),
            }
        }
    }

    async fn manage_tls_incoming(
        node: Arc<Node>,
        tls_acceptor: Arc<TlsAcceptor>,
        stream: TcpStream,
        sock_addr: SocketAddr,
    ) {
        match tls_acceptor.accept(stream).await {
            Ok(tls_stream) => {
                let server_connection = tls_stream.get_ref().1;
                let certs = server_connection.peer_certificates();
                let Some(certs) = certs else {
                    log::debug!("no peer certs");
                    return;
                };
                let Some(cert) = certs.iter().next() else {
                    log::debug!("no certificate from listened connection");
                    return;
                };

                let x509_certificate = cert.as_ref().to_vec();
                let node_id = NodeId::from_cert(&x509_certificate);

                // TODO support peer replacements
                if let Err(err) = node.network.check_add_peer(&node_id) {
                    log::debug!("{node_id}, {err}");
                    return;
                }

                let tls = TlsStream::Server(tls_stream);
                let peer = Peer::new(node_id, x509_certificate, sock_addr, 0, tls);

                let hs_permit = node.hs_permit().await;

                if let Err(err) = node.loop_peer(hs_permit, peer, None).await {
                    log::debug!("{err}");
                }
            }
            Err(err) => log::debug!("on accepting TLS stream {err:?}"),
        }
    }
}
