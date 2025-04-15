use ruint::Uint;
use crate::id::NodeId;

pub struct KademliaDht<const BITS: usize, const LIMBS: usize> {
    node_id: NodeId,
}

pub enum ValueOrNodes {
    Value(Vec<u8>),
    Nodes(Vec<NodeId>),
}

impl<const BITS: usize, const LIMBS: usize> KademliaDht<BITS, LIMBS> {
    pub fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }

    /// Find up to `n` unique nodes that are the closest to the `bucket`.
    pub fn find_node(&self, bucket: Uint<BITS, LIMBS>, n: u16) -> Vec<NodeId> {
        todo!()
    }

    /// Find up to `n` unique nodes that are the closest to the `bucket` or return the value
    /// if it is in the store.
    pub fn find_value(&self, bucket: Uint<BITS, LIMBS>, n: u16) -> ValueOrNodes {
        todo!()
    }
}