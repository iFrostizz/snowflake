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
            jsonrpc_errors::METHOD_NOT_FOUND,
            "unimplemented method",
            None,
        ))
    };
}

mod rpc_impl {
    use super::*;
    use crate::dht::block::DhtBlocks;
    use crate::dht::kademlia::LockedMapDb;
    use crate::dht::light_errors;
    use crate::dht::Bucket;
    use crate::dht::DhtId;
    use crate::id::NodeId;
    use crate::message::SubscribableMessage;
    use crate::net::light::DhtCodex;
    use crate::net::queue::ConnectionData;
    use crate::node::Node;
    use crate::server::msg::AppRequestMessage;
    use crate::server::msg::InboundMessage;
    use crate::server::msg::InboundMessageExt;
    use crate::utils::constants;
    use crate::utils::rlp::{Block, Header, Transaction};
    use crate::utils::twokhashmap::CompositeKey;
    use crate::utils::unpacker::StatelessBlock;
    use crate::Arc;
    use alloy::primitives::{keccak256, Address, Bytes, FixedBytes, U256, U64};
    use flume::Sender;
    use jsonrpsee::core::{async_trait, RpcResult};
    use jsonrpsee::proc_macros::rpc;
    use jsonrpsee::types::ErrorObject;
    use proto_lib::{p2p, sdk};
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

    fn header_to_rpc(
        header: Header,
        hash: alloy::primitives::B256,
        size: Option<U256>,
    ) -> alloy::rpc::types::Header {
        alloy::rpc::types::Header {
            hash,
            inner: header.into(),
            total_difficulty: None,
            size,
        }
    }

    fn block_to_rpc(block: Block, full: bool) -> alloy::rpc::types::Block {
        let hash = block.hash();
        let header = header_to_rpc(
            block.header,
            hash,
            Some(U256::try_from(block.size).unwrap()),
        );

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
        async fn get_block_transaction_count_by_hash(&self, hash: Bytes32) -> RpcResult<u64>;

        #[method(name = "getBlockTransactionCountByNumber")]
        async fn get_block_transaction_count_by_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<u64>;

        #[method(name = "getUncleCountByBlockHash")]
        async fn get_uncle_count_by_block_hash(&self, hash: Bytes32) -> RpcResult<u64>;

        #[method(name = "getUncleCountByBlockNumber")]
        async fn get_uncle_count_by_block_number(
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
        async fn get_block_by_hash(
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
        ) -> RpcResult<alloy::rpc::types::Transaction>;

        #[method(name = "getTransactionByBlockHashAndIndex")]
        async fn get_transaction_by_block_hash_and_index(
            &self,
            hash: Bytes32,
            position: u64,
        ) -> RpcResult<alloy::rpc::types::Transaction>;

        #[method(name = "getTransactionByBlockNumberAndIndex")]
        async fn get_transaction_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            position: u64,
        ) -> RpcResult<alloy::rpc::types::Transaction>;

        #[method(name = "getTransactionReceipt")]
        fn get_transaction_receipt(&self, hash: Bytes32) -> RpcResult<TransactionReceiptObject>;

        #[method(name = "getUncleByBlockHashAndIndex")]
        async fn get_uncle_by_block_hash_and_index(
            &self,
            hash: Bytes32,
            index: u64,
        ) -> RpcResult<alloy::rpc::types::Header>;

        #[method(name = "getUncleByBlockNumberAndIndex")]
        async fn get_uncle_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            index: u64,
        ) -> RpcResult<alloy::rpc::types::Header>;

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

    #[derive(Serialize, Clone)]
    pub enum Value {
        Block(alloy::rpc::types::Block),
    }

    #[derive(Serialize, Clone)]
    pub enum RpcValueOrNodes {
        Value(Value),
        Nodes(Vec<NodeId>),
    }

    #[rpc(server, namespace = "light")]
    pub trait Light {
        #[method(name = "ping")]
        async fn ping(&self, node_id: NodeId) -> RpcResult<()>;

        #[method(name = "store")]
        async fn store(
            &self,
            node_id: Option<NodeId>,
            dht_id: DhtId,
            value: Bytes,
        ) -> RpcResult<()>;

        #[method(name = "find_node")]
        async fn find_node(
            &self,
            node_id: Option<NodeId>,
            bucket: Bucket,
        ) -> RpcResult<Vec<NodeId>>;

        #[method(name = "find_value")]
        async fn find_value(
            &self,
            node_id: Option<NodeId>,
            dht_id: DhtId,
            bucket: Bucket,
        ) -> RpcResult<RpcValueOrNodes>;
    }

    #[derive(Clone)]
    pub struct RpcServerImpl {
        pub(crate) node: Arc<Node>,
        pub(crate) tx: Sender<(Vec<u8>, Instant)>,
    }

