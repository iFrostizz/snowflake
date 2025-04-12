use crate::node::Node;
use crate::server::{listener::Listener, rpc::Rpc};
use flume::Sender;
use rustls::ServerConfig;
use std::{sync::Arc, time::Instant};
use tokio::sync::oneshot;

pub mod config;
pub mod listener;
pub mod msg;
pub mod peers;
pub mod rpc;
pub mod tcp;
pub mod tls;

pub struct Server {
    //
}

impl Server {
    pub async fn start(
        node: Arc<Node>,
        config: Arc<ServerConfig>,
        tx: Sender<(Vec<u8>, Instant)>,
        rpc_port: u16,
        max_in_connections: usize,
    ) {
        let network = node.network.clone();
        let listener = tokio::spawn(async move {
            Listener::start(&node, config, max_in_connections).await;
        });

        let rpc = Rpc::new(network, rpc_port, tx).await;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let rpc_server = tokio::spawn(async move {
            rpc.start(shutdown_rx).await;
        });

        let _ = tokio::try_join!(listener, rpc_server);
        let _ = shutdown_tx.send(());
    }
}
