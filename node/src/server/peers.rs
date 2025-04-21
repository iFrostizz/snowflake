use crate::{
    id::NodeId,
    message::{mail_box::Mail, SubscribableMessage},
    net::{node::NodeError, HandshakeInfos},
};
use flume::Sender;
use proto_lib::p2p::{message::Message, Ping};
use tokio::sync::oneshot;

// TODO better name
#[derive(Debug)]
pub struct PeerLessInfo {
    pub sender: PeerSender,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub x509_certificate: Vec<u8>,
    pub sender: PeerSender,
    pub infos: Option<HandshakeInfos>,
}

impl PeerInfo {
    pub fn handshook(&self) -> bool {
        self.infos.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct PeerSender(Sender<Message>);

impl PeerSender {
    pub fn send(&self, message: Message) -> Result<(), NodeError> {
        self.0.send(message).map_err(|_| NodeError::SendError)
    }

    /// Send a message that is supposed to receive an answer.
    // This function works by generating a random request_id and spawn a task that will timeout at the deadline if the response was not received.
    // These messages are: Get, GetAccepted, GetAcceptedFrontier, GetAcceptedStateSummary, GetAncestors, GetPeerList, GetStateSummaryFrontier, PushQuery, PullQuery, AppRequest
    fn send_with_subscribe(
        &self,
        mail_tx: &Sender<Mail>,
        node_id: NodeId,
        message: SubscribableMessage,
        callback: oneshot::Sender<Message>,
    ) -> Result<(), NodeError> {
        // TODO: the node_id parameter feels a bit duplicated here, does it really makes sense?
        mail_tx
            .send(Mail {
                node_id,
                message: message.clone(),
                callback,
            })
            .map_err(|_| NodeError::SendError)?;

        self.send(message.into())
    }

    pub fn send_and_response(
        &self,
        mail_tx: &Sender<Mail>,
        node_id: NodeId,
        message: SubscribableMessage,
    ) -> Result<oneshot::Receiver<Message>, NodeError> {
        let (tx, rx) = oneshot::channel();
        self.send_with_subscribe(mail_tx, node_id, message, tx)?;
        Ok(rx)
    }
}

impl From<Sender<Message>> for PeerSender {
    fn from(value: Sender<Message>) -> Self {
        PeerSender(value)
    }
}

impl PeerLessInfo {
    pub fn send(&self, message: Message) -> Result<(), NodeError> {
        self.sender.send(message)
    }

    pub fn ping(&self) -> Result<(), NodeError> {
        self.send(Message::Ping(Ping {
            uptime: 100,
            subnet_uptimes: Vec::new(),
        }))
        .map_err(|_| NodeError::SendError)
    }
}
