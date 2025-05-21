#![allow(unused)]

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Debug, Clone, Copy)]
pub enum CompositeKey<K1, K2> {
    First(K1),
    Second(K2),
    Both(K1, K2),
}

#[derive(Debug)]
pub struct DoubleKeyedHashMap<K1, K2, V> {
    map1: HashMap<K1, usize>,
    map2: HashMap<K2, usize>,
    arr: Vec<V>,
}

impl<K1, K2, V> DoubleKeyedHashMap<K1, K2, V>
where
    K1: Eq + Hash,
    K2: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            map1: HashMap::new(),
            map2: HashMap::new(),
            arr: Vec::new(),
        }
    }

    pub fn insert(&mut self, key1: K1, key2: K2, value: V) {
        let i = self.arr.len();
        self.arr.push(value);
        self.map1.insert(key1, i);
        self.map2.insert(key2, i);
    }

    pub fn get1(&self, key: &K1) -> Option<&V> {
        self.map1.get(key).map(|i| self.arr.get(*i).unwrap())
    }

    pub fn get2(&self, key: &K2) -> Option<&V> {
        self.map2.get(key).map(|i| self.arr.get(*i).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push() {
        let mut map = DoubleKeyedHashMap::new();
        map.insert("hello", 1u32, "world");
        assert_eq!(map.get1(&"hello"), Some(&"world"));
        assert_eq!(map.get2(&1), Some(&"world"));
        assert!(map.get1(&"aaa").is_none());
        assert!(map.get2(&2).is_none());
    }
}
