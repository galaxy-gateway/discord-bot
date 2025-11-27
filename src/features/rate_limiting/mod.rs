//! # Rate Limiting Feature
//!
//! Prevents spam with configurable request limits per user.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false

pub mod limiter;

pub use limiter::RateLimiter;
