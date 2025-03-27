use crate::net::Network;
use crate::node::Node;
use crate::server::{config, Server};
use cli::Args;
use net::node::NodeError;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

mod blocks;
mod cli;
mod client;
mod id;
mod message;
mod net;
mod node;
mod server;
mod stats;
mod utils;

/// Turn the tokio-console subscriber on or off by passing `true` or `false`
#[macro_export]
macro_rules! debugger {
    ($val:expr) => {
        match $val {
            true => {
                console_subscriber::init();
            }
            false => {
                tracing_subscriber::fmt::init();
            }
        }
    };
}

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::main]
async fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    #[cfg(feature = "dhat-ad-hoc")]
    let _profiler = dhat::Profiler::new_ad_hoc();

    debugger!(false);

    let args = cli::read_args();
    let network_config = args.network_config();
    let network = Arc::new(Network::new(network_config).unwrap());

    let node = Arc::new(Node::new(
        network,
        args.max_out_connections,
        args.max_latency_records,
    ));

    let (node_tx, node_ops, server) = server(&node, &args).await;

    let client = tokio::task::spawn(async move {
        client::start(
            &node,
            &args.bootstrappers_path,
            args.max_out_connections,
            &args.network_id.to_string(),
        )
        .await
        .expect("failed to start client"); // TODO better error handling
    });

    #[cfg(feature = "dhat-heap")]
    {
        use std::time::Duration;
        use crate::utils::constants;

        tokio::time::sleep(Duration::from_secs(constants::DHAT_TIME_S)).await;
        drop(_profiler);
        node_tx.send(()).unwrap();
        drop(node_ops);
        drop(server);
        drop(client);
    }

    #[cfg(not(feature = "dhat-heap"))]
    {
        let _ = tokio::join!(node_ops, server, client);

        node_tx.send(()).unwrap();
    }
}

async fn server(
    node: &Arc<Node>,
    args: &Args,
) -> (
    broadcast::Sender<()>,
    JoinHandle<Result<(), NodeError>>,
    JoinHandle<()>,
) {
    let (transaction_tx, transaction_rx) = flume::unbounded();
    let node2 = node.clone();
    let (node_tx, rx) = broadcast::channel(1);
    let enable_metrics = args.enable_metrics;
    let metrics_port = args.metrics_port;
    let node_ops = tokio::task::spawn(async move {
        node2
            .start(enable_metrics, metrics_port, transaction_rx, rx)
            .await
    });

    let server_config = config::server_config(&args.cert_path, &args.pem_key_path);
    let server_config = Arc::new(server_config);
    let node2 = node.clone();
    let ipc_socket_path = args.ipc_socket_path.clone();
    let max_in_connections = args.max_in_connections;
    let server = tokio::task::spawn(async move {
        Server::start(
            node2,
            server_config,
            transaction_tx,
            &ipc_socket_path,
            max_in_connections,
        )
        .await
    });

    (node_tx, node_ops, server)
}
