use crate::id::NodeId;
use ruint::Uint;
use std::collections::HashMap;

#[derive(Debug)]
pub struct KademliaDht<
    const BITS: usize,
    const LIMBS: usize,
    const B_SIZE: usize,
    const OFFSET: usize,
> {
    store: HashMap<Uint<BITS, LIMBS>, Vec<u8>>,
    nodes: Vec<NodeId>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ValueOrNodes {
    Value(Vec<u8>),
    Nodes(Vec<NodeId>),
}

impl<const BITS: usize, const LIMBS: usize, const B_SIZE: usize, const OFFSET: usize>
    KademliaDht<BITS, LIMBS, B_SIZE, OFFSET>
{
    pub fn new(nodes: Vec<NodeId>) -> Self {
        Self {
            store: HashMap::new(),
            nodes,
        }
    }

    fn take_node_id_part(node_id: NodeId) -> Uint<BITS, LIMBS> {
        let arr: [u8; 20] = node_id.into();
        let be_bytes: [u8; B_SIZE] = arr[OFFSET..OFFSET + B_SIZE].try_into().unwrap();
        Uint::from_be_bytes(be_bytes)
    }

    fn distance(a: &Uint<BITS, LIMBS>, b: Uint<BITS, LIMBS>) -> Uint<BITS, LIMBS> {
        a ^ b
    }

    pub fn store(&mut self, key: Uint<BITS, LIMBS>, value: Vec<u8>) -> Option<Vec<u8>> {
        self.store.insert(key, value)
    }

    /// Find up to `n` unique nodes that are the closest to the `bucket`.
    pub fn find_node(&self, bucket: &Uint<BITS, LIMBS>, mut n: usize) -> Vec<NodeId> {
        if self.nodes.is_empty() {
            return vec![];
        }

        if n >= self.nodes.len() {
            n = self.nodes.len() - 1;
        }
        let distances: Vec<Uint<BITS, LIMBS>> = self
            .nodes
            .iter()
            .map(|node_id| Self::distance(bucket, Self::take_node_id_part(*node_id)))
            .collect();
        let mut distances2 = distances.clone();
        let (closest_buckets, ..) = distances2.select_nth_unstable(n);
        let closest_buckets: Vec<_> = closest_buckets.to_vec();
        closest_buckets
            .iter()
            .map(|bucket| {
                let i = distances
                    .iter()
                    .position(|sorted_bucket| bucket == sorted_bucket)
                    .unwrap();
                self.nodes[i]
            })
            .collect()
    }

    /// Find up to `n` unique nodes that are the closest to the `bucket` or return the value
    /// if it is in the store.
    pub fn find_value(&self, bucket: &Uint<BITS, LIMBS>, n: usize) -> ValueOrNodes {
        if let Some(value) = self.store.get(bucket) {
            return ValueOrNodes::Value(value.clone());
        }

        let nodes = self.find_node(bucket, n);
        ValueOrNodes::Nodes(nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    type MyDht = KademliaDht<16, 1, 2, 0>;

    fn extend_to_node_id<const N: usize>(arr: [u8; N]) -> NodeId {
        let mut out = [0u8; 20];
        out[..arr.len()].copy_from_slice(&arr);
        NodeId::from(out)
    }

    #[test]
    fn find_node() {
        let buckets = [
            [0, 0b00000000],
            [0, 0b10000001],
            [0, 0b11000011],
            [0, 0b11100111],
            [0, 0b11111111],
            [0, 0b11110111],
            [0, 0b11000111],
            [0, 0b00000111],
            [0, 0b0000001],
        ];
        let node_ids = buckets
            .into_iter()
            .map(extend_to_node_id)
            .collect::<Vec<_>>();
        let dht = MyDht::new(node_ids);
        let closest = dht.find_node(&Uint::from_be_bytes(buckets[4]), 3);
        assert_eq!(closest.len(), 3);
        assert_eq!(
            closest.into_iter().collect::<HashSet<_>>(),
            HashSet::from([
                extend_to_node_id(buckets[3]),
                extend_to_node_id(buckets[4]),
                extend_to_node_id(buckets[5]),
            ])
        );
    }

    #[test]
    fn find_value() {
        let mut dht = MyDht::new(vec![]);
        let key = [5, 6];
        let value = vec![1, 2, 3, 4];

        let uint_key = Uint::from_be_bytes(key);
        assert!(matches!(
            dht.find_value(&uint_key, 0),
            ValueOrNodes::Nodes(_)
        ));
        dht.store(uint_key, value.clone());
        assert_eq!(dht.find_value(&uint_key, 0), ValueOrNodes::Value(value));
    }
}
