use crate::dht::Bucket;
use crate::dht::{ConcreteDht, Dht, Task};
use crate::id::NodeId;
use alloy::primitives::keccak256;

type BlockId = [u8; 32]; // TODO change to end type

pub type DhtBlocks = Dht<BlockId>;

impl ConcreteDht<BlockId> for DhtBlocks {
    fn from_node_id(node_id: NodeId, k: Bucket) -> Self {
        DhtBlocks::_from_node_id(node_id, k)
    }

    fn is_desired_bucket(&self, bucket: Bucket) -> bool {
        self._is_desired_bucket(bucket)
    }

    fn in_to_bucket(val: BlockId) -> Bucket {
        let arr: [u8; 20] = keccak256(val)[0..20].try_into().unwrap();
        <Bucket>::from_be_bytes(arr)
    }
}

impl Task for DhtBlocks {
    async fn run(&self) {
        // let
        todo!()
    }
}
