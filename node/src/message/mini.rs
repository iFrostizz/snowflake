use proto_lib::p2p::message::Message;

use crate::stats;

// /// A helper struct that implements [`std::fmt::Display`] for printing without data
// #[derive(Debug)]
// pub struct MiniMessage<'a>(&'a Message);

/// A helper struct that implements [`std::fmt::Display`] for printing without data
#[derive(Debug)]
pub enum MiniMessage {
    CompressedZstd,
    Ping,
    Pong,
    Handshake,
    LightHandshake,
    GetPeerList,
    PeerList,
    GetStateSummaryFrontier,
    StateSummaryFrontier,
    GetAcceptedStateSummary,
    AcceptedStateSummary,
    GetAcceptedFrontier,
    AcceptedFrontier,
    GetAccepted,
    Accepted,
    GetAncestors,
    Ancestors,
    Get,
    Put,
    PushQuery,
    PullQuery,
    Chits,
    AppRequest,
    AppResponse,
    AppGossip,
    AppError,
}

impl std::fmt::Display for MiniMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let variant = match self {
            MiniMessage::CompressedZstd => "CompressedZstd",
            MiniMessage::Ping => "Ping",
            MiniMessage::Pong => "Pong",
            MiniMessage::Handshake => "Handshake",
            MiniMessage::LightHandshake => "LightHandshake",
            MiniMessage::GetPeerList => "GetPeerList",
            MiniMessage::PeerList => "PeerList",
            MiniMessage::GetStateSummaryFrontier => "GetStateSummaryFrontier",
            MiniMessage::StateSummaryFrontier => "StateSummaryFrontier",
            MiniMessage::GetAcceptedStateSummary => "GetAcceptedStateSummary",
            MiniMessage::AcceptedStateSummary => "AcceptedStateSummary",
            MiniMessage::GetAcceptedFrontier => "GetAcceptedFrontier",
            MiniMessage::AcceptedFrontier => "AcceptedFrontier",
            MiniMessage::GetAccepted => "GetAccepted",
            MiniMessage::Accepted => "Accepted",
            MiniMessage::GetAncestors => "GetAncestors",
            MiniMessage::Ancestors => "Ancestors",
            MiniMessage::Get => "Get",
            MiniMessage::Put => "Put",
            MiniMessage::PushQuery => "PushQuery",
            MiniMessage::PullQuery => "PullQuery",
            MiniMessage::Chits => "Chits",
            MiniMessage::AppRequest => "AppRequest",
            MiniMessage::AppResponse => "AppResponse",
            MiniMessage::AppGossip => "AppGossip",
            MiniMessage::AppError => "AppError",
        };
        write!(f, "Message::{}", variant)
    }
}

impl<'a> From<&'a Message> for MiniMessage {
    fn from(value: &'a Message) -> Self {
        match value {
            Message::CompressedZstd(_) => MiniMessage::CompressedZstd,
            Message::Ping(_) => MiniMessage::Ping,
            Message::Pong(_) => MiniMessage::Pong,
            Message::Handshake(_) => MiniMessage::Handshake,
            Message::LightHandshake(_) => MiniMessage::LightHandshake,
            Message::GetPeerList(_) => MiniMessage::GetPeerList,
            Message::PeerList(_) => MiniMessage::PeerList,
            Message::GetStateSummaryFrontier(_) => MiniMessage::GetStateSummaryFrontier,
            Message::StateSummaryFrontier(_) => MiniMessage::StateSummaryFrontier,
            Message::GetAcceptedStateSummary(_) => MiniMessage::GetAcceptedStateSummary,
            Message::AcceptedStateSummary(_) => MiniMessage::AcceptedStateSummary,
            Message::GetAcceptedFrontier(_) => MiniMessage::GetAcceptedFrontier,
            Message::AcceptedFrontier(_) => MiniMessage::AcceptedFrontier,
            Message::GetAccepted(_) => MiniMessage::GetAccepted,
            Message::Accepted(_) => MiniMessage::Accepted,
            Message::GetAncestors(_) => MiniMessage::GetAncestors,
            Message::Ancestors(_) => MiniMessage::Ancestors,
            Message::Get(_) => MiniMessage::Get,
            Message::Put(_) => MiniMessage::Put,
            Message::PushQuery(_) => MiniMessage::PushQuery,
            Message::PullQuery(_) => MiniMessage::PullQuery,
            Message::Chits(_) => MiniMessage::Chits,
            Message::AppRequest(_) => MiniMessage::AppRequest,
            Message::AppResponse(_) => MiniMessage::AppResponse,
            Message::AppGossip(_) => MiniMessage::AppGossip,
            Message::AppError(_) => MiniMessage::AppError,
        }
    }
}

