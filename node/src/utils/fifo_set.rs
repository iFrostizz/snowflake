#![allow(unused)]

use crate::utils::FIFO;
use std::{collections::HashSet, hash::Hash};

#[derive(Debug)]
pub struct FIFOSet<T> {
    set: HashSet<T>,
    cache: FIFO<T>,
}

impl<T> FIFOSet<T>
where
    T: Eq + Hash + Clone,
{
    pub fn new(size: usize) -> Self {
        Self {
            set: HashSet::new(),
            cache: FIFO::new(size),
        }
    }

    // TODO adapt this method to only insert in the cache if the set doesn't contain this value.
    //  If it already contains it, then we should make this value the last one.
    pub fn insert(&mut self, element: T) -> Option<T> {
        let res = self.cache.push(element.clone());

        if let Some(ref el) = res {
            self.set.remove(el);
        }

        self.set.insert(element);

        res
    }

    pub fn contains(&self, element: &T) -> bool {
        self.set.contains(element)
    }
}
