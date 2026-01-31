//! # TUI State Management
//!
//! State management for watched channels, messages, and cached stats.

mod channels;
mod stats_cache;
mod users;
mod errors;

pub use channels::ChannelState;
pub use stats_cache::StatsCache;
pub use users::UsersState;
pub use errors::ErrorsState;
