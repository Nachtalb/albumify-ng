//! Per-user ack debouncer.
//!
//! When a user dumps a batch of media at the bot, we don't want to send one
//! "Queued …" reply per item — that turns a 20-photo upload into a 20-message
//! wall of acks. Instead we debounce: each incoming media item resets a short
//! timer; when the timer fires (no new items for `DEBOUNCE`), we send a single
//! summary message with the current queue length.
//!
//! Implementation: one tokio task per user, replaced (via `abort()`) on every
//! new push. The task simply sleeps, then asks the dispatcher to send the ack.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use teloxide::prelude::*;
use teloxide::types::{ChatId, UserId};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::state::MediaStore;

/// Delay between the last queued media item and the summary ack.
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// Per-user pending ack timers.
#[derive(Debug, Default, Clone)]
pub struct AckDebouncer {
    inner: Arc<Mutex<HashMap<UserId, JoinHandle<()>>>>,
}

impl AckDebouncer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedule (or re-schedule) the summary ack for `user`.
    ///
    /// Any previously-pending ack for the same user is aborted, so a burst of
    /// rapid pushes collapses into a single message fired `DEBOUNCE` after the
    /// final one. The spawned task reads the *current* queue length at fire
    /// time, not at schedule time — so the count is always accurate.
    pub async fn schedule(&self, bot: Bot, chat: ChatId, user: UserId, store: MediaStore) {
        let inner = self.inner.clone();
        let inner_for_task = inner.clone();

        let handle = tokio::spawn(async move {
            tokio::time::sleep(DEBOUNCE).await;

            let count = store.len(user).await;
            if count > 0 {
                let text = if count == 1 {
                    "Queued 1 item. /create when ready.".to_string()
                } else {
                    format!("Queued {count} items. /create when ready.")
                };
                if let Err(err) = bot.send_message(chat, text).await {
                    tracing::warn!(?err, %user, "failed to send debounced queue ack");
                }
            }

            // Self-evict from the map so the HashMap doesn't grow forever.
            let mut guard = inner_for_task.lock().await;
            // Only remove if we're still the registered handle — a later
            // schedule() may have already replaced us, in which case the new
            // handle owns the slot.
            if let Some(h) = guard.get(&user)
                && h.is_finished()
            {
                guard.remove(&user);
            }
        });

        let mut guard = inner.lock().await;
        if let Some(prev) = guard.insert(user, handle) {
            // Cancel the previous, still-sleeping ack.
            prev.abort();
        }
    }

    /// Drop any pending ack for the user (used on /create and /cancel so we
    /// don't ack right after a real status message).
    pub async fn cancel(&self, user: UserId) {
        let mut guard = self.inner.lock().await;
        if let Some(prev) = guard.remove(&user) {
            prev.abort();
        }
    }
}
