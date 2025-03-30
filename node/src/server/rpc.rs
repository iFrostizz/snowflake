use flume::Sender;
use jsonrpsee::server::ServerBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Instant;
use jsonrpsee::Methods;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::{
    unix::{OwnedReadHalf, OwnedWriteHalf},
    UnixListener,
};
use tokio::sync::oneshot;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Method {
    #[serde(rename = "eth_chainId")]
    ChainId,
    #[serde(rename = "eth_feeHistory")]
    FeeHistory,
    #[serde(rename = "eth_sendRawTransaction")]
    SendRawTransaction,
    #[serde(rename = "eth_subscribe")]
    Subscribe,
    #[serde(rename = "eth_subscription")]
    Subscription,
}

impl Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Method::ChainId => f.write_str("eth_chainId"),
            Method::FeeHistory => f.write_str("eth_feeHistory"),
            Method::SendRawTransaction => f.write_str("eth_sendRawTransaction"),
            Method::Subscribe => f.write_str("eth_subscribe"),
            Method::Subscription => f.write_str("eth_subscription"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Request {
    id: usize,
    method: Method,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Vec<JsonVal>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum JsonVal {
    String(String),
    Bool(bool),
}

#[derive(Debug, Serialize)]
pub struct Response {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    method: Option<Method>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ResponseError>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    params: HashMap<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseError {
    code: i32,
    message: String,
}

pub struct Rpc {
    network: Arc<Network>,
    listener: UnixListener,
    tx: Sender<(Vec<u8>, Instant)>,
}

macro_rules! not_implemented {
    () => {
        return jsonrpsee::core::RpcResult::Err(jsonrpsee::types::ErrorObject::borrowed(
            -32000,
            "unimplemented method",
            None,
        ))
    };
}

mod rpc_impl {
    use crate::Arc;
use crate::Network;

use alloy::primitives::{Address, FixedBytes, U256};
    use jsonrpsee::core::{async_trait, RpcResult, SubscriptionResult};
    use jsonrpsee::server::{
        IntoSubscriptionCloseResponse, PendingSubscriptionSink, SubscriptionCloseResponse,
        SubscriptionMessage,
    };
    use jsonrpsee::{proc_macros::rpc, Extensions};
    use serde::{Deserialize, Serialize};

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

    #[derive(Debug, Default, Serialize, Deserialize, Clone)]
    pub enum BlockParameter {
        Number(u64),
        #[default]
        Latest,
        Earliest,
        Pending,
        Safe,
        Finalized,
    }

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct TransactionObject {
        block_hash: Bytes32,
        block_number: u64,
        from: Address,
        gas: u64,
        gas_price: u64,
        hash: Bytes32,
        input: Vec<u8>,
        nonce: u64,
        to: Address,
        transaction_index: Option<u64>,
        value: U256,
        v: u8,
        r: u64,
        s: u64,
    }

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub enum TransactionOrHash {
        Transactions(Vec<TransactionObject>),
        Hashes(Vec<Bytes32>),
    }

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct BlockObject {
        number: Option<u64>,
        hash: Option<Bytes32>,
        parent_hash: Bytes32,
        nonce: u64,
        sha3uncles: Bytes32,
        logs_bloom: BloomFilter,
        transactions_root: Bytes32,
        state_root: Bytes32,
        receipts_root: Bytes32,
        miner: Address,
        difficulty: U256,
        total_difficulty: U256,
        extra_data: Vec<u8>,
        size: u64,
        gas_limit: u64,
        gas_used: u64,
        timestamp: u64,
        transactions: TransactionOrHash,
        uncles: Vec<Bytes32>,
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
        fn sha3(&self, data: &[u8]) -> RpcResult<Bytes32>;
    }

    #[rpc(server, namespace = "net")]
    pub trait Net {
        #[method(name = "version")]
        fn version(&self) -> RpcResult<u64>;

        #[method(name = "listening")]
        fn listening(&self) -> RpcResult<bool>;

        #[method(name = "peerCount")]
        fn peer_count(&self) -> RpcResult<u64>;
    }

    #[rpc(server, namespace = "eth")]
    pub trait Eth {
        #[method(name = "protocolVersion")]
        fn protocol_version(&self) -> RpcResult<String>;

        #[method(name = "syncing")]
        fn syncing(&self) -> RpcResult<bool> {
            RpcResult::Ok(false)
        }

        #[method(name = "coinbase")]
        fn coinbase(&self) -> RpcResult<String> {
            RpcResult::Ok("0x0100000000000000000000000000000000000000".to_string())
        }

        #[method(name = "chainId")]
        fn chain_id(&self) -> RpcResult<u64>;

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
        fn get_transaction_count_by_hash(&self, hash: Bytes32) -> RpcResult<u64>;

        #[method(name = "getBlockTransactionCountByNumber")]
        fn get_transaction_count_by_number(
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
        fn send_raw_transaction(&self, data: Vec<u8>) -> RpcResult<Bytes32>;

        #[method(name = "call")]
        fn call(&self, object: CallObject, block_parameter: BlockParameter) -> RpcResult<Vec<u8>>;

        #[method(name = "estimateGas")]
        fn estimate_gas(
            &self,
            object: CallObject,
            block_parameter: BlockParameter,
        ) -> RpcResult<Vec<u8>>;

        #[method(name = "getBlockByHash")]
        fn get_block_by_hash(&self, hash: Bytes32, full: bool) -> RpcResult<Option<BlockObject>>;

        #[method(name = "getBlockByNumber")]
        fn get_block_by_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<Option<BlockObject>>;

        #[method(name = "getTransaction_by_hash")]
        fn get_transaction_by_hash(&self, hash: Bytes32) -> RpcResult<Option<TransactionObject>>;

        #[method(name = "getTransactionByHashAndIndex")]
        fn get_transaction_by_hash_and_index(
            &self,
            hash: Bytes32,
            position: u64,
        ) -> RpcResult<Option<TransactionObject>>;

        #[method(name = "getTransactionByBlockNumberAndIndex")]
        fn get_transaction_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            position: u64,
        ) -> RpcResult<Option<TransactionObject>>;

        #[method(name = "getTransactionReceipt")]
        fn get_transaction_receipt(
            &self,
            hash: Bytes32,
        ) -> RpcResult<Option<TransactionReceiptObject>>;

        #[method(name = "getUncleByBlockHashAndIndex")]
        fn get_uncle_by_block_hash_and_index(
            &self,
            hash: Bytes32,
            index: u64,
        ) -> RpcResult<Option<BlockObject>>;

        #[method(name = "getUncleByBlockNumberAndIndex")]
        fn get_uncle_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            index: u64,
        ) -> RpcResult<Option<BlockObject>>;

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

    #[derive(Debug, Clone)]
    pub struct RpcServerImpl {
        pub(crate) network: Arc<Network>
    }

    // #[async_trait]
    impl Web3Server for RpcServerImpl {
        fn client_version(&self) -> RpcResult<String> {
            not_implemented!()
        }

        fn sha3(&self, _data: &[u8]) -> RpcResult<Bytes32> {
            not_implemented!()
        }
    }

    impl NetServer for RpcServerImpl {
        fn version(&self) -> RpcResult<u64> {
            not_implemented!()
        }

        fn listening(&self) -> RpcResult<bool> {
            not_implemented!()
        }

        fn peer_count(&self) -> RpcResult<u64> {
            let peer_count = self.network.peers_infos.read().unwrap().len() as u64;
            Ok(peer_count)
        }
    }

    impl EthServer for RpcServerImpl {
        fn protocol_version(&self) -> RpcResult<String> {
            not_implemented!()
        }

        fn chain_id(&self) -> RpcResult<u64> {
            not_implemented!()
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

        fn get_balance(&self, block_parameter: BlockParameter) -> RpcResult<U256> {
            not_implemented!()
        }

        fn get_storage_at(&self, block_parameter: BlockParameter) -> RpcResult<Bytes32> {
            not_implemented!()
        }

        fn get_transaction_count(&self, block_parameter: BlockParameter) -> RpcResult<u64> {
            not_implemented!()
        }

        fn get_transaction_count_by_hash(&self, hash: Bytes32) -> RpcResult<u64> {
            not_implemented!()
        }

        fn get_transaction_count_by_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<u64> {
            not_implemented!()
        }

        fn get_uncle_count_by_block_hash(&self, block_parameter: BlockParameter) -> RpcResult<u64> {
            not_implemented!()
        }

        fn get_uncle_count_by_block_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<u64> {
            not_implemented!()
        }

        fn get_code(
            &self,
            address: Address,
            block_parameter: BlockParameter,
        ) -> RpcResult<Vec<u8>> {
            not_implemented!()
        }

        fn sign(&self, address: Address, message: Vec<u8>) -> RpcResult<Bytes32> {
            not_implemented!()
        }

        fn sign_transaction(&self, object: UnsignedTransactionObject) -> RpcResult<Vec<u8>> {
            not_implemented!()
        }

        fn send_transaction(&self, object: UnsignedTransactionObject) -> RpcResult<Bytes32> {
            not_implemented!()
        }

        fn send_raw_transaction(&self, data: Vec<u8>) -> RpcResult<Bytes32> {
            not_implemented!()
        }

        fn call(&self, object: CallObject, block_parameter: BlockParameter) -> RpcResult<Vec<u8>> {
            not_implemented!()
        }

        fn estimate_gas(
            &self,
            object: CallObject,
            block_parameter: BlockParameter,
        ) -> RpcResult<Vec<u8>> {
            not_implemented!()
        }

        fn get_block_by_hash(&self, hash: Bytes32, full: bool) -> RpcResult<Option<BlockObject>> {
            not_implemented!()
        }

        fn get_block_by_number(
            &self,
            block_parameter: BlockParameter,
        ) -> RpcResult<Option<BlockObject>> {
            not_implemented!()
        }

        fn get_transaction_by_hash(&self, hash: Bytes32) -> RpcResult<Option<TransactionObject>> {
            not_implemented!()
        }

        fn get_transaction_by_hash_and_index(
            &self,
            hash: Bytes32,
            position: u64,
        ) -> RpcResult<Option<TransactionObject>> {
            not_implemented!()
        }

        fn get_transaction_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            position: u64,
        ) -> RpcResult<Option<TransactionObject>> {
            not_implemented!()
        }

        fn get_transaction_receipt(
            &self,
            hash: Bytes32,
        ) -> RpcResult<Option<TransactionReceiptObject>> {
            not_implemented!()
        }

        fn get_uncle_by_block_hash_and_index(
            &self,
            hash: Bytes32,
            index: u64,
        ) -> RpcResult<Option<BlockObject>> {
            not_implemented!()
        }

        fn get_uncle_by_block_number_and_index(
            &self,
            block_parameter: BlockParameter,
            index: u64,
        ) -> RpcResult<Option<BlockObject>> {
            not_implemented!()
        }

        fn new_filter(&self, block_parameter: BlockParameter) -> RpcResult<FilterObject> {
            not_implemented!()
        }

        fn new_block_filter(&self) -> RpcResult<FilterId> {
            not_implemented!()
        }

        fn new_pending_transaction_filter(&self) -> RpcResult<FilterId> {
            not_implemented!()
        }

        fn uninstall_filter(&self, filter_id: FilterId) -> RpcResult<bool> {
            not_implemented!()
        }

        fn get_filter_changes(&self, filter_id: FilterId) -> RpcResult<Vec<LogObject>> {
            not_implemented!()
        }

        fn get_filter_logs(&self, filter_id: FilterId) -> RpcResult<Vec<LogObject>> {
            not_implemented!()
        }

        fn get_logs(&self, filter_object: FilterObject) -> RpcResult<Vec<LogObject>> {
            not_implemented!()
        }
    }
}

use rpc_impl::{EthServer, NetServer, RpcServerImpl, Web3Server};
use crate::net::Network;

impl Rpc {
    pub fn new(network: Arc<Network>, path: &str, tx: Sender<(Vec<u8>, Instant)>) -> Self {
        log::debug!("starting rpc at {path}"); // TODO deprecate IPC

        if fs::metadata(path).is_ok() {
            fs::remove_file(path).expect("could not remove socket");
        }

        let listener = UnixListener::bind(path).expect("failed to bind socket");

        Self { network, listener, tx }
    }

    pub async fn start(&self, mut shutdown_rx: oneshot::Receiver<()>) {
        let rpc_port = 9781; // TODO configurable
        let server = ServerBuilder::default()
            .build(format!("127.0.0.1:{}", rpc_port))
            .await
            .unwrap();
        let addr = server.local_addr().unwrap();
        log::debug!("Listening on {}", addr);

        let rpc_impl = RpcServerImpl {
            network: self.network.clone(),
        };
        let mut rpc = Web3Server::into_rpc(rpc_impl.clone());
        rpc.merge(NetServer::into_rpc(rpc_impl.clone()))
            .expect("should not fail");
        rpc.merge(EthServer::into_rpc(rpc_impl))
            .expect("should not fail");
        let server_handle = server.start(rpc);

        tokio::spawn(server_handle.stopped());

        let mut threads = Vec::new();
        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    match accepted {
                        Ok((stream, _addr)) => {
                            log::debug!("listener got a new socket");
                            let (read, write) = stream.into_split();

                            let tx = self.tx.clone();

                            let (thread_tx, thread_rx) = oneshot::channel();

                            tokio::spawn(async move {
                                if let Err(err) = Self::communicate(read, write, tx, thread_rx).await {
                                    match err.kind() {
                                        ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe => (), // normal disconnection
                                        rest => {
                                            log::error!("error when communicating over IPC: {rest}")
                                        }
                                    }
                                }
                            });

                            threads.push(thread_tx);
                        }
                        Err(err) => log::error!("err from socket accept {err:?}"),
                    }
                }
                _ = &mut shutdown_rx => {
                    log::debug!("shutting down rpc!");
                    while let Some(thread) = threads.pop() {
                        let _ = thread.send(());
                    }

                    return
                }
            }
        }
    }

    async fn communicate(
        read: OwnedReadHalf,
        mut write: OwnedWriteHalf,
        tx: Sender<(Vec<u8>, Instant)>,
        thread_rx: oneshot::Receiver<()>,
    ) -> Result<(), io::Error> {
        let (read_tx, read_rx) = oneshot::channel();
        tokio::spawn(async move {
            Self::process_incoming_requests(tx, &read, &mut write, read_rx).await
        });

        let _ = thread_rx.await;

        let _ = read_tx.send(());

        Err(ErrorKind::BrokenPipe.into())
    }

    async fn read_message(read: &OwnedReadHalf) -> Result<Request, io::Error> {
        log::trace!("read");

        let mut buf = Vec::new();
        loop {
            read.readable().await?;

            let r = read.try_read_buf(&mut buf);
            log::trace!("{:?}", &r);
            match r {
                Ok(0) => break,
                Ok(_) => continue,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if buf.is_empty() {
                        continue;
                    } else {
                        break;
                    }
                }
                Err(e) => return Err(e),
            }
        }

        let req = serde_json::from_slice(&buf)?;
        Ok(req)
    }

    async fn write_message(
        write: &mut OwnedWriteHalf,
        method: Option<Method>,
        id: Option<usize>,
        result: Option<Result<String, String>>,
        params: HashMap<String, Value>,
    ) -> Result<(), io::Error> {
        write.writable().await?;

        let (result, error) = result.map_or((None, None), |result| {
            (result.clone().ok(), result.clone().err())
        });

        log::trace!("write");

        let response = Response {
            jsonrpc: "2.0".to_string(),
            method,
            id,
            result,
            error: error.map(|message| ResponseError { code: -69, message }),
            params,
        };
        let data = serde_json::to_vec(&response)?;
        write.write_all(&data).await?;
        Ok(())
    }

    fn process_maybe_req(
        maybe_req: Result<Request, io::Error>,
        tx: &Sender<(Vec<u8>, Instant)>,
    ) -> (usize, Result<String, String>) {
        match maybe_req {
            Ok(req) => {
                let id = req.id;
                let res = Self::process_req(req, tx);
                (id, res)
            }
            Err(err) => (0, Err(err.to_string())),
        }
    }

    fn process_req(req: Request, tx: &Sender<(Vec<u8>, Instant)>) -> Result<String, String> {
        match req.method {
            Method::SendRawTransaction => {
                if let Some(signed_transaction) = req.params.unwrap().first() {
                    match signed_transaction {
                        JsonVal::String(signed_transaction) => {
                            let as_bytes =
                                hex::decode(signed_transaction.strip_prefix("0x").unwrap())
                                    .unwrap();
                            tx.send((as_bytes, Instant::now())).unwrap();
                            // TODO calculate hash from RLP encoded tx hash
                            let hash =
                                "0x0000000000000000000000000000000000000000000000000000000000000000"
                                    .to_string();
                            Ok(hash)
                        }
                        _ => Err("invalid type".to_string()),
                    }
                } else {
                    Err("no param passed".to_string())
                }
            }
            _ => Err(format!("invalid method {}", &req.method)),
        }
    }

    async fn process_incoming(
        tx: &Sender<(Vec<u8>, Instant)>,
        read: &OwnedReadHalf,
        write: &mut OwnedWriteHalf,
    ) -> Result<(), io::Error> {
        let maybe_req = Self::read_message(read).await;
        log::debug!("maybe_req: {:?}", &maybe_req);
        if maybe_req
            .as_ref()
            .is_err_and(|err| err.kind() != ErrorKind::WouldBlock)
        {
            return Err(maybe_req.unwrap_err());
        }

        let (id, result) = Self::process_maybe_req(maybe_req, tx);

        Self::write_message(write, None, Some(id), Some(result), HashMap::new()).await
    }

    async fn process_incoming_requests(
        tx: Sender<(Vec<u8>, Instant)>,
        read: &OwnedReadHalf,
        write: &mut OwnedWriteHalf,
        mut read_rx: oneshot::Receiver<()>,
    ) -> Result<(), io::Error> {
        loop {
            tokio::select! {
                res = Self::process_incoming(&tx, read, write) => {
                    res?;
                }
                _ = &mut read_rx => {
                    return Ok(())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Rpc;
    use alloy::providers::{network::EthereumWallet, Provider, ProviderBuilder};
    use alloy::signers::local::PrivateKeySigner;
    use alloy::transports::ipc::IpcConnect;
    use tokio::sync::oneshot;

    #[tokio::test(flavor = "multi_thread")]
    async fn send_transaction() {
        // tracing_subscriber::fmt::init();

        let path = "/tmp/snowflake_test1.ipc";

        let (tx, rx) = flume::unbounded();
        tokio::spawn(async move {
            rx.recv().unwrap();
        });
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        log::debug!("start");
        tokio::spawn(async move {
            let rpc = Rpc::new(path, tx);
            rpc.start(shutdown_rx).await;
        });

        let signer = PrivateKeySigner::random();
        let wallet = EthereumWallet::from(signer);
        let ipc = IpcConnect::new(path.to_string());
        let provider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .on_ipc(ipc.clone())
            .await
            .unwrap();

        log::debug!("sending");
        let _ = provider.send_raw_transaction(&hex::decode("f86680843b9aca00825208940000000000000000000000000000000000000000808083015285a06c1cbdd2e8d1a0a9119159527cbe00151778bcc5ea8ae8ccd687b05f5316c325a044ea618c2ca67374cbec06747b048e7915a488f4cf9f911887bfa9e766112846").unwrap()).await.unwrap();

        shutdown_tx.send(()).unwrap();
    }
}
