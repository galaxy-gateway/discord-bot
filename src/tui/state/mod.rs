//! # TUI State Management
//!
//! State management for watched channels, messages, and cached stats.

mod channels;
mod stats_cache;

pub use channels::ChannelState;
pub use stats_cache::StatsCache;
