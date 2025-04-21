use crate::dht::Bucket;
use crate::id::NodeId;
use prost::Message;
use proto_lib::sdk;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::RwLock;

pub trait LockedMapDb<K, V> {
    fn insert(&self, key: K, value: V) -> Option<V>;
    fn get(&self, key: &K) -> Option<V>;
}

impl<K, V> LockedMapDb<K, V> for RwLock<HashMap<K, V>>
where
    K: Eq + Hash,
    V: Clone,
{
    fn insert(&self, key: K, value: V) -> Option<V> {
        self.write().unwrap().insert(key, value)
    }

    fn get(&self, key: &K) -> Option<V> {
        self.read().unwrap().get(key).cloned()
    }
}

#[derive(Debug, Default)]
pub struct KademliaDht<V, DB: LockedMapDb<Bucket, V>> {
    store: DB,
    nodes: Vec<NodeId>,
    _marker: std::marker::PhantomData<V>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ValueOrNodes<V> {
    Value(V),
    Nodes(Vec<NodeId>),
}

impl ValueOrNodes<Vec<u8>> {
    pub fn encode(self, buf: &mut Vec<u8>) -> Result<(), prost::EncodeError> {
        match self {
            ValueOrNodes::Value(value) => sdk::Value::encode(&sdk::Value { value }, buf),

            ValueOrNodes::Nodes(nodes) => sdk::Nodes::encode(
                &sdk::Nodes {
                    node_ids: nodes
                        .into_iter()
                        .map(|node_id| node_id.as_ref().to_vec())
                        .collect(),
                },
                buf,
            ),
        }
    }
}

impl<V, DB: LockedMapDb<Bucket, V>> KademliaDht<V, DB> {
    pub fn new(nodes: Vec<NodeId>, store: DB) -> Self {
        Self {
            store,
            nodes,
            _marker: Default::default(),
        }
    }

    fn distance(a: &Bucket, b: Bucket) -> Bucket {
        a ^ b
    }

    pub fn store(&self, key: Bucket, value: V) -> Option<V> {
        self.store.insert(key, value)
    }

    pub fn get(&self, key: &Bucket) -> Option<V> {
        self.store.get(key)
    }

    /// Find up to `n` unique nodes that are the closest to the `bucket`.
    pub fn find_node(&self, bucket: &Bucket) -> Vec<NodeId> {
        let mut n = 8; // TODO param

        if self.nodes.is_empty() {
            return vec![];
        }

        if n >= self.nodes.len() {
            n = self.nodes.len() - 1;
        }

        let distances: Vec<Bucket> = self
            .nodes
            .iter()
            .map(|node_id| Self::distance(bucket, Bucket::from_be_bytes((*node_id).into())))
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

    /// Find unique nodes that are the closest to the `bucket` or return the value
    /// if it is in the store.
    pub fn find_value(&self, bucket: &Bucket) -> ValueOrNodes<V> {
        if let Some(value) = self.store.get(bucket) {
            return ValueOrNodes::Value(value);
        }

        let nodes = self.find_node(bucket);
        ValueOrNodes::Nodes(nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn extend_to_bytes<const N: usize>(arr: [u8; N]) -> [u8; 20] {
        let mut out = [0u8; 20];
        out[..arr.len()].copy_from_slice(&arr);
        out
    }

    fn extend_to_bucket<const N: usize>(arr: [u8; N]) -> Bucket {
        Bucket::from_be_bytes(extend_to_bytes(arr))
    }

    fn extend_to_node_id<const N: usize>(arr: [u8; N]) -> NodeId {
        NodeId::from(extend_to_bytes(arr))
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
        let dht: KademliaDht<(), RwLock<HashMap<Bucket, ()>>> =
            KademliaDht::new(node_ids, RwLock::new(HashMap::new()));
        let closest = dht.find_node(&extend_to_bucket(buckets[4]), 3);
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
        let dht = KademliaDht::new(vec![], RwLock::new(HashMap::new()));
        let key = [5, 6];
        let value = vec![1, 2, 3, 4];

        let uint_key = extend_to_bucket(key);
        assert!(matches!(
            dht.find_value(&uint_key, 0),
            ValueOrNodes::Nodes(_)
        ));
        dht.store(uint_key, value.clone());
        assert_eq!(dht.find_value(&uint_key, 0), ValueOrNodes::Value(value));
    }
}
