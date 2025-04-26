use flume::Sender;
use jsonrpsee::server::{Server, ServerBuilder};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::oneshot;

pub struct Rpc {
    node: Arc<Node>,
    server: Server,
    local_addr: SocketAddr,
    tx: Sender<(Vec<u8>, Instant)>,
}

#[allow(dead_code)]
mod jsonrpc_errors {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
}

macro_rules! not_implemented {
    () => {
        return jsonrpsee::core::RpcResult::Err(jsonrpsee::types::ErrorObject::borrowed(
            METHOD_NOT_FOUND,
            "unimplemented method",
            None,
        ))
    };
}

mod rpc_impl {
    use super::*;
    use crate::node::Node;
    use crate::utils::constants;
    use crate::utils::rlp::{Block, Transaction};
    use crate::Arc;
    use alloy::consensus::{EthereumTxEnvelope, TxEip4844};
    use alloy::primitives::{keccak256, Address, Bytes, FixedBytes, U256, U64};
    use flume::Sender;
    use jsonrpc_errors::*;
    use jsonrpsee::core::{async_trait, RpcResult};
    use jsonrpsee::proc_macros::rpc;
    use jsonrpsee::types::ErrorObject;
    use serde::{Deserialize, Deserializer, Serialize};
    use std::env;
    use std::str::FromStr;
    use std::time::Instant;

    type Bytes32 = FixedBytes<32>;
    type BloomFilter = FixedBytes<256>;

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct UnsignedTransactionObject {
        from: Address,
        to: Option<Address>,
        gas: Option<u64>,
        gas_price: Option<u64>,
        value: Option<U256>,
        input: Vec<u8>,
        nonce: Option<u64>,
    }

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct CallObject {
        from: Option<Address>,
        to: Address,
        gas: Option<u64>,
        value: Option<U256>,
        input: Vec<u8>,
    }

    #[derive(Debug, Default, Serialize, Clone)]
    #[serde(rename_all = "lowercase")]
    pub enum BlockParameter {
        Number(u64),
        #[default]
        Latest,
        Earliest,
        Pending,
        Safe,
        Finalized,
    }

