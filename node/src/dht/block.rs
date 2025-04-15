use ruint::Uint;
use crate::dht::{ConcreteDht, Dht, Task};
use crate::id::NodeId;

pub type DhtBlocks = Dht<16, 2, 1, 0>;

impl ConcreteDht<u16> for DhtBlocks {
    fn from_node_id(node_id: NodeId, k: u16) -> Self {
        let k = Uint::from_be_bytes(k.to_be_bytes());
        DhtBlocks::_from_node_id(node_id, k)
    }

    fn is_desired_bucket(&self, bucket: u16) -> bool {
        let bucket = Uint::from_be_bytes(bucket.to_be_bytes());
        self._is_desired_bucket(bucket)
    }
}

impl Task for DhtBlocks {
    async fn run(&self) {
        // let
        todo!()
    }
}