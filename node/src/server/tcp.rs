use super::msg::DELIMITER_LEN;
use crate::net::node::NodeError;
use crate::stats;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio_rustls::TlsStream;

pub async fn write_stream_message(
    write: &mut WriteHalf<TlsStream<TcpStream>>,
    bytes: Vec<u8>,
) -> Result<(), NodeError> {
    tokio::time::timeout(Duration::from_secs(5), async {
        write.write_all(&bytes).await?;
        write.flush().await?;
        stats::messages::inc_sent_messages_bytes(bytes.len() as u64);
        Ok(())
    })
    .await?
}

pub async fn read_stream_message(
    read: &mut ReadHalf<TlsStream<TcpStream>>,
) -> Result<Vec<u8>, std::io::Error> {
    let msg_len = read.read_u32().await?;
    stats::messages::inc_recv_messages_bytes(DELIMITER_LEN as u64);
    read_stream_packet(read, msg_len).await
}

pub async fn read_stream_packet(
    read: &mut ReadHalf<TlsStream<TcpStream>>,
    len: u32,
) -> Result<Vec<u8>, std::io::Error> {
    let mut buf = vec![0; len as usize];
    read.read_exact(&mut buf).await?;
    stats::messages::inc_recv_messages_bytes(len as u64);
    Ok(buf)
}
