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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;

#[derive(Debug)]
pub struct BucketMessage {
    message: WriteMessage,
    handler: WriteHandler,
}

#[derive(Debug)]
pub struct Pipeline {
    /// Maximum number of tokens that can accumulate
    bucket_size: usize,

    /// Current available tokens
    tokens: AtomicUsize,

    bucket_tx: Sender<BucketMessage>,
    bucket_rx: Receiver<BucketMessage>,
}

impl Pipeline {
    pub fn new(bucket_size: usize) -> Self {
        let (bucket_tx, bucket_rx) = flume::bounded(100);

        Self {
            bucket_size,
            tokens: AtomicUsize::new(bucket_size),
            bucket_tx,
            bucket_rx,
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
        if size > self.bucket_size {
            log::error!("dropping too big message. size: {}, bucket_size: {}", size, self.bucket_size);
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

        if self.try_take_tokens(size) {
            let _ = handler.handle_message(message).await;
        } else {
            if let Err(_) = self.bucket_tx.try_send(BucketMessage { message, handler }) {
                log::error!("dropping message: queue full");
            }
        }
    }

    /// Attempts to execute as much messages in the queue as possible.
    async fn try_exec_messages(&self) {
        while let Ok(BucketMessage { message, handler }) = self.bucket_rx.try_recv() {
            let len = message.size();

            if !self.try_take_tokens(len) {
                // put it back if not enough tokens
                let _ = self.bucket_tx.try_send(BucketMessage { message, handler });
                break;
            }

            let _ = handler.handle_message(message).await;
        }
    }
}
