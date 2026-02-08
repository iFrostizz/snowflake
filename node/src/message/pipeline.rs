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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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

    /// Maximum total queued bytes
    max_bytes: usize,

    /// Currently queued bytes
    current_bytes: AtomicUsize,

    /// Maximum number of tokens that can accumulate
    bucket_size: usize,

    /// Current available tokens
    tokens: AtomicUsize,

    /// Base time reference
    start: Instant,

    /// Last execution time in microseconds since `start`
    last_executed_micros: AtomicU64,

    /// Queue of pending messages
    bucket_messages: Mutex<VecDeque<BucketMessage>>,
}

impl Pipeline {
    pub fn new(max_throughput: u32, max_bytes: usize, bucket_size: usize) -> Self {
        Self {
            max_throughput,
            max_bytes,
            current_bytes: AtomicUsize::new(0),
            bucket_size,
            tokens: AtomicUsize::new(bucket_size),
            start: Instant::now(),
            last_executed_micros: AtomicU64::new(0),
            bucket_messages: Mutex::new(VecDeque::new()),
        }
    }

    pub async fn start(&self, mut rx: broadcast::Receiver<()>) {
        let mut int = tokio::time::interval(Duration::from_millis(1));
        loop {
            tokio::select! {
                _ = int.tick() => {
                    self.try_exec_messages().await;
                }
                _ = rx.recv() => {
                    return;
                }
            }
        }
    }

    fn try_take_tokens(&self, size: usize) -> bool {
        let tokens = &self.tokens;

        loop {
            let current = tokens.load(Ordering::Acquire);

            if current < size {
                return false;
            }

            let new = current - size;

            match tokens.compare_exchange(
                current,
                new,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(_) => std::hint::spin_loop(),
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

        let queued = self.current_bytes.load(Ordering::Acquire);
        if queued + size > self.max_bytes {
            log::error!(
                "dropping message. Queue {} would exceed max size {}",
                queued,
                self.max_bytes
            );
            return;
        }

        if self.try_take_tokens(size) {
            let _ = handler.handle_message(message).await;
        } else {
            self.current_bytes.fetch_add(size, Ordering::AcqRel);

            self.bucket_messages
                .lock()
                .unwrap()
                .push_back(BucketMessage { message, handler });
        }
    }

    /// Attempts to execute as much messages in the queue as possible.
    async fn try_exec_messages(&self) {
        let now_micros = self.start.elapsed().as_micros() as u64;

        let last = self.last_executed_micros.load(Ordering::Acquire);
        let dur = now_micros.saturating_sub(last) as usize;

        let refill_rate = self.max_throughput;
        let added = dur * refill_rate as usize / 1_000_000;

        let previous = self.tokens.fetch_add(added, Ordering::AcqRel);
        let refilled = previous + added;

        let mut bytes_sum = 0;
        let mut messages_to_send = Vec::new();

        {
            let mut messages = self.bucket_messages.lock().unwrap();

            for _ in 0..messages.len() {
                let BucketMessage { message, .. } = messages.front().unwrap();
                let len = message.size();

                if bytes_sum + len > refilled {
                    break;
                }

                bytes_sum += len;
                let msg = messages.pop_front().unwrap();
                messages_to_send.push(msg);
            }
        }

        if bytes_sum > 0 {
            self.current_bytes.fetch_sub(bytes_sum, Ordering::AcqRel);
        }

        let unused_tokens =
            std::cmp::min(refilled.saturating_sub(bytes_sum), self.bucket_size);

        self.tokens.store(unused_tokens, Ordering::Release);
        self.last_executed_micros.store(now_micros, Ordering::Release);

        for BucketMessage { message, handler } in messages_to_send {
            tokio::spawn(async move {
                let _ = handler.handle_message(message).await;
            });
        }
    }
}
