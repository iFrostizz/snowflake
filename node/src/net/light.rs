use crate::net::RwLock;
use crate::Arc;
use crate::dht::Bucket;
use std::collections::HashMap;
use crate::dht::kademlia::KademliaDht;
use crate::dht::block::DhtBlocks;
use crate::id::NodeId;
use crate::node::Node;

#[derive(Debug)]
pub struct LightNetwork {
    block_dht: Arc<DhtBlocks>,
    bootstrappers: Vec<NodeId>,
    sync_headers: bool,
}

pub enum DhtType {
    Block,
}

impl LightNetwork {
    pub fn new(node_id: NodeId, bootstrappers: Vec<NodeId>, sync_headers: bool) -> Self {
        let store = RwLock::new(HashMap::new());
        let block_dht = Arc::new(DhtBlocks::new(
            node_id,
            Bucket::from(10),
            KademliaDht::new(vec![], store),
        ));
        Self {block_dht, bootstrappers, sync_headers}
    }
    
    pub async fn start(&self, node: Arc<Node>) {
        *node.network.bootstrappers.write().unwrap() = self.bootstrappers.clone().into_iter().collect();
        if self.sync_headers {
            let mut block_dht = self.block_dht.clone();
            tokio::spawn(async move {
                block_dht.sync_headers(node).await;
            });
        }
    }

    pub async fn find_content<V>(&self, dht: DhtType, bucket: Bucket) -> Result<V, ()> {
        match dht {
            DhtType::Block => {

            }
        }
        todo!()
    }
}