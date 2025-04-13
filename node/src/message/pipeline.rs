// A message pipeline that manages their volume.
// If the amount of messages is too much for the buffer, they may either
// be flushed, added to the buffer, or discarded.
// It also provides a feedback that can be used as a loop
// to regulate the message sender

// TODO adapt code to support priority with messages
// For instance, we could define priorities:
// 0: no information and need one quickly
// 1: need to answer to a registered message (AppRequest -> AppResponse / AppError)
// 2: no information, enough similar messages already sent
// 3: cosmetic messages
// We could also add a dynamic property to messages to make those sent to big stakes a priority.

use crate::net::node::{WriteHandler, WriteMessage};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::Instant;

#[derive(Debug)]
pub struct BucketMessage {
    message: WriteMessage,
    handler: WriteHandler,
}

#[derive(Debug)]
pub struct Pipeline {
    /// Max throughput in B/s
    max_throughput: u32,
    max_bytes: usize,
    current_bytes: AtomicUsize,
    bucket_size: usize,
    tokens: Mutex<usize>,
    last_executed: Mutex<Instant>,
    bucket_messages: Mutex<VecDeque<BucketMessage>>,
}

impl Pipeline {
    /// Creates a new pipeline with a max leaking rate and an initial bucket size
    /// Set `bucket_size` such that it can at least handle the biggest message.
    pub fn new(max_throughput: u32, max_bytes: usize, bucket_size: usize) -> Self {
        Self {
            max_throughput,
            max_bytes,
            current_bytes: AtomicUsize::new(0),
            bucket_size,
            tokens: Mutex::new(bucket_size), // start full
            last_executed: Mutex::new(Instant::now()),
            bucket_messages: Mutex::new(VecDeque::new()),
        }
    }

    /// Start the pipeline and look at executable messages in intervals
    pub async fn start(&self, mut rx: broadcast::Receiver<()>) {
        let mut int = tokio::time::interval(Duration::from_millis(1));
        loop {
            tokio::select! {
                _ = int.tick() => {
                    let _ = self.try_exec_messages().await;
                }
                _ = rx.recv() => {
                    return;
                }
            }
        }
    }

    /// Queue a message to the bucket.
    /// It will be executed right away if the bucket is full enough,
    /// but will be on the wait list if there aren't enough tokens in the bucket.
    /// This is achieved by using an interval that will batch any incoming messages
    /// by storing the [`Instant`] the last message was sent.
    pub async fn queue_message(&self, message: WriteMessage, handler: WriteHandler) {
        let size = message.size();
        if size > self.bucket_size {
            log::error!(
                "dropping too big message. size: {}, bucket_size: {}",
                size,
                self.bucket_size
            );
            return;
        }

        let current_bytes = self.current_bytes.load(Ordering::Relaxed);
        if current_bytes > self.max_bytes {
            log::error!(
                "dropping message. Queue {} would exceed max size {}",
                current_bytes,
                self.max_bytes
            );
            return;
        }

        let has_enough_tokens = {
            let mut tokens = self.tokens.lock().unwrap();
            if *tokens >= size {
                *tokens -= size;
                true
            } else {
                false
            }
        };

        if has_enough_tokens {
            let _ = handler.handle_message(message).await;
        } else {
            self.bucket_messages
                .lock()
                .unwrap()
                .push_back(BucketMessage { message, handler });
        }
    }

    /// Attempts to execute as much messages in the queue as possible.
    async fn try_exec_messages(&self) {
        // TODO replace last_executed by current one with an atomic by fetch
        // it might be nice to write it at the end too though because this way we are being pessimistic about the mutex or atomic ordering.
        // refilled (B) = refill_rate (B/s) * dur (s)
        let dur = Instant::now()
            .duration_since(*self.last_executed.lock().unwrap())
            .as_micros() as usize;
        let refill_rate = self.max_throughput;
        let refilled = *self.tokens.lock().unwrap() + dur * refill_rate as usize / 1_000_000;

        let mut bytes_sum = 0;
        let mut messages_to_send = Vec::new();

        let mut i = 0;
        {
            let mut messages = self.bucket_messages.lock().unwrap();
            let max = messages.len();
            while i < max {
                let BucketMessage { message, .. } = messages.front().unwrap();
                let len = message.size();
                if bytes_sum + len > refilled {
                    break;
                }

                bytes_sum += len;
                messages_to_send.push(messages.pop_front().unwrap());
                i += 1;
            }
        }

        // this is why it's important to set the bucket_size accordingly to the most huge message
        let unused_tokens = std::cmp::min(refilled - bytes_sum, self.bucket_size);
        *self.tokens.lock().unwrap() = unused_tokens;

        for BucketMessage { message, handler } in messages_to_send {
            tokio::spawn(async move {
                let _ = handler.handle_message(message).await;
            });
        }

        *self.last_executed.lock().unwrap() = Instant::now();
    }
}