    impl RpcServerImpl {
        async fn get_block_by_parameter(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<StatelessBlock> {
            let number = match block_parameter {
                BlockParameter::Number(number) => number,
                BlockParameter::Earliest => 0,
                BlockParameter::Latest
                | BlockParameter::Finalized
                | BlockParameter::Pending
                | BlockParameter::Safe => self.node.network.latest_block().await.map_err(|_| {
                    ErrorObject::borrowed(jsonrpc_errors::INTERNAL_ERROR, "block not found", None)
                })?,
            };
            Ok(self
                .node
                .network
                .light_network
                .find_content(
                    &self.node.network.light_network.block_dht,
                    CompositeKey::First(number),
                )
                .await?)
        }

        async fn get_block_by_hash(&self, hash: Bytes32) -> RpcResult<StatelessBlock> {
            Ok(self
                .node
                .network
                .light_network
                .find_content(
                    &self.node.network.light_network.block_dht,
                    CompositeKey::Second(hash),
                )
                .await?)
        }

        fn transaction_at_position(
            block: alloy::rpc::types::Block,
            position: u64,
        ) -> RpcResult<alloy::rpc::types::Transaction> {
            block
                .transactions
                .as_transactions()
                .ok_or(ErrorObject::borrowed(
                    jsonrpc_errors::INTERNAL_ERROR,
                    "block has no transaction",
                    None,
                ))?
                .get(position as usize)
                .ok_or(ErrorObject::borrowed(
                    jsonrpc_errors::INTERNAL_ERROR,
                    "missing transaction at position",
                    None,
                ))
                .cloned()
        }
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

        async fn get_block_transaction_count_by_hash(&self, hash: Bytes32) -> RpcResult<u64> {
            let block = self.get_block_by_hash(hash).await?;
            let block = block.block;
            Ok(block.transactions.len() as u64)
        }

        async fn get_block_transaction_count_by_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<u64> {
            let block = self.get_block_by_parameter(block_parameter).await?;
            let block = block.block;
            Ok(block.transactions.len() as u64)
        }

        async fn get_uncle_count_by_block_hash(&self, hash: Bytes32) -> RpcResult<u64> {
            let block = self.get_block_by_hash(hash).await?;
            let block = block.block;
            Ok(block.uncles.len() as u64)
        }

        async fn get_uncle_count_by_block_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<u64> {
            let block = self.get_block_by_parameter(block_parameter).await?;
            let block = block.block;
            Ok(block.uncles.len() as u64)
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
                    jsonrpc_errors::PARSE_ERROR,
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

        async fn get_block_by_hash(
            &self,
            hash: Bytes32,
            full: bool,
        ) -> RpcResult<alloy::rpc::types::Block> {
            self.get_block_by_hash(hash)
                .await
                .map(|block| block_to_rpc(block.block, full))
        }

        async fn get_block_by_number(
            &self,
            block_parameter: BlockParameter,
            full: bool,
        ) -> RpcResult<alloy::rpc::types::Block> {
            self.get_block_by_parameter(block_parameter)
                .await
                .map(|block| block_to_rpc(block.block, full))
        }

        fn get_transaction_by_hash(
            &self,
            _hash: Bytes32,
        ) -> RpcResult<alloy::rpc::types::Transaction> {
            not_implemented!()
        }

        async fn get_transaction_by_block_hash_and_index(
            &self,
            hash: Bytes32,
            position: u64,
        ) -> RpcResult<alloy::rpc::types::Transaction> {
            let block: alloy::rpc::types::Block = self
                .get_block_by_hash(hash)
                .await
                .map(|block| block_to_rpc(block.block, true))?;
            Self::transaction_at_position(block, position)
        }

        async fn get_transaction_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            position: u64,
        ) -> RpcResult<alloy::rpc::types::Transaction> {
            let block = self
                .get_block_by_parameter(block_parameter)
                .await
                .map(|block| block_to_rpc(block.block, true))?;
            Self::transaction_at_position(block, position)
        }

        fn get_transaction_receipt(&self, _hash: Bytes32) -> RpcResult<TransactionReceiptObject> {
            not_implemented!()
        }

        async fn get_uncle_by_block_hash_and_index(
            &self,
            hash: Bytes32,
            index: u64,
        ) -> RpcResult<alloy::rpc::types::Header> {
            let block = self.get_block_by_hash(hash).await?;
            let mut block = block.block;
            if block.uncles.get(index as usize).is_none() {
                return Err(ErrorObject::borrowed(1000, "missing uncle at index", None));
            }
            let hash = block.hash();
            let uncle = block.uncles.swap_remove(index as usize);
            Ok(header_to_rpc(
                uncle,
                hash,
                Some(U256::try_from(block.size).unwrap()),
            ))
        }

        async fn get_uncle_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            index: u64,
        ) -> RpcResult<alloy::rpc::types::Header> {
            let block = self.get_block_by_parameter(block_parameter).await?;
            let mut block = block.block;
            let hash = block.hash();
            if block.uncles.get(index as usize).is_none() {
                return Err(ErrorObject::borrowed(1000, "missing uncle at index", None));
            }
            let uncle = block.uncles.swap_remove(index as usize);
            Ok(header_to_rpc(
                uncle,
                hash,
                Some(U256::try_from(block.size).unwrap()),
            ))
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

    #[async_trait]
    impl LightServer for RpcServerImpl {
        async fn ping(&self, node_id: NodeId) -> RpcResult<()> {
            if node_id == self.node.network.node_id {
                return Err(light_errors::SEND_TO_SELF.into());
            }
            let maybe_peer_infos = {
                let peers_infos = self
                    .node
                    .network
                    .light_network
                    .kademlia_dht
                    .peers_infos
                    .read()
                    .unwrap();
                peers_infos.get(&node_id).cloned()
            };
            if let Some(peer_infos) = maybe_peer_infos {
                peer_infos
                    .ping(&self.node.network.light_network.kademlia_dht.mail_tx)
                    .await
                    .map_err(|err| ErrorObject::owned(1000, err.to_string(), None::<()>))
            } else {
                Err(light_errors::PEER_MISSING.into())
            }
        }

        async fn store(
            &self,
            node_id: Option<NodeId>,
            dht_id: DhtId,
            value: Bytes,
        ) -> RpcResult<()> {
            let node_id = if node_id.is_none() {
                self.node.network.node_id
            } else {
                node_id.unwrap()
            };
            match dht_id {
                DhtId::Block => {
                    let block = DhtBlocks::decode(&value)?;
                    self.node
                        .network
                        .light_network
                        .store(&self.node.network.light_network.block_dht, node_id, block)
                        .await?;
                }
                DhtId::State => {}
            }
            Ok(())
        }

        async fn find_node(
            &self,
            node_id: Option<NodeId>,
            bucket: Bucket,
        ) -> RpcResult<Vec<NodeId>> {
            let node_ids = if node_id.is_none() || node_id == Some(self.node.network.node_id) {
                self.node
                    .network
                    .light_network
                    .kademlia_dht
                    .find_node(&bucket)
                    .into_iter()
                    .map(|ConnectionData { node_id, .. }| node_id)
                    .collect()
            } else {
                let node_id = node_id.unwrap();
                let sender = {
                    let peers_infos = self
                        .node
                        .network
                        .light_network
                        .kademlia_dht
                        .peers_infos
                        .read()
                        .unwrap();
                    let Some(peer_infos) = peers_infos.get(&node_id) else {
                        return Err(light_errors::PEER_MISSING.into());
                    };
                    peer_infos.sender.clone()
                };
                let message = AppRequestMessage::encode(
                    &self.node.network.light_network.kademlia_dht.chain_id,
                    sdk::FindNode {
                        bucket: bucket.to_be_bytes_vec(),
                    },
                );
                let Ok(p2p::message::Message::AppRequest(app_request)) = message else {
                    return Err(light_errors::INVALID_CONTENT.into());
                };
                let light_message: sdk::light_response::Message = sender
                    .send_and_app_response(
                        self.node.network.light_network.kademlia_dht.chain_id,
                        constants::SNOWFLAKE_HANDLER_ID,
                        &self.node.network.light_network.kademlia_dht.mail_tx,
                        SubscribableMessage::AppRequest(app_request),
                    )
                    .await
                    .map_err(|err| ErrorObject::owned(1000, err.to_string(), None::<()>))?;
                match light_message {
                    sdk::light_response::Message::Nodes(p2p::PeerList { claimed_ip_ports }) => {
                        if claimed_ip_ports.len() > 10 {
                            return Err(light_errors::INVALID_CONTENT.into());
                        }
                        claimed_ip_ports
                            .into_iter()
                            .filter_map(|claimed_ip_port| claimed_ip_port.try_into().ok())
                            .map(|ConnectionData { node_id, .. }| node_id)
                            .collect()
                    }
                    _ => return Err(light_errors::INVALID_CONTENT.into()),
                }
            };
            Ok(node_ids)
        }

        async fn find_value(
            &self,
            node_id: Option<NodeId>,
            dht_id: DhtId,
            bucket: Bucket,
        ) -> RpcResult<RpcValueOrNodes> {
            let value_or_nodes = if node_id.is_none() || node_id == Some(self.node.network.node_id)
            {
                match dht_id {
                    DhtId::Block => {
                        match self
                            .node
                            .network
                            .light_network
                            .block_dht
                            .dht
                            .store
                            .get_bucket(&bucket)
                        {
                            Some(value) => RpcValueOrNodes::Value(Value::Block(block_to_rpc(
                                DhtBlocks::decode(&value)?.block,
                                true,
                            ))),
                            None => {
                                let connections_data = self
                                    .node
                                    .network
                                    .light_network
                                    .kademlia_dht
                                    .find_node(&bucket);
                                let node_ids = connections_data
                                    .into_iter()
                                    .map(|ConnectionData { node_id, .. }| node_id)
                                    .collect();
                                RpcValueOrNodes::Nodes(node_ids)
                            }
                        }
                    }
                    _ => return Err(light_errors::INVALID_DHT.into()),
                }
            } else {
                let node_id = node_id.unwrap();
                let sender = {
                    let peers_infos = self
                        .node
                        .network
                        .light_network
                        .kademlia_dht
                        .peers_infos
                        .read()
                        .unwrap();
                    let Some(peer_infos) = peers_infos.get(&node_id) else {
                        return Err(light_errors::PEER_MISSING.into());
                    };
                    peer_infos.sender.clone()
                };
                let message = AppRequestMessage::encode(
                    &self.node.network.light_network.kademlia_dht.chain_id,
                    sdk::FindValue {
                        dht_id: dht_id.into(),
                        bucket: bucket.to_be_bytes_vec(),
                    },
                );
                let Ok(p2p::message::Message::AppRequest(app_request)) = message else {
                    return Err(light_errors::INVALID_CONTENT.into());
                };
                let rx = sender
                    .send_and_response(
                        &self.node.network.light_network.kademlia_dht.mail_tx,
                        SubscribableMessage::AppRequest(app_request),
                    )
                    .map_err(|err| ErrorObject::owned(1000, err.to_string(), None::<()>))?;

                let p2p::message::Message::AppResponse(app_response) = rx
                    .await
                    .map_err(|_| ErrorObject::borrowed(1000, "timeout", None))?
                else {
                    return Err(light_errors::INVALID_CONTENT.into());
                };
                if app_response.chain_id
                    != self
                        .node
                        .network
                        .light_network
                        .kademlia_dht
                        .chain_id
                        .as_ref()
                        .to_vec()
                {
                    return Err(light_errors::INVALID_CONTENT.into());
                }
                let bytes = app_response.app_bytes;
                let Ok((app_id, bytes)) = unsigned_varint::decode::u64(&bytes) else {
                    return Err(light_errors::INVALID_CONTENT.into());
                };
                if app_id != constants::SNOWFLAKE_HANDLER_ID {
                    return Err(light_errors::INVALID_CONTENT.into());
                }
                let Ok(light_message) = InboundMessage::decode(bytes) else {
                    return Err(light_errors::INVALID_CONTENT.into());
                };
                match light_message {
                    sdk::light_response::Message::Value(sdk::Value { value }) => match dht_id {
                        DhtId::Block => {
                            let block = DhtBlocks::decode(&value)?;
                            RpcValueOrNodes::Value(Value::Block(block_to_rpc(block.block, true)))
                        }
                        _ => return Err(light_errors::INVALID_DHT.into()),
                    },
                    sdk::light_response::Message::Nodes(p2p::PeerList { claimed_ip_ports }) => {
                        if claimed_ip_ports.len() > 10 {
                            return Err(light_errors::INVALID_CONTENT.into());
                        }
                        let node_ids = claimed_ip_ports
                            .into_iter()
                            .filter_map(|claimed_ip_port| claimed_ip_port.try_into().ok())
                            .map(|ConnectionData { node_id, .. }| node_id)
                            .collect();
                        RpcValueOrNodes::Nodes(node_ids)
                    }
                    _ => return Err(light_errors::INVALID_CONTENT.into()),
                }
            };
            Ok(value_or_nodes)
        }
    }
}

use crate::net::node::NodeError;
use crate::node::Node;
use rpc_impl::{EthServer, LightServer, NetServer, RpcServerImpl, Web3Server};

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
        rpc.merge(EthServer::into_rpc(rpc_impl.clone()))
            .expect("should not fail");
        rpc.merge(LightServer::into_rpc(rpc_impl))
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
        let _ = rustls::crypto::ring::default_provider().install_default();

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
        let config = NetworkConfig {
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
                find_nodes: 0,
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
            max_light_peers: None,
            bootstrappers: HashMap::new(),
            dht_buckets: DhtBuckets {
                block: Default::default(),
            },
            max_latency_records: 1,
            max_out_connections: 1,
            sync_headers: false,
        };
        let node = Node::new(config);

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
