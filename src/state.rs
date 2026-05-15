//! State management: per-user pending media buffer.
//!
//! Album-NG operates in private chats only — we key buffers by `UserId`.
//! Each buffer stores a sequence of media items in the order they arrived.
//! `take` drains the buffer (the only mutation a "send" performs), so the
//! memory is released as soon as we hand the list to the dispatcher.

use std::collections::HashMap;
use std::sync::Arc;

use teloxide::types::UserId;
use tokio::sync::Mutex;

/// A single piece of media the user wants in their album.
///
/// We only persist the `file_id`. Telegram resolves it server-side when we
/// re-send via `InputMedia*`, so there is no need to download anything.
///
/// Animations (GIFs) are deliberately not supported — `sendMediaGroup` won't
/// accept an animation file_id under any `InputMedia*` variant, and we don't
/// want to download+reupload, so the bot rejects them at intake.
#[derive(Debug, Clone)]
pub enum PendingMedia {
    Photo(String),
    Video(String),
    Document(String),
    Audio(String),
}

/// Thread-safe per-user buffer of pending media items.
#[derive(Debug, Default, Clone)]
pub struct MediaStore {
    inner: Arc<Mutex<HashMap<UserId, Vec<PendingMedia>>>>,
}

impl MediaStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one media item to the user's buffer.
    pub async fn push(&self, user: UserId, item: PendingMedia) {
        let mut guard = self.inner.lock().await;
        guard.entry(user).or_default().push(item);
    }

    /// Number of items currently queued for the user.
    pub async fn len(&self, user: UserId) -> usize {
        let guard = self.inner.lock().await;
        guard.get(&user).map(Vec::len).unwrap_or(0)
    }

    /// Drain the user's buffer, returning everything that was queued and
    /// freeing the HashMap entry. This is the canonical way to reset state
    /// for a user — no further bookkeeping needed.
    pub async fn take(&self, user: UserId) -> Vec<PendingMedia> {
        let mut guard = self.inner.lock().await;
        guard.remove(&user).unwrap_or_default()
    }

    /// Forget a user's buffer without returning anything (used by /cancel).
    pub async fn clear(&self, user: UserId) -> usize {
        let mut guard = self.inner.lock().await;
        guard.remove(&user).map(|v| v.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u64) -> UserId {
        UserId(n)
    }

    #[tokio::test]
    async fn push_and_len_track_per_user() {
        let store = MediaStore::new();
        store.push(uid(1), PendingMedia::Photo("p1".into())).await;
        store.push(uid(1), PendingMedia::Video("v1".into())).await;
        store.push(uid(2), PendingMedia::Photo("p2".into())).await;

        assert_eq!(store.len(uid(1)).await, 2);
        assert_eq!(store.len(uid(2)).await, 1);
        assert_eq!(store.len(uid(3)).await, 0);
    }

    #[tokio::test]
    async fn take_drains_and_frees() {
        let store = MediaStore::new();
        store.push(uid(1), PendingMedia::Photo("p1".into())).await;
        store
            .push(uid(1), PendingMedia::Document("d1".into()))
            .await;

        let drained = store.take(uid(1)).await;
        assert_eq!(drained.len(), 2);
        assert_eq!(store.len(uid(1)).await, 0);

        // Second take is empty — entry was removed.
        assert!(store.take(uid(1)).await.is_empty());
    }

    #[tokio::test]
    async fn clear_reports_count() {
        let store = MediaStore::new();
        store.push(uid(7), PendingMedia::Audio("a1".into())).await;
        store.push(uid(7), PendingMedia::Audio("a2".into())).await;

        assert_eq!(store.clear(uid(7)).await, 2);
        assert_eq!(store.clear(uid(7)).await, 0);
    }
}
