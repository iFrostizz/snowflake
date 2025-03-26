// The mailbox holds all messages which expects a response.
// It's a data structure that registers those messages and retrieve them when received.

use super::SubscribableMessage;
use crate::{id::NodeId, net::latency::PeersLatency};
use flume::{Receiver, Sender};
use proto_lib::p2p::message::Message;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, oneshot};

pub struct Mail {
    pub node_id: NodeId,
    pub message: SubscribableMessage,
}

pub struct MailBox {
    mails: Mails,
    peers_latency: Arc<RwLock<PeersLatency>>,
    tx: Sender<Mail>,
    rx: Receiver<Mail>,
}

// TODO name doesn't make sense anymore
type Mails = Arc<Mutex<HashMap<NodeId, HashMap<u32, (oneshot::Sender<()>, SubscribableMessage)>>>>;

impl MailBox {
    pub fn new(max_latency_records: usize) -> MailBox {
        let (tx, rx) = flume::unbounded();

        Self {
            mails: Arc::new(Mutex::new(HashMap::new())),
            peers_latency: Arc::new(RwLock::new(PeersLatency::new(max_latency_records))),
            tx,
            rx,
        }
    }

    /// Start the mailbox by listening to mails. They will be forwarded to the internal data once received.
    pub async fn start(&self, mut rx: broadcast::Receiver<()>) {
        loop {
            tokio::select! {
                res = self.rx.recv_async() => {
                    if let Ok(Mail { node_id, message }) = res {
                        let timeout = Duration::from_nanos(message.deadline());
                        let mails = self.mails.clone();
                        let peers_latency = self.peers_latency.clone();
                        tokio::spawn(async move {
                            Self::store_mail(timeout, mails, node_id, message, peers_latency).await;
                        });
                    }
                }
                _ = rx.recv() => {
                    return;
                }
            }
        }
    }

    pub fn tx(&self) -> &Sender<Mail> {
        &self.tx
    }

    pub fn peers_latency(&self) -> &Arc<RwLock<PeersLatency>> {
        &self.peers_latency
    }

    /// Store a [`SubscribableMessage`] sent to a [`NodeId`] and return a tx handle that should be used to delete it once the response is received.
    /// The mail can only last a maximum of `duration`.
    pub async fn store_mail(
        duration: Duration,
        mails: Mails,
        node_id: NodeId,
        message: SubscribableMessage,
        peers_latency: Arc<RwLock<PeersLatency>>,
    ) {
        let (tx, rx) = oneshot::channel();

        let request_id = *message.request_id();
        mails
            .lock()
            .unwrap()
            .entry(node_id)
            .or_default()
            .insert(request_id, (tx, message));

        let start = Instant::now();

        let res = tokio::time::timeout(duration, async move {
            let _ = rx.await;
            let lat = Instant::now().duration_since(start);
            peers_latency.write().unwrap().record(node_id, lat);
        })
        .await;

        let mut mails = mails.lock().unwrap();
        let set = mails.get_mut(&node_id).unwrap();

        // has timed out, we need to force clean
        if res.is_err() {
            let (_, message) = set.remove(&request_id).unwrap();
            message.on_timeout();
        }

        if set.is_empty() {
            debug_assert!(mails.remove(&node_id).is_some());
        }
    }

    /// Mark a message as received and return the sent messages if it had not timed out
    pub fn mark_mail_received(&self, node_id: &NodeId, request_id: &u32) -> Option<Message> {
        if let Some((tx, message)) = self
            .mails
            .lock()
            .unwrap()
            .get_mut(node_id)
            .and_then(|map| map.remove(request_id))
        {
            let _ = tx.send(());
            Some(message.into())
        } else {
            None
        }
    }
}