impl MiniMessage {
    #[rustfmt::skip]
    fn change_bytes(&self, size: u64, sent: bool) {
        if sent {
            match self {
                MiniMessage::CompressedZstd => stats::messages::inc_sent_compressed_zstd_bytes(size),
                MiniMessage::Ping => stats::messages::inc_sent_ping_bytes(size),
                MiniMessage::Pong => stats::messages::inc_sent_pong_bytes(size),
                MiniMessage::Handshake => stats::messages::inc_sent_handshake_bytes(size),
                MiniMessage::LightHandshake => stats::messages::inc_sent_light_handshake_bytes(size),
                MiniMessage::GetPeerList => stats::messages::inc_sent_get_peer_list_bytes(size),
                MiniMessage::PeerList => stats::messages::inc_sent_peer_list_bytes(size),
                MiniMessage::GetStateSummaryFrontier => stats::messages::inc_sent_get_state_summary_frontier_bytes(size),
                MiniMessage::StateSummaryFrontier => stats::messages::inc_sent_state_summary_frontier_bytes(size),
                MiniMessage::GetAcceptedStateSummary => stats::messages::inc_sent_get_accepted_state_summary_bytes(size),
                MiniMessage::AcceptedStateSummary => stats::messages::inc_sent_accepted_state_summary_bytes(size),
                MiniMessage::GetAcceptedFrontier => stats::messages::inc_sent_get_accepted_frontier_bytes(size),
                MiniMessage::AcceptedFrontier => stats::messages::inc_sent_accepted_frontier_bytes(size),
                MiniMessage::GetAccepted => stats::messages::inc_sent_get_accepted_bytes(size),
                MiniMessage::Accepted => stats::messages::inc_sent_accepted_bytes(size),
                MiniMessage::GetAncestors => stats::messages::inc_sent_get_ancestors_bytes(size),
                MiniMessage::Ancestors => stats::messages::inc_sent_ancestors_bytes(size),
                MiniMessage::Get => stats::messages::inc_sent_get_bytes(size),
                MiniMessage::Put => stats::messages::inc_sent_put_bytes(size),
                MiniMessage::PushQuery => stats::messages::inc_sent_push_query_bytes(size),
                MiniMessage::PullQuery => stats::messages::inc_sent_pull_query_bytes(size),
                MiniMessage::Chits => stats::messages::inc_sent_chits_bytes(size),
                MiniMessage::AppRequest => stats::messages::inc_sent_app_request_bytes(size),
                MiniMessage::AppResponse => stats::messages::inc_sent_app_response_bytes(size),
                MiniMessage::AppGossip => stats::messages::inc_sent_app_gossip_bytes(size),
                MiniMessage::AppError => stats::messages::inc_sent_app_error_bytes(size),
            }
        } else {
            match self {
                MiniMessage::CompressedZstd => stats::messages::inc_recv_compressed_zstd_bytes(size),
                MiniMessage::Ping => stats::messages::inc_recv_ping_bytes(size),
                MiniMessage::Pong => stats::messages::inc_recv_pong_bytes(size),
                MiniMessage::Handshake => stats::messages::inc_recv_handshake_bytes(size),
                MiniMessage::LightHandshake => stats::messages::inc_recv_light_handshake_bytes(size),
                MiniMessage::GetPeerList => stats::messages::inc_recv_get_peer_list_bytes(size),
                MiniMessage::PeerList => stats::messages::inc_recv_peer_list_bytes(size),
                MiniMessage::GetStateSummaryFrontier => stats::messages::inc_recv_get_state_summary_frontier_bytes(size),
                MiniMessage::StateSummaryFrontier => stats::messages::inc_recv_state_summary_frontier_bytes(size),
                MiniMessage::GetAcceptedStateSummary => stats::messages::inc_recv_get_accepted_state_summary_bytes(size),
                MiniMessage::AcceptedStateSummary => stats::messages::inc_recv_accepted_state_summary_bytes(size),
                MiniMessage::GetAcceptedFrontier => stats::messages::inc_recv_get_accepted_frontier_bytes(size),
                MiniMessage::AcceptedFrontier => stats::messages::inc_recv_accepted_frontier_bytes(size),
                MiniMessage::GetAccepted => stats::messages::inc_recv_get_accepted_bytes(size),
                MiniMessage::Accepted => stats::messages::inc_recv_accepted_bytes(size),
                MiniMessage::GetAncestors => stats::messages::inc_recv_get_ancestors_bytes(size),
                MiniMessage::Ancestors => stats::messages::inc_recv_ancestors_bytes(size),
                MiniMessage::Get => stats::messages::inc_recv_get_bytes(size),
                MiniMessage::Put => stats::messages::inc_recv_put_bytes(size),
                MiniMessage::PushQuery => stats::messages::inc_recv_push_query_bytes(size),
                MiniMessage::PullQuery => stats::messages::inc_recv_pull_query_bytes(size),
                MiniMessage::Chits => stats::messages::inc_recv_chits_bytes(size),
                MiniMessage::AppRequest => stats::messages::inc_recv_app_request_bytes(size),
                MiniMessage::AppResponse => stats::messages::inc_recv_app_response_bytes(size),
                MiniMessage::AppGossip => stats::messages::inc_recv_app_gossip_bytes(size),
                MiniMessage::AppError => stats::messages::inc_recv_app_error_bytes(size),
            }
        }
    }

    pub fn inc_sent(&self, size: u64) {
        self.change_bytes(size, true);
    }

    pub fn inc_recv(&self, size: u64) {
        self.change_bytes(size, false);
    }
}
