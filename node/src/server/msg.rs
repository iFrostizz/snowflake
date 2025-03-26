use prost::{DecodeError, EncodeError, Message};
use proto_lib::p2p;
use thiserror::Error;

#[derive(Debug)]
pub struct OutboundMessage {
    pub bytes: Vec<u8>,
}

#[derive(Default)]
enum CompressionType {
    #[default]
    None,
}

pub(crate) const DELIMITER_LEN: u32 = 4;

impl OutboundMessage {
    pub fn create(message: p2p::message::Message) -> Result<Self, EncodeError> {
        let message = p2p::Message {
            message: Some(message),
        };
        let encoded_len: usize = message.encoded_len();
        let len: u32 = encoded_len.try_into().unwrap_or(u32::MAX);
        let mut buf = Vec::with_capacity(DELIMITER_LEN as usize + encoded_len);
        buf.append(&mut len.to_be_bytes().to_vec());
        message.encode(&mut buf)?;

        Ok(Self { bytes: buf })
    }

    #[allow(unused)]
    pub fn size(&self) -> usize {
        self.bytes.len()
    }
}

pub struct InboundMessage;

#[derive(Error, Debug)]
pub enum DecodingError {
    #[error("prost decode error: {0}")]
    Prost(#[from] DecodeError),
    #[error("message is empty")]
    EmptyMessage,
}

impl InboundMessage {
    pub fn decode(message: &[u8]) -> Result<p2p::message::Message, DecodingError> {
        let len = message.len();
        if len == 0 {
            return Err(DecodingError::EmptyMessage);
        }

        let decoded = p2p::Message::decode(message).map_err(DecodingError::Prost)?;

        decoded.message.ok_or(DecodingError::EmptyMessage)
    }
}
