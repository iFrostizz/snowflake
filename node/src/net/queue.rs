use crate::id::NodeId;
use crate::node::Node;
use flume::{Receiver, Sender};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{broadcast, Semaphore};

pub struct ConnectionData {
    pub node_id: NodeId,
    pub socket_addr: SocketAddr,
    #[allow(unused)]
    pub timestamp: u64,
    #[allow(unused)]
    pub x509_certificate: Vec<u8>,
}

/// A connection queue to manage and control concurrent connections
/// This is to prevent too much concurrent connection and to remember about those which
/// have been tried so far.
pub struct ConnectionQueue {
    semaphore: Arc<Semaphore>,
    connections: RwLock<HashMap<NodeId, usize>>,
    rcd: Receiver<ConnectionData>,
    scd: Sender<ConnectionData>,
}

impl ConnectionQueue {
    #[allow(unused)]
    /// The time the queue should wait for before retrying a connection
    const RETRY_DEADLINE: Duration = Duration::from_secs(30);

    const MAX_RETRIES: usize = 3;

    pub fn new(max_concurrent: usize) -> Self {
        let (scd, rcd) = flume::unbounded();

        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            connections: RwLock::new(HashMap::new()),
            rcd,
            scd,
        }
    }

    /// Continuously checks for newly added connections to the queue.
    /// It tries to connect to the oldest connections and never exceeds
    /// the max amount of concurrent connections.
    pub async fn watch_connections(&self, node: &Arc<Node>, mut rx: broadcast::Receiver<()>) {
        loop {
            tokio::select! {
                res = self.rcd.recv_async() => {
                    if let Ok(data) = res {
                        let node = node.clone();
                        let semaphore = self.semaphore.clone();
                        if !node.network.can_add_peer(&data.node_id) {
                            log::debug!("cannot add peer {}", &data.node_id);
                        } else {
                            tokio::spawn(async move {
                                if let Err(err) = node
                                    .create_connection(semaphore, data)
                                    .await
                                {
                                    log::debug!("err when creating connection {err}");
                                }
                            });
                        }
                    } else {
                        return;
                    }
                }
                _ = rx.recv() => {
                    return;
                }
            }
        }
    }

    pub fn mark_connected(&self, node_id: &NodeId) {
        self.connections.write().unwrap().remove(node_id);
    }

    /// Schedule a connection that will be executed once that the semaphore will be acquired
    /// returns true if it was added
    pub fn maybe_add_connection(&self, data: ConnectionData) -> bool {
        let maybe_retries = self.connections.read().unwrap().get(&data.node_id).cloned();
        match maybe_retries {
            None => false,
            Some(retries) => {
                if retries >= Self::MAX_RETRIES {
                    self.connections.write().unwrap().remove(&data.node_id);
                    false
                } else {
                    self._add_connection(data, retries + 1);
                    true
                }
            }
        }
    }

    pub fn add_connection(&self, data: ConnectionData) -> bool {
        let maybe_retries = self.connections.read().unwrap().get(&data.node_id).cloned();
        match maybe_retries {
            None => {
                self._add_connection(data, 0);
                true
            }
            Some(_) => false,
        }
    }

    fn _add_connection(&self, data: ConnectionData, retries: usize) {
        self.connections
            .write()
            .unwrap()
            .insert(data.node_id, retries);
        self.scd.send(data).expect("receivers dropped");
    }
}
