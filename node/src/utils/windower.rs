#![allow(unused)]

use crate::id::{ChainId, NodeId};
use rand_mt::Mt64;

pub struct ValidatorData<'a> {
    pub id: &'a NodeId,
    pub weight: &'a u64,
}

pub struct Windower<'a> {
    /// Canonically sorted validator set
    validators: &'a Vec<ValidatorData<'a>>,
    cumulative_weights: Vec<u64>,
    total_weight: u64,
    chain_source: u64,
}

impl<'a> Windower<'a> {
    /// The maximum amount of slots in a window
    pub const MAX_SLOT: u64 = 6;

    pub fn new(validators: &'a Vec<ValidatorData>, chain_id: &ChainId) -> Self {
        let weights: Vec<_> = validators.iter().map(|data| *data.weight).collect();

        let total_weight = weights.iter().sum();
        if total_weight == 0 {
            panic!("cannot have null weight");
        }

        // NOTE in practice we can optimize such that the biggest weights comes first,
        // but then it complexifies the algorithm for a marginal performance change
        // because we would also need to keep track of the index.
        let mut cumulative_weights = weights;
        cumulative_weights.iter_mut().fold(0, |acc, weight| {
            *weight += acc;
            *weight
        });

        let chain_source_bytes = &chain_id.as_ref()[0..8];
        let chain_source = u64::from_be_bytes(chain_source_bytes.try_into().unwrap());

        Self {
            validators,
            cumulative_weights,
            total_weight,
            chain_source,
        }
    }

    pub fn proposer_at(&self, block_height: u64, slot: u64) -> &NodeId {
        let seed = self.chain_source ^ block_height ^ slot.reverse_bits();

        let mut rng = UniformRng::new(seed);
        let drawn = rng.u64_inclusive(self.total_weight - 1);
        let index = match self.cumulative_weights.binary_search(&drawn) {
            Ok(i) | Err(i) => i,
        };

        self.validators.get(index).expect("fuck :(").id
    }
}

struct UniformRng {
    mt_64: Mt64,
}

impl UniformRng {
    fn new(seed: u64) -> Self {
        Self {
            mt_64: Mt64::new(seed),
        }
    }

    fn u64_inclusive(&mut self, n: u64) -> u64 {
        if n & (n + 1) == 0 {
            self.mt_64.next_u64()
        } else if n > i64::MAX as u64 {
            loop {
                let out = self.mt_64.next_u64();
                if out <= n {
                    break out;
                }
            }
        } else {
            let max = (1 << 63) - 1 - (1 << 63) % (n + 1);
            let out = loop {
                let out = self.u63();
                if out <= max {
                    break out;
                }
            };
            out % (n + 1)
        }
    }

    fn u63(&mut self) -> u64 {
        self.mt_64.next_u64() & i64::MAX as u64
    }
}

#[cfg(test)]
mod tests {
    use super::{ValidatorData, Windower};
    use crate::id::{NodeId, MAINNET_C_CHAIN_ID};

    #[test]
    fn expected_proposer() {
        let node_id1 = NodeId::try_from("NodeID-A6onFGyJjA37EZ7kYHANMR1PFRT8NmXrF").unwrap();
        let node_id2 = NodeId::try_from("NodeID-6SwnPJLH8cWfrJ162JjZekbmzaFpjPcf").unwrap();
        let node_id3 = NodeId::try_from("NodeID-GSgaA47umS1px2ohVjodW9621Ks63xDxD").unwrap();

        #[allow(clippy::inconsistent_digit_grouping)]
        let validators = vec![
            ValidatorData {
                id: &node_id2,
                weight: &2_000_00000000,
            },
            ValidatorData {
                id: &node_id1,
                weight: &2_000_00000000,
            },
            ValidatorData {
                id: &node_id3,
                weight: &2_000_00000000,
            },
        ];

        let window = Windower::new(&validators, &MAINNET_C_CHAIN_ID);
        assert_eq!(window.proposer_at(0, 0), &node_id2);
        assert_eq!(window.proposer_at(1, 0), &node_id1);
        assert_eq!(window.proposer_at(1, 1), &node_id3);
    }
}