    impl<'de> Deserialize<'de> for BlockParameter {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let variant = String::deserialize(deserializer)?;
            if let Some(stripped) = variant.strip_prefix("0x") {
                let mut stripped = stripped.to_owned();
                if stripped.len() % 2 != 0 {
                    stripped = "0".to_owned() + &*stripped;
                }
                let bytes = hex::decode(stripped).map_err(serde::de::Error::custom)?;
                if bytes.len() > 8 {
                    return Err(serde::de::Error::custom("too many bytes"));
                }
                let mut arr = [0; 8];
                arr[8 - bytes.len()..].copy_from_slice(&bytes);
                Ok(BlockParameter::Number(u64::from_be_bytes(arr)))
            } else {
                match variant.as_str() {
                    "latest" => Ok(BlockParameter::Latest),
                    "earliest" => Ok(BlockParameter::Earliest),
                    "pending" => Ok(BlockParameter::Pending),
                    "safe" => Ok(BlockParameter::Safe),
                    "finalized" => Ok(BlockParameter::Finalized),
                    _ => Err(serde::de::Error::custom("unknown block parameter")),
                }
            }
        }
    }

    fn block_to_rpc(block: Block, full: bool) -> alloy::rpc::types::Block {
        let header = alloy::rpc::types::Header {
            hash: block.hash,
            inner: block.header.into(),
            total_difficulty: None,
            size: Some(U256::try_from(block.size).unwrap()),
        };

        let transactions = if full {
            alloy::rpc::types::BlockTransactions::Full(
                block
                    .transactions
                    .into_iter()
                    .map(alloy::rpc::types::Transaction::from)
                    .collect(),
            )
        } else {
            alloy::rpc::types::BlockTransactions::Hashes(
                block
                    .transactions
                    .into_iter()
                    .map(|transaction| match transaction {
                        Transaction::Legacy { hash, .. } | Transaction::EIP2718 { hash, .. } => {
                            hash
                        }
                    })
                    .collect(),
            )
        };

        alloy::rpc::types::Block::new(header, transactions)
    }

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct LogObject {}

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct TransactionReceiptObject {
        transaction_hash: Bytes32,
        transaction_index: u64,
        block_hash: Bytes32,
        block_number: u64,
        from: Address,
        to: Address,
        cumulative_gas_used: u64,
        effective_gas_price: u64,
        gas_used: u64,
        contract_address: Option<Address>,
        logs: Vec<LogObject>,
        logs_bloom: BloomFilter,
        _type: u8,
    }

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct FilterOptions {
        from_block: Option<BlockParameter>,
        to_block: Option<BlockParameter>,
        address: Option<Address>,
        topics: Vec<Bytes32>,
    }

    type FilterId = Bytes32;

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct FilterObject {}

    #[rpc(server, namespace = "web3")]
    pub trait Web3 {
        #[method(name = "clientVersion")]
        fn client_version(&self) -> RpcResult<String>;

        #[method(name = "sha3")]
        fn sha3(&self, data: Bytes) -> RpcResult<Bytes32> {
            Ok(keccak256(data))
        }
    }

    #[rpc(server, namespace = "net")]
    pub trait Net {
        #[method(name = "version")]
        fn version(&self) -> RpcResult<String>;

        #[method(name = "listening")]
        fn listening(&self) -> RpcResult<bool> {
            Ok(true) // constant for now
        }

        #[method(name = "peerCount")]
        fn peer_count(&self) -> RpcResult<U64>;
    }

    #[rpc(server, namespace = "eth")]
    pub trait Eth {
        #[method(name = "protocolVersion")]
        fn protocol_version(&self) -> RpcResult<String>;

        #[method(name = "syncing")]
        fn syncing(&self) -> RpcResult<bool> {
            // TODO: remove the constant once we ask peers for information to initialize state.
            RpcResult::Ok(false)
        }

        #[method(name = "coinbase")]
        fn coinbase(&self) -> RpcResult<Address> {
            RpcResult::Ok(Address::from_str("0x0100000000000000000000000000000000000000").unwrap())
        }

        #[method(name = "chainId")]
        fn chain_id(&self) -> RpcResult<U64>;

        #[method(name = "mining")]
        fn mining(&self) -> RpcResult<bool> {
            RpcResult::Ok(false)
        }

        #[method(name = "hashrate")]
        fn hashrate(&self) -> RpcResult<u64>;

        #[method(name = "gasPrice")]
        fn gas_price(&self) -> RpcResult<u64>;

        #[method(name = "accounts")]
        fn accounts(&self) -> RpcResult<Vec<Address>>;

        #[method(name = "blockNumber")]
        fn block_number(&self) -> RpcResult<u64>;

        #[method(name = "getBalance")]
        fn get_balance(&self, block_parameter: BlockParameter) -> RpcResult<U256>;

        #[method(name = "getStorageAt")]
        fn get_storage_at(&self, block_parameter: BlockParameter) -> RpcResult<Bytes32>;

        #[method(name = "getTransactionCount")]
        fn get_transaction_count(&self, block_parameter: BlockParameter) -> RpcResult<u64>;

        #[method(name = "getBlockTransactionCountByHash")]
        fn get_block_transaction_count_by_hash(&self, hash: Bytes32) -> RpcResult<u64>;

        #[method(name = "getBlockTransactionCountByNumber")]
        async fn get_block_transaction_count_by_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<u64>;

        #[method(name = "getUncleCountByBlockHash")]
        fn get_uncle_count_by_block_hash(&self, block_parameter: BlockParameter) -> RpcResult<u64>;

        #[method(name = "getUncleCountByBlockNumber")]
        fn get_uncle_count_by_block_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<u64>;

        #[method(name = "getCode")]
        fn get_code(&self, address: Address, block_parameter: BlockParameter)
            -> RpcResult<Vec<u8>>;

        #[method(name = "sign")]
        fn sign(&self, address: Address, message: Vec<u8>) -> RpcResult<Bytes32>;

        #[method(name = "signTransaction")]
        fn sign_transaction(&self, object: UnsignedTransactionObject) -> RpcResult<Vec<u8>>;

        #[method(name = "sendTransaction")]
        fn send_transaction(&self, object: UnsignedTransactionObject) -> RpcResult<Bytes32>;

        #[method(name = "sendRawTransaction")]
        fn send_raw_transaction(&self, data: String) -> RpcResult<Bytes32>;

        #[method(name = "call")]
        fn call(&self, object: CallObject, block_parameter: BlockParameter) -> RpcResult<Vec<u8>>;

        #[method(name = "estimateGas")]
        fn estimate_gas(
            &self,
            object: CallObject,
            block_parameter: BlockParameter,
        ) -> RpcResult<Vec<u8>>;

        #[method(name = "getBlockByHash")]
        fn get_block_by_hash(
            &self,
            hash: Bytes32,
            full: bool,
        ) -> RpcResult<alloy::rpc::types::Block>;

        #[method(name = "getBlockByNumber")]
        async fn get_block_by_number(
            &self,
            block_parameter: BlockParameter,
            full: bool,
        ) -> RpcResult<alloy::rpc::types::Block>;

        #[method(name = "getTransaction_by_hash")]
        fn get_transaction_by_hash(
            &self,
            hash: Bytes32,
        ) -> RpcResult<alloy::rpc::types::Transaction<EthereumTxEnvelope<TxEip4844>>>;

        #[method(name = "getTransactionByBlockHashAndIndex")]
        fn get_transaction_by_block_hash_and_index(
            &self,
            hash: Bytes32,
            position: u64,
        ) -> RpcResult<alloy::rpc::types::Transaction<EthereumTxEnvelope<TxEip4844>>>;

        #[method(name = "getTransactionByBlockNumberAndIndex")]
        fn get_transaction_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            position: u64,
        ) -> RpcResult<alloy::rpc::types::Transaction<EthereumTxEnvelope<TxEip4844>>>;

        #[method(name = "getTransactionReceipt")]
        fn get_transaction_receipt(&self, hash: Bytes32) -> RpcResult<TransactionReceiptObject>;

        #[method(name = "getUncleByBlockHashAndIndex")]
        fn get_uncle_by_block_hash_and_index(
            &self,
            hash: Bytes32,
            index: u64,
        ) -> RpcResult<alloy::rpc::types::Block>;

        #[method(name = "getUncleByBlockNumberAndIndex")]
        fn get_uncle_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            index: u64,
        ) -> RpcResult<alloy::rpc::types::Block>;

        #[method(name = "newFilter")]
        fn new_filter(&self, block_parameter: BlockParameter) -> RpcResult<FilterObject>;

        #[method(name = "newBlockFilter")]
        fn new_block_filter(&self) -> RpcResult<FilterId>;

        #[method(name = "newPendingTransactionFilter")]
        fn new_pending_transaction_filter(&self) -> RpcResult<FilterId>;

        #[method(name = "uninstallFilter")]
        fn uninstall_filter(&self, filter_id: FilterId) -> RpcResult<bool>;

        #[method(name = "getFilterChanges")]
        fn get_filter_changes(&self, filter_id: FilterId) -> RpcResult<Vec<LogObject>>;

        #[method(name = "getFilterLogs")]
        fn get_filter_logs(&self, filter_id: FilterId) -> RpcResult<Vec<LogObject>>;

        #[method(name = "getLogs")]
        fn get_logs(&self, filter_object: FilterObject) -> RpcResult<Vec<LogObject>>;
    }

    #[derive(Clone)]
    pub struct RpcServerImpl {
        pub(crate) node: Arc<Node>,
        pub(crate) tx: Sender<(Vec<u8>, Instant)>,
    }

    impl Web3Server for RpcServerImpl {
        fn client_version(&self) -> RpcResult<String> {
            const CLIENT: &str = constants::CLIENT;
            const VERSION: &str = env!("CARGO_PKG_VERSION");
            const PLATFORM: &str = current_platform::CURRENT_PLATFORM;
            const RUSTC: Option<&str> = option_env!("RUSTC_VERSION");
            let rustc = RUSTC.unwrap_or("x.x.x");
            let client_version = format!("{}/{}/{}/{}", CLIENT, VERSION, PLATFORM, rustc);
            Ok(client_version)
        }
    }

    impl NetServer for RpcServerImpl {
        fn version(&self) -> RpcResult<String> {
            Ok(self.node.network.config.eth_network_id.to_string())
        }

        fn peer_count(&self) -> RpcResult<U64> {
            let peer_count = self.node.network.peers_infos.read().unwrap().len() as u64;
            Ok(U64::from(peer_count))
        }
    }

    #[async_trait]
    impl EthServer for RpcServerImpl {
        fn protocol_version(&self) -> RpcResult<String> {
            not_implemented!()
        }

        fn chain_id(&self) -> RpcResult<U64> {
            Ok(U64::from(self.node.network.config.eth_network_id))
        }

        fn hashrate(&self) -> RpcResult<u64> {
            not_implemented!()
        }

        fn gas_price(&self) -> RpcResult<u64> {
            not_implemented!()
        }

        fn accounts(&self) -> RpcResult<Vec<Address>> {
            not_implemented!()
        }

        fn block_number(&self) -> RpcResult<u64> {
            not_implemented!()
        }

        fn get_balance(&self, _block_parameter: BlockParameter) -> RpcResult<U256> {
            not_implemented!()
        }

        fn get_storage_at(&self, _block_parameter: BlockParameter) -> RpcResult<Bytes32> {
            not_implemented!()
        }

        fn get_transaction_count(&self, _block_parameter: BlockParameter) -> RpcResult<u64> {
            not_implemented!()
        }

        fn get_block_transaction_count_by_hash(&self, _hash: Bytes32) -> RpcResult<u64> {
            not_implemented!()
        }

        async fn get_block_transaction_count_by_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<u64> {
            let number = match block_parameter {
                BlockParameter::Number(number) => number,
                _ => not_implemented!(), // resolve block
            };
            let block = self.node.light_network.find_block(number).await?;
            Ok(block.transactions.len() as u64)
        }

        fn get_uncle_count_by_block_hash(
            &self,
            _block_parameter: BlockParameter,
        ) -> RpcResult<u64> {
            not_implemented!()
        }

        fn get_uncle_count_by_block_number(
            &self,
            _block_parameter: BlockParameter,
        ) -> RpcResult<u64> {
            not_implemented!()
        }

        fn get_code(
            &self,
            _address: Address,
            _block_parameter: BlockParameter,
        ) -> RpcResult<Vec<u8>> {
            not_implemented!()
        }

        fn sign(&self, _address: Address, _message: Vec<u8>) -> RpcResult<Bytes32> {
            not_implemented!()
        }

        fn sign_transaction(&self, _object: UnsignedTransactionObject) -> RpcResult<Vec<u8>> {
            not_implemented!()
        }

        fn send_transaction(&self, _object: UnsignedTransactionObject) -> RpcResult<Bytes32> {
            not_implemented!()
        }

        fn send_raw_transaction(&self, data: String) -> RpcResult<Bytes32> {
            let data_hex = data
                .strip_prefix("0x")
                .and_then(|stripped| hex::decode(stripped).ok())
                .ok_or(ErrorObject::borrowed(
                    PARSE_ERROR,
                    "invalid hex string",
                    None,
                ))?;
            let hash = keccak256(&data_hex);
            self.tx.send((data_hex, Instant::now())).unwrap();
            Ok(hash)
        }

        fn call(
            &self,
            _object: CallObject,
            _block_parameter: BlockParameter,
        ) -> RpcResult<Vec<u8>> {
            not_implemented!()
        }

        fn estimate_gas(
            &self,
            _object: CallObject,
            _block_parameter: BlockParameter,
        ) -> RpcResult<Vec<u8>> {
            not_implemented!()
        }

        fn get_block_by_hash(
            &self,
            _hash: Bytes32,
            _full: bool,
        ) -> RpcResult<alloy::rpc::types::Block> {
            not_implemented!()
        }

        async fn get_block_by_number(
            &self,
            block_parameter: BlockParameter,
            full: bool,
        ) -> RpcResult<alloy::rpc::types::Block> {
            let number = match block_parameter {
                BlockParameter::Number(number) => number,
                _ => not_implemented!(), // resolve block
            };
            self.node
                .light_network
                .find_block(number)
                .await
                .map(|block| block_to_rpc(block, full))
                .map_err(Into::into)
        }

        fn get_transaction_by_hash(
            &self,
            _hash: Bytes32,
        ) -> RpcResult<alloy::rpc::types::Transaction<EthereumTxEnvelope<TxEip4844>>> {
            not_implemented!()
        }

        fn get_transaction_by_block_hash_and_index(
            &self,
            _hash: Bytes32,
            _position: u64,
        ) -> RpcResult<alloy::rpc::types::Transaction<EthereumTxEnvelope<TxEip4844>>> {
            not_implemented!()
        }

        fn get_transaction_by_block_number_and_index(
            &self,
            _block_parameter: BlockParameter,
            _position: u64,
        ) -> RpcResult<alloy::rpc::types::Transaction<EthereumTxEnvelope<TxEip4844>>> {
            not_implemented!()
        }

        fn get_transaction_receipt(&self, _hash: Bytes32) -> RpcResult<TransactionReceiptObject> {
            not_implemented!()
        }

        fn get_uncle_by_block_hash_and_index(
            &self,
            _hash: Bytes32,
            _index: u64,
        ) -> RpcResult<alloy::rpc::types::Block> {
            not_implemented!()
        }

        fn get_uncle_by_block_number_and_index(
            &self,
            _block_parameter: BlockParameter,
            _index: u64,
        ) -> RpcResult<alloy::rpc::types::Block> {
            not_implemented!()
        }

        fn new_filter(&self, _block_parameter: BlockParameter) -> RpcResult<FilterObject> {
            not_implemented!()
        }

        fn new_block_filter(&self) -> RpcResult<FilterId> {
            not_implemented!()
        }

        fn new_pending_transaction_filter(&self) -> RpcResult<FilterId> {
            not_implemented!()
        }

        fn uninstall_filter(&self, _filter_id: FilterId) -> RpcResult<bool> {
            not_implemented!()
        }

        fn get_filter_changes(&self, _filter_id: FilterId) -> RpcResult<Vec<LogObject>> {
            not_implemented!()
        }

        fn get_filter_logs(&self, _filter_id: FilterId) -> RpcResult<Vec<LogObject>> {
            not_implemented!()
        }

        fn get_logs(&self, _filter_object: FilterObject) -> RpcResult<Vec<LogObject>> {
            not_implemented!()
        }
    }
}

