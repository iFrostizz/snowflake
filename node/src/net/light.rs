use crate::dht::block::DhtBlocks;
use crate::dht::kademlia::KademliaDht;
use crate::dht::{Bucket, ConcreteDht, DhtId, LightMessage, LightResult, Task};
use crate::id::NodeId;
use crate::net::node::NodeError;
use crate::net::RwLock;
use crate::node::Node;
use crate::utils::rlp::Block;
use crate::utils::unpacker::StatelessBlock;
use crate::Arc;
use std::collections::HashMap;
use tokio::sync::broadcast;
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct LightNetwork {
    pub block_dht: Arc<DhtBlocks>,
    sync_headers: bool,
}

impl LightNetwork {
    pub fn new(node_id: NodeId, sync_headers: bool) -> Self {
        let store = RwLock::new(HashMap::new());
        let block_dht = Arc::new(DhtBlocks::new(
            node_id,
            Bucket::from(10),
            KademliaDht::new(vec![], store),
        ));
        Self {
            block_dht,
            sync_headers,
        }
    }

    pub async fn start(
        &self,
        node: Arc<Node>,
        mut rx: broadcast::Receiver<()>,
    ) -> Result<(), NodeError> {
        if self.sync_headers {
            let mut block_dht = self.block_dht.clone();
            let rx = rx.resubscribe();
            tokio::spawn(async move {
                block_dht.sync_headers(node, rx).await;
            });
        }

        // let (tx_light_message, rx_light_message) = flume::unbounded();

        // TODO listen to light messages
        // loop {
        //     tokio::select! {
        //         // message = rx_light_message.recv_async() => {},
        //         _ = rx.recv() => {
        //             return Ok(())
        //         }
        //     }
        // }

        let _ = rx.recv().await;
        Ok(())
    }

    pub fn manage_message(&self, message: LightMessage, resp: oneshot::Sender<LightResult>) {
        let res = match message {
            LightMessage::Store(dht_id, value) => match dht_id {
                DhtId::Block => self.block_dht.store(value),
                _ => unimplemented!(),
            },
            LightMessage::FindNode(dht_id, bucket) => match dht_id {
                DhtId::Block => self.block_dht.find_node(bucket),
                _ => unimplemented!(),
            },
            LightMessage::FindValue(dht_id, bucket) => match dht_id {
                DhtId::Block => self.block_dht.find_value(bucket),
                _ => unimplemented!(),
            },
        };

        let _ = resp.send(res);
    }

    async fn find_value<V>(&self, bucket: &Bucket) -> Result<V, ()> {
        // TODO externally request to other nodes
        todo!()
    }

    pub async fn find_content<DHT, K, V>(&self, dht: DHT, key: K) -> Result<V, ()>
    where
        DHT: ConcreteDht<K> + DhtContent<V>,
    {
        let bucket = DHT::key_to_bucket(key);
        if let Some(value) = dht.get_from_store(&bucket) {
            return Ok(value);
        }
        self.find_value(&bucket).await
    }
}

pub trait DhtContent<V> {
    fn get_from_store(&self, key: &Bucket) -> Option<V>;
}

impl DhtContent<Block> for DhtBlocks {
    fn get_from_store(&self, key: &Bucket) -> Option<Block> {
        let block_bytes = self.kademlia_dht.get(key)?;
        let block =
            StatelessBlock::unpack(&block_bytes).expect("should not be stored if ill-formed");
        Some(block.block)
    }
}
