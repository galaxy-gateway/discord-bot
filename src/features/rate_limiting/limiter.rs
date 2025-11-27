//! # Feature: Rate Limiting
//!
//! Prevents spam with configurable request limits per user. Uses sliding window
//! algorithm with DashMap for thread-safe concurrent access.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.0.0: Initial release with per-user sliding window rate limiting

use dashmap::DashMap;
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[derive(Clone)]
pub struct RateLimiter {
    requests: DashMap<String, Vec<Instant>>,
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

    pub async fn check_rate_limit(&self, user_id: &str) -> bool {
        let now = Instant::now();
        let mut entry = self.requests.entry(user_id.to_string()).or_default();
        
        entry.retain(|&time| now.duration_since(time) < self.time_window);
        
        if entry.len() >= self.max_requests {
            false
        } else {
            entry.push(now);
            true
        }
    }

    pub async fn wait_for_rate_limit(&self, user_id: &str) -> bool {
        if self.check_rate_limit(user_id).await {
            return true;
        }

        if let Some(entry) = self.requests.get(user_id) {
            if let Some(&oldest_request) = entry.first() {
                let wait_time = self.time_window - oldest_request.elapsed();
                if wait_time > Duration::ZERO {
                    sleep(wait_time).await;
                    return self.check_rate_limit(user_id).await;
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

    #[tokio::test]
    async fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(1));
        
        assert!(limiter.check_rate_limit("user1").await);
        assert!(limiter.check_rate_limit("user1").await);
        assert!(limiter.check_rate_limit("user1").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(2, Duration::from_secs(1));
        
        assert!(limiter.check_rate_limit("user1").await);
        assert!(limiter.check_rate_limit("user1").await);
        assert!(!limiter.check_rate_limit("user1").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_resets_after_window() {
        let limiter = RateLimiter::new(1, Duration::from_millis(100));
        
        assert!(limiter.check_rate_limit("user1").await);
        assert!(!limiter.check_rate_limit("user1").await);
        
        sleep(Duration::from_millis(150)).await;
        assert!(limiter.check_rate_limit("user1").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_per_user() {
        let limiter = RateLimiter::new(1, Duration::from_secs(1));
        
        assert!(limiter.check_rate_limit("user1").await);
        assert!(limiter.check_rate_limit("user2").await);
        assert!(!limiter.check_rate_limit("user1").await);
        assert!(!limiter.check_rate_limit("user2").await);
    }
}