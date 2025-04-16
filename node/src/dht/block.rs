use crate::dht::{ConcreteDht, Dht, Task};
use crate::id::NodeId;
use alloy::primitives::keccak256;
use ruint::Uint;

type BlockId = [u8; 32]; // TODO change to end type

pub type DhtBlocks = Dht<16, 1, 2, 0, BlockId>;

impl ConcreteDht<u16, BlockId> for DhtBlocks {
    fn from_node_id(node_id: NodeId, k: u16) -> Self {
        let k = Uint::from_be_bytes(k.to_be_bytes());
        DhtBlocks::_from_node_id(node_id, k)
    }

    fn is_desired_bucket(&self, bucket: u16) -> bool {
        let bucket = Uint::from_be_bytes(bucket.to_be_bytes());
        self._is_desired_bucket(bucket)
    }

    fn in_to_bucket(val: BlockId) -> u16 {
        let arr = keccak256(val)[0..2].try_into().unwrap();
        u16::from_be_bytes(arr)
    }
}

impl Task for DhtBlocks {
    async fn run(&self) {
        // let
        todo!()
    }
}
