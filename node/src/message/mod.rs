use proto_lib::p2p::{
    message::Message, Accepted, AcceptedFrontier, AcceptedStateSummary, Ancestors, AppError,
    AppRequest, AppResponse, Chits, Get, GetAccepted, GetAcceptedFrontier, GetAcceptedStateSummary,
    GetAncestors, GetStateSummaryFrontier, Ping, PullQuery, PushQuery, Put, StateSummaryFrontier,
};

use crate::stats;

pub mod mail_box;
pub mod pipeline;

mod mini;
use crate::utils::constants;
pub use mini::MiniMessage;

/// Some [`Message`] can be subscribed.
/// Those are the p2p messages which have a `request_id` field.
#[derive(Debug, Clone)]
pub enum SubscribableMessage {
    Get(Get),
    GetAccepted(GetAccepted),
    GetAcceptedFrontier(GetAcceptedFrontier),
    GetAcceptedStateSummary(GetAcceptedStateSummary),
    GetAncestors(GetAncestors),
    GetStateSummaryFrontier(GetStateSummaryFrontier),
    PushQuery(PushQuery),
    PullQuery(PullQuery),
    AppRequest(AppRequest),
    Ping(Ping),
}

impl SubscribableMessage {
    pub fn response_request_id(message: &Message) -> Option<&u32> {
        match message {
            Message::Put(Put { request_id, .. })
            | Message::Accepted(Accepted { request_id, .. })
            | Message::AcceptedFrontier(AcceptedFrontier { request_id, .. })
            | Message::AcceptedStateSummary(AcceptedStateSummary { request_id, .. })
            | Message::Ancestors(Ancestors { request_id, .. })
            | Message::StateSummaryFrontier(StateSummaryFrontier { request_id, .. })
            | Message::Chits(Chits { request_id, .. })
            | Message::AppResponse(AppResponse { request_id, .. })
            | Message::AppError(AppError { request_id, .. }) => Some(request_id),
            Message::Pong(_) => Some(&0),
            _ => None,
        }
    }

    pub fn request_id(&self) -> &u32 {
        match self {
            SubscribableMessage::Get(Get { request_id, .. })
            | SubscribableMessage::GetAccepted(GetAccepted { request_id, .. })
            | SubscribableMessage::GetAcceptedFrontier(GetAcceptedFrontier {
                request_id, ..
            })
            | SubscribableMessage::GetAcceptedStateSummary(GetAcceptedStateSummary {
                request_id,
                ..
            })
            | SubscribableMessage::GetAncestors(GetAncestors { request_id, .. })
            | SubscribableMessage::GetStateSummaryFrontier(GetStateSummaryFrontier {
                request_id,
                ..
            })
            | SubscribableMessage::PushQuery(PushQuery { request_id, .. })
            | SubscribableMessage::PullQuery(PullQuery { request_id, .. })
            | SubscribableMessage::AppRequest(AppRequest { request_id, .. }) => request_id,
            SubscribableMessage::Ping(_) => &0,
        }
    }

    /// The deadline of the message in nanoseconds
    pub fn deadline(&self) -> u64 {
        match self {
            SubscribableMessage::Get(Get { deadline, .. })
            | SubscribableMessage::GetAccepted(GetAccepted { deadline, .. })
            | SubscribableMessage::GetAcceptedFrontier(GetAcceptedFrontier { deadline, .. })
            | SubscribableMessage::GetAcceptedStateSummary(GetAcceptedStateSummary {
                deadline,
                ..
            })
            | SubscribableMessage::GetAncestors(GetAncestors { deadline, .. })
            | SubscribableMessage::GetStateSummaryFrontier(GetStateSummaryFrontier {
                deadline,
                ..
            })
            | SubscribableMessage::PushQuery(PushQuery { deadline, .. })
            | SubscribableMessage::PullQuery(PullQuery { deadline, .. })
            | SubscribableMessage::AppRequest(AppRequest { deadline, .. }) => *deadline,
            SubscribableMessage::Ping(_) => constants::DEFAULT_DEADLINE,
        }
    }

    pub fn on_timeout(&self) {
        #[allow(clippy::single_match)]
        match self {
            SubscribableMessage::AppRequest(_) => stats::messages::inc_expired_app_request(),
            _ => (),
        }
    }
}

impl From<SubscribableMessage> for Message {
    fn from(value: SubscribableMessage) -> Self {
        match value {
            SubscribableMessage::Get(get) => Message::Get(get),
            SubscribableMessage::GetAccepted(get_accepted) => Message::GetAccepted(get_accepted),
            SubscribableMessage::GetAcceptedFrontier(get_accepted_frontier) => {
                Message::GetAcceptedFrontier(get_accepted_frontier)
            }
            SubscribableMessage::GetAcceptedStateSummary(get_accepted_state_summary) => {
                Message::GetAcceptedStateSummary(get_accepted_state_summary)
            }
            SubscribableMessage::GetAncestors(get_ancestors) => {
                Message::GetAncestors(get_ancestors)
            }
            SubscribableMessage::GetStateSummaryFrontier(get_state_summary_frontier) => {
                Message::GetStateSummaryFrontier(get_state_summary_frontier)
            }
            SubscribableMessage::PushQuery(push_query) => Message::PushQuery(push_query),
            SubscribableMessage::PullQuery(pull_query) => Message::PullQuery(pull_query),
            SubscribableMessage::AppRequest(app_request) => Message::AppRequest(app_request),
            SubscribableMessage::Ping(ping) => Message::Ping(ping),
        }
    }
}
