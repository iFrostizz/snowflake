use crate::{
    id::NodeId,
    message::{mail_box::Mail, SubscribableMessage},
    net::{node::NodeError, HandshakeInfos},
};
use flume::Sender;
use proto_lib::p2p::{message::Message, Ping};

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
    pub fn send_with_subscribe(
        &self,
        mail_tx: &Sender<Mail>,
        node_id: NodeId,
        message: SubscribableMessage,
    ) -> Result<(), NodeError> {
        mail_tx
            .send(Mail {
                node_id,
                message: message.clone(),
            })
            .map_err(|_| NodeError::SendError)?;

        self.send(message.into())
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
