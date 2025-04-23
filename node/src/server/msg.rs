use crate::id::ChainId;
use crate::utils::constants;
use prost::{DecodeError, EncodeError, Message};
use proto_lib::sdk::light_request;
use proto_lib::{p2p, sdk};
use thiserror::Error;

#[derive(Debug)]
pub struct OutboundMessage;

// TODO should handle compression if the message is too big
#[derive(Default)]
enum CompressionType {
    #[default]
    None,
}

pub(crate) const DELIMITER_LEN: u32 = 4;

impl OutboundMessage {
    pub fn encode(message: p2p::message::Message) -> Result<Vec<u8>, EncodeError> {
        let message = p2p::Message {
            message: Some(message),
        };
        let encoded_len: usize = message.encoded_len();
        let len: u32 = encoded_len.try_into().unwrap_or(u32::MAX);
        let mut buf = Vec::with_capacity(DELIMITER_LEN as usize + encoded_len);
        buf.append(&mut len.to_be_bytes().to_vec());
        message.encode(&mut buf)?;

        Ok(buf)
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

pub trait InboundMessageExt<OUTER, MESSAGE> {
    fn decode(message: &[u8]) -> Result<MESSAGE, DecodingError>;
}

impl InboundMessageExt<p2p::Message, p2p::message::Message> for InboundMessage {
    fn decode(message: &[u8]) -> Result<p2p::message::Message, DecodingError> {
        let len = message.len();
        if len == 0 {
            return Err(DecodingError::EmptyMessage);
        }

        let decoded = p2p::Message::decode(message).map_err(DecodingError::Prost)?;

        decoded.message.ok_or(DecodingError::EmptyMessage)
    }
}

impl InboundMessageExt<sdk::LightRequest, sdk::light_request::Message> for InboundMessage {
    fn decode(message: &[u8]) -> Result<sdk::light_request::Message, DecodingError> {
        let len = message.len();
        if len == 0 {
            return Err(DecodingError::EmptyMessage);
        }

        let decoded = sdk::LightRequest::decode(message).map_err(DecodingError::Prost)?;

        decoded.message.ok_or(DecodingError::EmptyMessage)
    }
}

impl InboundMessageExt<sdk::LightResponse, sdk::light_response::Message> for InboundMessage {
    fn decode(message: &[u8]) -> Result<sdk::light_response::Message, DecodingError> {
        let len = message.len();
        if len == 0 {
            return Err(DecodingError::EmptyMessage);
        }

        let decoded = sdk::LightResponse::decode(message).map_err(DecodingError::Prost)?;

        decoded.message.ok_or(DecodingError::EmptyMessage)
    }
}

pub struct AppRequestMessage;

impl AppRequestMessage {
    pub fn encode(
        chain_id: ChainId,
        message: light_request::Message,
    ) -> Result<p2p::message::Message, EncodeError> {
        let mut bytes = unsigned_varint::encode::u64_buffer();
        let bytes = unsigned_varint::encode::u64(constants::SNOWFLAKE_HANDLER_ID, &mut bytes);
        let mut app_bytes = bytes.to_vec();
        let message = sdk::LightRequest {
            message: Some(message),
        };
        message.encode(&mut app_bytes)?;
        let app_request = p2p::AppRequest {
            chain_id: chain_id.as_ref().to_vec(),
            request_id: rand::random(),
            deadline: constants::DEFAULT_DEADLINE,
            app_bytes,
        };
        Ok(p2p::message::Message::AppRequest(app_request))
    }
}
