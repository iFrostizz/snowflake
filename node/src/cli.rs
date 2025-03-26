use crate::id::ChainId;
use crate::net::node::NetworkConfig;
use crate::net::{BackoffParams, Intervals};
use crate::utils::constants::{self};
use clap::Parser;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::{fmt::Display, time::Duration};
use tokio::sync::Semaphore;

#[derive(clap::ValueEnum, Clone, Default, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkName {
    #[default]
    Mainnet,
    Fuji,
}

impl Display for NetworkName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkName::Mainnet => write!(f, "mainnet"),
            NetworkName::Fuji => write!(f, "fuji"),
        }
    }
}

/// Avalanche node
#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Args {
    // TODO replace this by an automatic IP resolving mechanism e.g: https://docs.rs/public-ip/latest/public_ip/
    // TODO the port should be 9651 by default but could be changed, maybe as part of another arg.
    /// Socket address of the node
    #[arg(long)]
    pub public_socket: SocketAddr,

    /// Path of the certificate
    #[arg(short, long, default_value = "./staker.crt")]
    pub cert_path: PathBuf,

    /// Path of the private key
    #[arg(short, long, default_value = "./staker.key")]
    pub pem_key_path: PathBuf,

    /// Path of the BLS key
    #[arg(long, default_value = "./bls.key")]
    pub bls_key_path: PathBuf,

    /// Path of the bootstrappers path in the .json format
    #[arg(long, default_value = "./bootstrappers.json")]
    pub bootstrappers_path: PathBuf,

    /// Network to operate on
    #[arg(short, long, default_value = "mainnet")]
    pub network_id: NetworkName,

    /// Maximum amount of simultaneous inbound connections
    #[arg(long, alias = "max-in", default_value_t = Semaphore::MAX_PERMITS)]
    pub max_in_connections: usize,

    /// Maximum amount of simultaneous outbound connections
    #[arg(long, alias = "max-out", default_value_t = Semaphore::MAX_PERMITS)]
    pub max_out_connections: usize,

    /// Cache size of messages that can be subscribed, to record peers latency
    #[arg(long, alias = "max-lat", default_value = "10")]
    pub max_latency_records: usize,

    /// Maximum amount of simultaneous handshakes
    #[arg(long, alias = "max-hs", default_value_t = Semaphore::MAX_PERMITS)]
    pub max_handshakes: usize,

    /// Intervals configuration
    #[arg(long, default_value = "60000")]
    pub intervals_ping_ms: u64,

    /// Intervals configuration
    #[arg(long, default_value = "60000")]
    pub intervals_get_peer_list_ms: u64,

    #[arg(long, default_value = "9000")]
    pub metrics_port: u16,

    #[arg(long, default_value = "false")]
    pub enable_metrics: bool,

    #[arg(long, default_value = "50")]
    pub max_peers: Option<usize>,

    /// IPC socket path
    #[arg(long, default_value = "/tmp/snowflake.ipc")]
    pub ipc_socket_path: String,
}

pub fn read_args() -> Args {
    Args::parse()
}

impl Args {
    fn intervals(&self) -> Intervals {
        Intervals {
            ping: self.intervals_ping_ms,
            get_peer_list: self.intervals_get_peer_list_ms,
        }
    }

    // TODO from args
    fn back_off(&self) -> BackoffParams {
        BackoffParams {
            initial_duration: Duration::from_secs(1),
            muln: 3,
            max_retries: 3,
        }
    }

    pub fn network_config(&self) -> NetworkConfig {
        let intervals = self.intervals();
        let back_off = self.back_off();

        let network = &self.network_id.to_string();
        let network_id: u32 = constants::NETWORK[network];
        let c_chain_id: ChainId = constants::C_CHAIN_ID[network].clone();

        NetworkConfig {
            socket_addr: self.public_socket,
            network_id,
            c_chain_id,
            pem_key_path: self.pem_key_path.clone(),
            bls_key_path: self.bls_key_path.clone(),
            cert_path: self.cert_path.clone(),
            intervals,
            back_off,
            max_throughput: 1_000_000,      // 1000 kB/s
            max_out_queue_size: 10_000_000, // 10 MB
            bucket_size: 500_000,           // 500 kB
            max_concurrent_handshakes: self.max_handshakes,
            max_peers: self.max_peers,
        }
    }
}
