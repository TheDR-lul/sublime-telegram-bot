use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

pub struct RateLimiter {
    inner: Mutex<HashMap<(i64, i64), Instant>>,
    cooldown: Duration,
}

impl RateLimiter {
    pub fn new(cooldown_secs: u64) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            cooldown: Duration::from_secs(cooldown_secs),
        }
    }

    /// Returns true if the action is allowed (first time or cooldown passed).
    /// (chat_id, user_id) is used as the key; user_id may be 0 when not available.
    pub async fn check(&self, chat_id: i64, user_id: i64) -> bool {
        let now = Instant::now();
        let key = (chat_id, user_id);
        let mut guard = self.inner.lock().await;
        match guard.get(&key) {
            Some(&last) if now.duration_since(last) < self.cooldown => false,
            _ => {
                if guard.len() > 10000 {
                    guard.retain(|_, &mut last| now.duration_since(last) < self.cooldown * 2);
                }
                guard.insert(key, now);
                true
            }
        }
    }
}

