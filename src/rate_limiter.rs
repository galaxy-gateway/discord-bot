//! # Feature: Rate Limiting
//!
//! Prevents spam with configurable request limits per bot-user pair. Uses sliding window
//! algorithm with DashMap for thread-safe concurrent access. Multi-bot aware: users
//! interacting with different bots have independent rate limits.
//!
//! - **Version**: 1.1.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.1.0: Multi-bot support with (bot_id, user_id) composite keys
//! - 1.0.0: Initial release with per-user sliding window rate limiting

use dashmap::DashMap;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Composite key for rate limiting: (bot_id, user_id)
/// This ensures users have independent rate limits per bot in multi-bot deployments.
type RateLimitKey = (String, String);

#[derive(Clone)]
pub struct RateLimiter {
    requests: DashMap<RateLimitKey, Vec<Instant>>,
    max_requests: usize,
    time_window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: usize, time_window: Duration) -> Self {
        RateLimiter {
            requests: DashMap::new(),
            max_requests,
            time_window,
        }
    }

    /// Create a composite key from bot_id and user_id
    fn make_key(bot_id: &str, user_id: &str) -> RateLimitKey {
        (bot_id.to_string(), user_id.to_string())
    }

    /// Check rate limit for a specific bot-user pair
    pub async fn check_rate_limit(&self, bot_id: &str, user_id: &str) -> bool {
        let key = Self::make_key(bot_id, user_id);
        let now = Instant::now();
        let mut entry = self.requests.entry(key).or_default();

        entry.retain(|&time| now.duration_since(time) < self.time_window);

        if entry.len() >= self.max_requests {
            false
        } else {
            entry.push(now);
            true
        }
    }

    /// Wait for rate limit to clear, then check again
    pub async fn wait_for_rate_limit(&self, bot_id: &str, user_id: &str) -> bool {
        if self.check_rate_limit(bot_id, user_id).await {
            return true;
        }

        let key = Self::make_key(bot_id, user_id);
        if let Some(entry) = self.requests.get(&key) {
            if let Some(&oldest_request) = entry.first() {
                let wait_time = self.time_window - oldest_request.elapsed();
                if wait_time > Duration::ZERO {
                    sleep(wait_time).await;
                    return self.check_rate_limit(bot_id, user_id).await;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    const TEST_BOT: &str = "test_bot";

    #[tokio::test]
    async fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(1));

        assert!(limiter.check_rate_limit(TEST_BOT, "user1").await);
        assert!(limiter.check_rate_limit(TEST_BOT, "user1").await);
        assert!(limiter.check_rate_limit(TEST_BOT, "user1").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(2, Duration::from_secs(1));

        assert!(limiter.check_rate_limit(TEST_BOT, "user1").await);
        assert!(limiter.check_rate_limit(TEST_BOT, "user1").await);
        assert!(!limiter.check_rate_limit(TEST_BOT, "user1").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_resets_after_window() {
        let limiter = RateLimiter::new(1, Duration::from_millis(100));

        assert!(limiter.check_rate_limit(TEST_BOT, "user1").await);
        assert!(!limiter.check_rate_limit(TEST_BOT, "user1").await);

        sleep(Duration::from_millis(150)).await;
        assert!(limiter.check_rate_limit(TEST_BOT, "user1").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_per_user() {
        let limiter = RateLimiter::new(1, Duration::from_secs(1));

        assert!(limiter.check_rate_limit(TEST_BOT, "user1").await);
        assert!(limiter.check_rate_limit(TEST_BOT, "user2").await);
        assert!(!limiter.check_rate_limit(TEST_BOT, "user1").await);
        assert!(!limiter.check_rate_limit(TEST_BOT, "user2").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_per_bot() {
        // Users should have independent rate limits per bot
        let limiter = RateLimiter::new(1, Duration::from_secs(1));

        // User1 hits limit on bot1
        assert!(limiter.check_rate_limit("bot1", "user1").await);
        assert!(!limiter.check_rate_limit("bot1", "user1").await);

        // Same user should still be allowed on bot2
        assert!(limiter.check_rate_limit("bot2", "user1").await);
        assert!(!limiter.check_rate_limit("bot2", "user1").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_bot_isolation() {
        // Verify complete isolation between bot-user pairs
        let limiter = RateLimiter::new(2, Duration::from_secs(1));

        // Fill up limit for bot1/user1
        assert!(limiter.check_rate_limit("bot1", "user1").await);
        assert!(limiter.check_rate_limit("bot1", "user1").await);
        assert!(!limiter.check_rate_limit("bot1", "user1").await); // blocked

        // bot2/user1 should be independent
        assert!(limiter.check_rate_limit("bot2", "user1").await);
        assert!(limiter.check_rate_limit("bot2", "user1").await);
        assert!(!limiter.check_rate_limit("bot2", "user1").await); // blocked

        // bot1/user2 should also be independent
        assert!(limiter.check_rate_limit("bot1", "user2").await);
        assert!(limiter.check_rate_limit("bot1", "user2").await);
        assert!(!limiter.check_rate_limit("bot1", "user2").await); // blocked
    }
}