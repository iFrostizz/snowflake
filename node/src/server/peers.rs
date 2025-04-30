use crate::net::node::SendErrorWrapper;
use crate::{
    id::NodeId,
    message::{mail_box::Mail, SubscribableMessage},
    net::{node::NodeError, HandshakeInfos},
};
use flume::Sender;
use proto_lib::p2p::{message::Message, Ping};
use tokio::sync::{broadcast, oneshot};

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub x509_certificate: Vec<u8>,
    pub sender: PeerSender,
    pub infos: Option<HandshakeInfos>,
    pub tx: broadcast::Sender<()>,
}

impl PeerInfo {
    pub fn handshook(&self) -> bool {
        self.infos.is_some()
    }

    pub async fn ping(&self, mail_tx: &Sender<Mail>) -> Result<(), NodeError> {
        self.sender.send_without_response(
            mail_tx,
            SubscribableMessage::Ping(Ping {
                uptime: 100,
                subnet_uptimes: vec![],
            }),
        )
    }
}

#[derive(Debug, Clone)]
pub struct PeerSender {
    pub(crate) node_id: NodeId,
    pub(crate) tx: Sender<Message>,
}

impl PeerSender {
    pub fn send(&self, message: Message) -> Result<(), NodeError> {
        Ok(self.tx.send(message).map_err(SendErrorWrapper::from)?)
    }

    /// Send a message supposed to receive an answer.
    // This function works by generating a random request_id and spawns a task that will timeout
    // at the deadline if the response was not received.
    // These messages are: Get, GetAccepted,
    // GetAcceptedFrontier, GetAcceptedStateSummary, GetAncestors, GetPeerList,
    // GetStateSummaryFrontier, PushQuery, PullQuery, AppRequest
    // There is a reserved request_id of 0 for the ping message.
    // We use those to keep track of latency.
    fn send_with_subscribe(
        &self,
        mail_tx: &Sender<Mail>,
        message: SubscribableMessage,
        callback: oneshot::Sender<Message>,
    ) -> Result<(), NodeError> {
        // TODO: the node_id parameter feels a bit duplicated here, does it really makes sense?
        mail_tx
            .send(Mail {
                node_id: self.node_id,
                message: message.clone(),
                callback,
            })
            .map_err(SendErrorWrapper::from)?;

        self.send(message.into())
    }

    // TODO: Instead, provide a generic that automatically resolves to the correct message
    //  enum variant with specific impls. Also, it seems like we could just return the message
    //  instead of the oneshot channel since it's always awaited after calling the function.
    pub fn send_and_response(
        &self,
        mail_tx: &Sender<Mail>,
        message: SubscribableMessage,
    ) -> Result<oneshot::Receiver<Message>, NodeError> {
        let (tx, rx) = oneshot::channel();
        self.send_with_subscribe(mail_tx, message, tx)?;
        Ok(rx)
    }

    pub fn send_without_response(
        &self,
        mail_tx: &Sender<Mail>,
        message: SubscribableMessage,
    ) -> Result<(), NodeError> {
        self.send_with_subscribe(mail_tx, message, {
            let (tx, _) = oneshot::channel();
            tx
        })?;
        Ok(())
    }
}
