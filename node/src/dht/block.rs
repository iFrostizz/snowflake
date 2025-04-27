use crate::dht::kademlia::LockedMapDb;
use crate::dht::{Bucket, DhtId};
use crate::dht::{ConcreteDht, Dht};
use crate::id::NodeId;
use crate::message::SubscribableMessage;
use crate::node::{MessageOrSubscribable, Node, SinglePickerConfig};
use crate::utils::constants::DEFAULT_DEADLINE;
use crate::utils::unpacker::StatelessBlock;
use crate::Arc;
use alloy::primitives::keccak256;
use proto_lib::p2p::message::Message;
use proto_lib::p2p::GetAcceptedFrontier;
use proto_lib::p2p::{EngineType, GetAncestors};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;
use tokio::sync::broadcast;

pub type DhtBlocks = Dht<RwLock<HashMap<Bucket, Vec<u8>>>>;

impl ConcreteDht<u64> for DhtBlocks {
    fn id() -> DhtId {
        DhtId::Block
    }

    fn key_to_bucket(block_number: u64) -> Bucket {
        let arr: [u8; 20] = keccak256(block_number.to_be_bytes())[0..20].try_into().unwrap();
        <Bucket>::from_be_bytes(arr)
    }
}

impl DhtBlocks {
    async fn sync_process(self: Arc<Self>, node: Arc<Node>) {
        let chain_id = node.network.config.c_chain_id.as_ref().to_vec();
        let mut bootstrapper = Self::pick_random_bootstrapper(&node).await;

        let message = SubscribableMessage::GetAcceptedFrontier(GetAcceptedFrontier {
            chain_id: chain_id.clone(),
            request_id: rand::random(),
            deadline: DEFAULT_DEADLINE,
        });
        let message = loop {
            if let Some(Message::AcceptedFrontier(res)) = node
                .send_to_peer(
                    &MessageOrSubscribable::Subscribable(message.clone()),
                    &bootstrapper,
                )
                .await
            {
                break res;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };

        let mut last_container_id = message.container_id;
        loop {
            let message = SubscribableMessage::GetAncestors(GetAncestors {
                chain_id: chain_id.clone(),
                request_id: rand::random(),
                deadline: DEFAULT_DEADLINE,
                container_id: last_container_id.clone(),
                engine_type: EngineType::Snowman.into(),
            });
            let message = loop {
                if let Some(Message::Ancestors(res)) = node
                    .send_to_peer(
                        &MessageOrSubscribable::Subscribable(message.clone()),
                        &bootstrapper,
                    )
                    .await
                {
                    break res;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            };

            let len = message.containers.len();
            for (i, container) in message.containers.into_iter().enumerate() {
                let block = StatelessBlock::unpack(&container).unwrap();
                if i == len - 1 {
                    last_container_id = block.id.as_ref().to_vec();
                }
                let number = u64::from_be_bytes(*block.block.header.number());
                self.store.insert(Self::key_to_bucket(number), container);
                bootstrapper = Self::pick_random_bootstrapper(&node).await;
            }
        }
    }

    pub async fn sync_headers(self: Arc<Self>, node: Arc<Node>, mut rx: broadcast::Receiver<()>) {
        let dht = self.clone();
        let process = tokio::spawn(dht.sync_process(node));
        tokio::select! {
            _ = process => {},
            _ = rx.recv() => {},
        }
    }

    async fn pick_random_bootstrapper(node: &Arc<Node>) -> NodeId {
        let mut maybe_bootstrapper = node.pick_peer(SinglePickerConfig::Bootstrapper);
        while maybe_bootstrapper.is_none() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            maybe_bootstrapper = node.pick_peer(SinglePickerConfig::Bootstrapper);
        }
        maybe_bootstrapper.unwrap()
    }
}
