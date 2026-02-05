//! # Analytics Feature
//!
//! Usage tracking, interaction analytics, and system metrics.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.5.0
//! - **Toggleable**: false

pub mod interaction_tracker;
pub mod system_info;
pub mod usage_tracker;

pub use interaction_tracker::InteractionTracker;
pub use system_info::{
    format_bytes, format_bytes_signed, format_duration, format_history, get_db_file_size,
    metrics_collection_loop, CurrentMetrics, DiskInfo, HistoricalSummary,
};
pub use usage_tracker::{CostBucket, UsageTracker};
