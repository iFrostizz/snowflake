pub mod p2p;
pub mod sdk;
pub mod vm;

impl From<sdk::FindValue> for sdk::light_request::Message {
    fn from(message: sdk::FindValue) -> Self {
        Self::FindValue(message)
    }
}

impl From<sdk::FindNode> for sdk::light_request::Message {
    fn from(message: sdk::FindNode) -> Self {
        Self::FindNode(message)
    }
}

impl From<sdk::Value> for sdk::light_response::Message {
    fn from(message: sdk::Value) -> Self {
        Self::Value(message)
    }
}

impl From<p2p::PeerList> for sdk::light_response::Message {
    fn from(message: p2p::PeerList) -> Self {
        Self::Nodes(message)
    }
}

impl From<sdk::Ack> for sdk::light_response::Message {
    fn from(message: sdk::Ack) -> Self {
        Self::Ack(message)
    }
}
