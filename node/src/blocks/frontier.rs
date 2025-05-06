#![allow(unused)]

use super::BlockStore;
use crate::{id::BlockID, utils::FIFO};
use std::collections::HashSet;

pub struct Frontier {
    blocks: FIFO<BlockID>,
    set: HashSet<BlockID>,
}

impl Frontier {
    pub fn new(cache_size: usize) -> Self {
        Frontier {
            blocks: FIFO::new(cache_size),
            set: HashSet::new(),
        }
    }

    pub fn tip(&self, block_store: &BlockStore) -> Option<(BlockID, u64)> {
        // TODO this is not looking great. Optimize for O(1) lookup.
        let mut bids = self.blocks.last_elements(usize::MAX);

        // true for empty so it cannot return the fold default value
        if bids.all(|block_id| block_store.get(block_id).is_none()) {
            None
        } else {
            // NOTE this is needed because otherwise the iterator is exhausted
            let bids = self.blocks.last_elements(usize::MAX);
            Some(
                bids.flat_map(|block_id| {
                    block_store.get(block_id).map(|height| (block_id, height))
                })
                .fold(
                    (BlockID::default(), 0),
                    |(block_id, height), (new_block_id, new_height)| {
                        if new_height >= &height {
                            (*new_block_id, *new_height)
                        } else {
                            (block_id, height)
                        }
                    },
                ),
            )
        }
    }

    pub fn push(&mut self, block_id: BlockID) {
        if !self.has_block(&block_id) {
            if let Some(rem_block_id) = self.blocks.push(block_id) {
                self.set.remove(&rem_block_id);
            }
            self.set.insert(block_id);
        }
    }

    pub fn has_block(&self, block_id: &BlockID) -> bool {
        self.set.contains(block_id)
    }
}

#[cfg(test)]
mod tests {
    use super::Frontier;
    use crate::{blocks::BlockStore, id::BlockID};

    #[test]
    fn no_value_ret() {
        let mut front = Frontier::new(5);
        assert!(front.tip(&BlockStore::new(1)).is_none());
        let block_id = BlockID::from([1; 32]);
        front.push(block_id);
        assert!(front.tip(&BlockStore::new(1)).is_none());
        let mut store = BlockStore::new(1);
        store.push(block_id, 0);
        assert_eq!(front.tip(&store), Some((block_id, 0)));
    }
}
