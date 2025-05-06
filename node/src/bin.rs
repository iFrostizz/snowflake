use crate::node::Node;
use crate::server::listener::Listener;
use crate::server::{config, Server};
use cli::Args;
use net::node::NodeError;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

mod blocks;
mod cli;
mod client;
mod dht;
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
async fn main() -> Result<(), NodeError> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    #[cfg(feature = "dhat-ad-hoc")]
    let _profiler = dhat::Profiler::new_ad_hoc();

    debugger!(false);

    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut args = cli::read_args().await?;
    log::debug!("args: {:?}", args);

    let server_config = config::server_config(&args.cert_path, &args.pem_key_path);
    let server_config = Arc::new(server_config);
    let listener = Listener::new(args.http_port, server_config, args.max_in_connections).await;
    args.http_port = listener.network_port;

    let network_config = args.network_config();

    let node = Arc::new(Node::new(
        network_config,
        args.max_out_connections,
        args.max_latency_records,
        args.sync_headers,
    ));
    node.light_network.block_dht.todo_attach_node(node.clone());

    let (node_tx, node_ops, server) = server(node.clone(), listener, &args).await;

    let client = tokio::task::spawn(async move {
        client::start(
            node,
            &args.bootstrappers_path,
            &args.light_bootstrappers_path,
            &args.network_id.to_string(),
        )
        .await
    });

    #[cfg(feature = "dhat-heap")]
    {
        use crate::utils::constants;
        use std::time::Duration;

        tokio::time::sleep(Duration::from_secs(constants::DHAT_TIME_S)).await;
        drop(_profiler);
        node_tx.send(()).unwrap();
        drop(node_ops);
        drop(server);
        drop(client);
    }

    #[cfg(not(feature = "dhat-heap"))]
    {
        let res = tokio::try_join!(node_ops, server, client);

        node_tx.send(()).unwrap();

        match res {
            Ok((Err(e), ..)) | Ok((Ok(_), Err(e), ..)) | Ok((.., Err(e))) => return Err(e),
            Ok((Ok(_), Ok(_), Ok(_))) => (),
            Err(e) => panic!("{:?}", e),
        }
    }

    Ok(())
}

async fn server(
    node: Arc<Node>,
    listener: Listener,
    args: &Args,
) -> (
    broadcast::Sender<()>,
    JoinHandle<Result<(), NodeError>>,
    JoinHandle<Result<(), NodeError>>,
) {
    log::debug!("starting server");

    let (transaction_tx, transaction_rx) = flume::unbounded();
    let node2 = node.clone();
    let (node_tx, node_rx) = broadcast::channel(1);
    let enable_metrics = args.enable_metrics;
    let metrics_port = args.metrics_port;
    let node_ops = tokio::task::spawn(async move {
        node2
            .start(enable_metrics, metrics_port, transaction_rx, node_rx)
            .await
    });

    let node2 = node.clone();
    let rpc_port = args.rpc_port;
    let server = tokio::task::spawn(async move {
        Server::start(node2, listener, transaction_tx, rpc_port).await
    });

    (node_tx, node_ops, server)
}
