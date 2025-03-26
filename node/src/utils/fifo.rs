use std::collections::VecDeque;

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug)]
pub struct FIFO<T> {
    size: usize,
    buffer: VecDeque<T>,
}

impl<T> FIFO<T> {
    /// Create a new FIFO cache
    /// # Panics
    ///
    /// Panics of the size is 0
    pub(crate) fn new(size: usize) -> Self {
        assert!(size != 0, "size cannot be null");

        Self {
            size,
            buffer: VecDeque::with_capacity(size),
        }
    }

    /// Add an element to the cache.
    /// Returns Some(T) with the oldest element if the cache is full.
    pub(crate) fn push(&mut self, element: T) -> Option<T> {
        let popped = if self.buffer.len() == self.size {
            self.buffer.pop_front()
        } else {
            None
        };

        self.buffer.push_back(element);

        popped
    }

    pub(crate) fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns a maximum of the last `max` elements in the cache.
    pub(crate) fn last_elements(&self, max: usize) -> impl Iterator<Item = &T> {
        self.buffer.iter().rev().take(max)
    }
}
