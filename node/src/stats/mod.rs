use metrics::{counter, describe_counter, describe_histogram, histogram};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub struct Metrics;

const CONNECTED_PEERS_INC: &str = "connected_peers_inc";
const CONNECTED_PEERS_DEC: &str = "connected_peers_dec";

pub mod connected_peers {
    use super::*;

    pub fn init() {
        describe_counter!(CONNECTED_PEERS_INC, "number of inc connected peers");
        describe_counter!(CONNECTED_PEERS_DEC, "number of dec connected peers");

        counter!(CONNECTED_PEERS_DEC).absolute(0);
        counter!(CONNECTED_PEERS_INC).absolute(0);
    }

    pub fn inc() {
        counter!(CONNECTED_PEERS_INC).increment(1);
    }

    pub fn dec() {
        counter!(CONNECTED_PEERS_DEC).increment(1);
    }
}

const HANDSHOOK_PEERS_INC: &str = "handshook_peers_inc";
const HANDSHOOK_PEERS_DEC: &str = "handshook_peers_dec";

pub mod handshook_peers {
    use super::*;

    pub fn init() {
        describe_counter!(HANDSHOOK_PEERS_DEC, "number of dec handshook peers");
        describe_counter!(HANDSHOOK_PEERS_INC, "number of inc handshook peers");

        counter!(HANDSHOOK_PEERS_DEC).absolute(0);
        counter!(HANDSHOOK_PEERS_INC).absolute(0);
    }

    pub fn inc() {
        counter!(HANDSHOOK_PEERS_INC).increment(1);
    }

    pub fn dec() {
        counter!(HANDSHOOK_PEERS_DEC).increment(1);
    }
}

const SENT_MESSAGES_BYTES: &str = "sent_messages_bytes";
const RECV_MESSAGES_BYTES: &str = "recv_messages_bytes";

const SUCCEEDED_APP_REQUEST: &str = "succeeded_app_request";
const FAILED_APP_REQUEST: &str = "failed_app_request";
const EXPIRED_APP_REQUEST: &str = "expired_app_request";

pub mod messages {
    use super::*;
    use paste::paste;

    pub fn init() {
        describe_counter!(RECV_MESSAGES_BYTES, "amount of bytes of received messages");
        describe_counter!(SENT_MESSAGES_BYTES, "amount of bytes of sent messages");

        describe_counter!(SUCCEEDED_APP_REQUEST, "number of succeeded AppRequest messages");
        describe_counter!(FAILED_APP_REQUEST, "number of failed AppRequest messages");
        describe_counter!(EXPIRED_APP_REQUEST, "number of expired AppRequest messages");
    }

    pub fn inc_recv_messages_bytes(size: u64) {
        counter!(RECV_MESSAGES_BYTES).increment(size);
    }

    pub fn inc_sent_messages_bytes(size: u64) {
        counter!(SENT_MESSAGES_BYTES).increment(size);
    }

    pub fn inc_expired_app_request() {
        counter!(EXPIRED_APP_REQUEST).increment(1);
    }

    macro_rules! gen_message_methods {
        ( $($name:ident),+ ) => {
            $(
                // Generates methods for each identifier in the list
                gen_message_methods!(@single $name);
            )+
        };
        (@single $name:ident) => {
            paste! {
                const [< SENT_ $name:snake:upper _BYTES >]: &str = stringify!([< sent_ $name:snake _bytes >]);
                const [< RECV_ $name:snake:upper _BYTES >]: &str = stringify!([< recv_ $name:snake _bytes >]);

                #[allow(unused)]
                pub fn [< inc_sent_$name:snake _bytes >] (size: u64) {
                    counter!([< SENT_ $name:snake:upper _BYTES >]).increment(size);
                }

                #[allow(unused)]
                pub fn [< inc_recv_$name:snake _bytes >] (size: u64) {
                    counter!([< RECV_ $name:snake:upper _BYTES >]).increment(size);
                }
            }
        };
    }

    gen_message_methods!(
        CompressedZstd,
        Ping,
        Pong,
        Handshake,
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
        AppError
    );
}

const MESSAGE_LATENCY_S: &str = "message_latency_s";

pub mod latency {
    use super::*;

    pub fn init() {
        describe_histogram!(MESSAGE_LATENCY_S, "message latency")
    }

    pub fn record_latency(latency_s: f64) {
        histogram!(MESSAGE_LATENCY_S).record(latency_s);
    }
}

// TODO this won't be sustainable. Find a way to make it compile-time and not annoying to write.
// maybe write a newtype struct that does that ?
const SENT_PING_BYTES: &str = "sent_ping_bytes";
const RECV_PING_BYTES: &str = "recv_ping_bytes";

const SENT_PONG_BYTES: &str = "sent_pong_bytes";
const RECV_PONG_BYTES: &str = "recv_pong_bytes";

pub mod spec_messages {
    use super::*;

    pub fn init() {
        describe_counter!(SENT_PING_BYTES, "");
        describe_counter!(RECV_PING_BYTES, "");

        describe_counter!(SENT_PONG_BYTES, "");
        describe_counter!(RECV_PONG_BYTES, "");
    }

    // pub fn inc_sent_ping_bytes(size: u64) {
    //     counter!(SENT_PING_BYTES).increment(size);
    // }

    // pub fn inc_recv_ping_bytes(size: u64) {
    //     counter!(RECV_PING_BYTES).increment(size);
    // }

    // pub fn inc_sent_pong_bytes(size: u64) {
    //     counter!(SENT_PONG_BYTES).increment(size);
    // }

    // pub fn inc_recv_pong_bytes(size: u64) {
    //     counter!(RECV_PONG_BYTES).increment(size);
    // }
}

impl Metrics {
    pub fn start(port: u16) {
        let builder = PrometheusBuilder::new();
        builder
            .with_http_listener(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port))
            .install()
            .expect("failed to install Prometheus recorder");

        connected_peers::init();
        handshook_peers::init();
        messages::init();
        latency::init();
        spec_messages::init();
    }
}
