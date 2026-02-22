//! Short-lived deduplication to avoid processing the same update twice
//! (e.g. webhook retry or duplicate delivery).

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Deduplication for /pidorscan: skip if the same (chat_id, message_id) was seen recently.
pub struct PidorscanDedup {
    inner: Mutex<HashMap<(i64, i32), Instant>>,
    ttl: Duration,
}

impl PidorscanDedup {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Returns true if this (chat_id, message_id) should be processed (first time or expired).
    /// Returns false if duplicate (already processed within TTL).
    pub async fn try_acquire(&self, chat_id: i64, message_id: i32) -> bool {
        let now = Instant::now();
        let mut g = self.inner.lock().await;
        g.retain(|_, t| now.duration_since(*t) < self.ttl);
        if g.contains_key(&(chat_id, message_id)) {
            return false;
        }
        g.insert((chat_id, message_id), now);
        true
    }
}