use crate::net::node::NodeError;
use crate::node::Node;
use rpc_impl::{EthServer, NetServer, RpcServerImpl, Web3Server};

impl Rpc {
    pub async fn new(
        node: Arc<Node>,
        rpc_port: u16,
        tx: Sender<(Vec<u8>, Instant)>,
    ) -> Result<Self, NodeError> {
        let server = ServerBuilder::default()
            .build(format!("127.0.0.1:{}", rpc_port))
            .await?;
        let local_addr = server.local_addr()?;

        Ok(Self {
            node,
            server,
            local_addr,
            tx,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn start(self, mut shutdown_rx: oneshot::Receiver<()>) {
        log::debug!("Listening on {}", self.local_addr());
        let rpc_impl = RpcServerImpl {
            node: self.node.clone(),
            tx: self.tx,
        };
        let mut rpc = Web3Server::into_rpc(rpc_impl.clone());
        rpc.merge(NetServer::into_rpc(rpc_impl.clone()))
            .expect("should not fail");
        rpc.merge(EthServer::into_rpc(rpc_impl))
            .expect("should not fail");

        let server_handle = self.server.start(rpc);
        tokio::select! {
            _ = server_handle.stopped() => {
                log::error!("server stopped!");
            }
            _ = &mut shutdown_rx => {
                // server_handle is dropped so it will be stopped.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Rpc;
    use crate::dht::DhtBuckets;
    use crate::id::ChainId;
    use crate::net::node::NetworkConfig;
    use crate::net::BackoffParams;
    use crate::net::Intervals;
    use crate::net::Network;
    use crate::node::Node;
    use alloy::providers::{network::EthereumWallet, Provider, ProviderBuilder};
    use alloy::signers::local::PrivateKeySigner;
    use std::collections::HashMap;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::path::Path;
    use std::sync::Arc;
    use tokio::sync::oneshot;

    #[tokio::test(flavor = "multi_thread")]
    async fn send_transaction() {
        // tracing_subscriber::fmt::init();

        let credentials_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/credentials/");
        let pem_key_path = credentials_path.join("node.key");
        let cert_path = credentials_path.join("node.crt");
        let bls_key_path = credentials_path.join("bls.key");

        let (tx, rx) = flume::unbounded();
        tokio::spawn(async move {
            rx.recv().unwrap();
        });
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        log::debug!("start");
        let network = Network::new(NetworkConfig {
            socket_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            network_id: 0,
            eth_network_id: 0,
            c_chain_id: ChainId::from([0; 32]),
            pem_key_path,
            bls_key_path,
            cert_path,
            intervals: Intervals {
                ping: 0,
                get_peer_list: 0,
            },
            back_off: BackoffParams {
                initial_duration: Default::default(),
                muln: 0,
                max_retries: 0,
            },
            max_throughput: 0,
            max_out_queue_size: 0,
            bucket_size: 0,
            max_concurrent_handshakes: 0,
            max_peers: None,
            bootstrappers: HashMap::new(),
            dht_buckets: DhtBuckets {
                block: Default::default(),
            },
        })
        .unwrap();
        let node = Node::new(network, 1, 1, false);

        let rpc = Rpc::new(Arc::from(node), 0, tx).await.unwrap();
        let addr = rpc.local_addr();

        tokio::spawn(async move {
            rpc.start(shutdown_rx).await;
        });

        let signer = PrivateKeySigner::random();
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .on_http(format!("http://{}", addr).parse().unwrap());

        log::debug!("sending");
        let tx_signed = hex::decode("f86680843b9aca00825208940000000000000000000000000000000000000000808083015285a06c1cbdd2e8d1a0a9119159527cbe00151778bcc5ea8ae8ccd687b05f5316c325a044ea618c2ca67374cbec06747b048e7915a488f4cf9f911887bfa9e766112846").unwrap();
        let _ = provider.send_raw_transaction(&tx_signed).await.unwrap();

        shutdown_tx.send(()).unwrap();
    }
}
