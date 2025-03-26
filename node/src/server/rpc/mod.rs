use flume::Sender;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::ErrorKind;
use std::time::Instant;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::{
    unix::{OwnedReadHalf, OwnedWriteHalf},
    UnixListener,
};
use tokio::sync::oneshot;

mod types;

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
    listener: UnixListener,
    tx: Sender<(Vec<u8>, Instant)>,
}

impl Rpc {
    pub fn new(path: &str, tx: Sender<(Vec<u8>, Instant)>) -> Self {
        log::debug!("starting rpc at {path}");

        if fs::metadata(path).is_ok() {
            fs::remove_file(path).expect("could not remove socket");
        }

        let listener = UnixListener::bind(path).expect("failed to bind socket");

        Self { listener, tx }
    }

    pub async fn start(&self, mut shutdown_rx: oneshot::Receiver<()>) {
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
