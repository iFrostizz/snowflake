#![allow(unused)]

use crate::client::bootstrap::Bootstrappers;
use crate::id::NodeId;
use crate::net::{node::NodeError};
use crate::node::Node;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub(crate) mod bootstrap;
pub(crate) mod config;
pub(crate) mod tls;

pub async fn start(
    node: &Arc<Node>,
    bootstrappers_path: &Path,
    max_connections: usize,
    network_name: &str,
) -> Result<(), NodeError> {
    // TODO we need tracing to have these function-level logs
    log::debug!("starting client");

    let boot = Bootstrappers {
        path: bootstrappers_path,
    };

    let res = boot
        .bootstrap_all(node, max_connections, network_name)
        .await;
    let bootstrapped = res.len();
    let errs: Vec<_> = res.into_iter().filter_map(Result::err).collect();
    if !errs.is_empty() && errs.len() == bootstrapped {
        return Err(NodeError::Bootstrap(errs));
    }

    Ok(())
}