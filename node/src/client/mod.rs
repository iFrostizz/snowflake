use crate::client::bootstrap::Bootstrappers;
use crate::id::NodeId;
use crate::net::node::NodeError;
use crate::node::Node;
use std::path::Path;
use std::sync::Arc;

pub(crate) mod bootstrap;
pub(crate) mod config;
pub(crate) mod tls;

pub async fn start(
    node: &Arc<Node>,
    bootstrappers_path: &Path,
    max_connections: usize,
    network_name: &str,
) -> Result<Vec<NodeId>, NodeError> {
    // TODO we need tracing to have these function-level logs
    log::debug!("starting client");

    let boot = Bootstrappers::new(bootstrappers_path);
    let res = boot
        .bootstrap_all(node, max_connections, network_name)
        .await;
    let bootstrapped = res.len();
    let (oks, errs): (Vec<_>, Vec<_>) = res.into_iter().partition(Result::is_ok);
    let node_ids: Vec<_> = oks.into_iter().map(Result::unwrap).collect();
    let errs: Vec<_> = errs.into_iter().map(Result::unwrap_err).collect();
    if !errs.is_empty() && errs.len() == bootstrapped {
        return Err(NodeError::Bootstrap(errs));
    }

    Ok(node_ids)
}
