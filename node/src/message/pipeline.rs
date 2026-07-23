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
use flume::{Receiver, Sender};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

#[derive(Debug)]
pub struct BucketMessage {
    message: WriteMessage,
    handler: WriteHandler,
}

#[derive(Debug)]
pub struct Pipeline {
    /// Maximum bucket size (burst capacity in bytes)
    burst_size: u64,

    /// Refill rate in bytes per second
    rate: u64,

    /// Current available tokens (bytes)
    tokens: AtomicU64,

    /// Last refill timestamp
    last_refill: parking_lot::Mutex<Instant>,

    bucket_tx: Sender<BucketMessage>,
    bucket_rx: Receiver<BucketMessage>,
}

impl Pipeline {
    /// Creates a new Pipeline.
    /// - `rate`: The sustained rate limit in bytes per second (default 5MB/s).
    /// - `burst_size`: Optional burst size in bytes; defaults to `rate` (allowing a 1-second burst).
    pub fn new(rate: u64, burst_size: Option<u64>) -> Self {
        let burst_size = burst_size.unwrap_or(rate);
        let (bucket_tx, bucket_rx) = flume::bounded(100);

        Self {
            burst_size,
            rate,
            tokens: AtomicU64::new(burst_size),
            last_refill: parking_lot::Mutex::new(Instant::now()),
            bucket_tx,
            bucket_rx,
        }
    }

    /// Starts the background task to periodically process queued messages.
    pub async fn start(&self, mut rx: broadcast::Receiver<()>) {
        // let mut int = tokio::time::interval(Duration::from_millis(10)); // Adjusted to 10ms for better efficiency
        // loop {
        //     tokio::select! {
        //         _ = int.tick() => {
        //             self.try_exec_messages();
        //         }
        //         _ = rx.recv() => {
        //             return;
        //         }
        //     }
        // }
    }

    /// Refills the token bucket based on elapsed time.
    fn refill(&self) {
        // let mut last = self.last_refill.lock();
        // let now = Instant::now();
        // let elapsed = now - *last;
        // let elapsed_nanos = elapsed.as_nanos();
        // let add = ((elapsed_nanos * self.rate as u128) / 1_000_000_000u128) as u64;
        //
        // if add > 0 {
        //     let current = self.tokens.fetch_add(add, Ordering::AcqRel);
        //     let new_tokens = current + add;
        //     if new_tokens > self.burst_size {
        //         self.tokens.store(self.burst_size, Ordering::Release);
        //     }
        //     *last = now;
        // }
    }

    /// Attempts to take `size` tokens from the bucket using CAS for thread safety.
    fn try_take_tokens(&self, size: u64) -> bool {
        if size > self.burst_size {
            log::error!(
                "dropping too big message. size: {}, burst_size: {}",
                size,
                self.burst_size
            );
            return false;
        }

        let tokens = &self.tokens;

        loop {
            let current = tokens.load(Ordering::Acquire);

            if current < size {
                return false;
            }

            let new = current - size;

            match tokens.compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return true,
                Err(_) => std::hint::spin_loop(),
            }
        }
    }

    /// Queue a message to be sent.
    /// It will be sent immediately if tokens are available after refilling,
    /// otherwise it will be queued for later processing.
    pub fn queue_message(&self, message: WriteMessage, handler: WriteHandler) {
        // self.refill();

        let size = message.size() as u64;

        if size > self.burst_size {
            log::error!(
                "dropping too big message. size: {}, burst_size: {}",
                size,
                self.burst_size
            );
            return;
        }

        // if self.try_take_tokens(size) {
        handler.handle_message(message);
        // } else {
        //     if let Err(_) = self.bucket_tx.try_send(BucketMessage { message, handler }) {
        //         log::error!("dropping message: queue full");
        //     }
        // }
    }

    /// Attempts to execute as many queued messages as possible after refilling tokens.
    fn try_exec_messages(&self) {
        //     // self.refill();
        //
        //     while let Ok(BucketMessage { message, handler }) = self.bucket_rx.try_recv() {
        //         let size = message.size() as u64;
        //
        //         if !self.try_take_tokens(size) {
        //             // Put it back if not enough tokens
        //             let _ = self.bucket_tx.try_send(BucketMessage { message, handler });
        //             break;
        //         }
        //
        //         handler.handle_message(message);
        //     }
    }
}
