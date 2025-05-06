#![allow(unused)]

use crate::{id::BlockID, utils::FIFO};
use std::collections::HashMap;

#[derive(Debug)]
pub struct BlockStore {
    cache: FIFO<BlockID>,
    store: HashMap<BlockID, u64>,
}

impl BlockStore {
    pub fn new(cache_size: usize) -> Self {
        Self {
            cache: FIFO::new(cache_size),
            store: HashMap::new(),
        }
    }

    pub fn push(&mut self, block_id: BlockID, height: u64) {
        if self.store.insert(block_id, height).is_none() {
            if let Some(block_id) = self.cache.push(block_id) {
                self.store.remove(&block_id);
            }
        }
    }

    pub fn get(&self, block_id: &BlockID) -> Option<&u64> {
        self.store.get(block_id)
    }

    pub fn contains_key(&self, block_id: &BlockID) -> bool {
        self.store.contains_key(block_id)
    }
}
