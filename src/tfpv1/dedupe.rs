use std::collections::HashMap;

use time::OffsetDateTime;

#[derive(Debug, Default)]
pub struct DedupeCache {
    entries: HashMap<String, OffsetDateTime>,
}

impl DedupeCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check_and_insert(
        &mut self,
        message_id: &str,
        expires_at: OffsetDateTime,
    ) -> DedupeResult {
        self.cleanup_expired(OffsetDateTime::now_utc());

        if self.entries.contains_key(message_id) {
            return DedupeResult::Duplicate;
        }

        self.entries.insert(message_id.to_string(), expires_at);
        DedupeResult::Inserted
    }

    pub fn cleanup_expired_now(&mut self) {
        self.cleanup_expired(OffsetDateTime::now_utc());
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn cleanup_expired(&mut self, now: OffsetDateTime) {
        self.entries.retain(|_, expires_at| *expires_at > now);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupeResult {
    Inserted,
    Duplicate,
}

pub fn within_replay_window(
    message_ts: OffsetDateTime,
    now: OffsetDateTime,
    allowed_skew_seconds: i64,
) -> bool {
    let skew = allowed_skew_seconds.max(0);
    let delta = (now - message_ts).whole_seconds().abs();
    delta <= skew
}

#[cfg(test)]
mod tests {
    use time::Duration;

    use super::{within_replay_window, DedupeCache, DedupeResult};

    #[test]
    fn dedupe_rejects_duplicate_message_ids() {
        let mut cache = DedupeCache::new();
        let now = time::OffsetDateTime::now_utc();

        let first = cache.check_and_insert("msg_1", now + Duration::seconds(60));
        let second = cache.check_and_insert("msg_1", now + Duration::seconds(60));

        assert_eq!(first, DedupeResult::Inserted);
        assert_eq!(second, DedupeResult::Duplicate);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn dedupe_cleans_expired_entries() {
        let mut cache = DedupeCache::new();
        let now = time::OffsetDateTime::now_utc();

        cache.check_and_insert("expired", now - Duration::seconds(1));
        cache.check_and_insert("fresh", now + Duration::seconds(5));
        cache.cleanup_expired_now();

        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache.check_and_insert("expired", now + Duration::seconds(10)),
            DedupeResult::Inserted
        );
    }

    #[test]
    fn replay_window_validation_works() {
        let now = time::OffsetDateTime::now_utc();

        assert!(within_replay_window(now - Duration::seconds(30), now, 60));
        assert!(within_replay_window(now + Duration::seconds(30), now, 60));
        assert!(!within_replay_window(now - Duration::seconds(61), now, 60));
        assert!(!within_replay_window(now + Duration::seconds(61), now, 60));
    }
}
