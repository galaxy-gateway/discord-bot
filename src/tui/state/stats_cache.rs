//! # Stats Cache
//!
//! Cached statistics from the database.

use std::time::Instant;
use std::collections::HashMap;
use crate::ipc::TopUser;

/// Cached usage statistics
#[derive(Debug, Clone, Default)]
pub struct UsageStats {
    /// Total API cost
    pub total_cost: f64,
    /// Today's cost
    pub today_cost: f64,
    /// Total tokens used
    pub total_tokens: u64,
    /// Total API calls
    pub total_calls: u64,
    /// Cost by service (e.g., "chat", "image", "whisper")
    pub cost_by_service: Vec<(String, f64)>,
    /// Daily breakdown (date string, cost)
    pub daily_breakdown: Vec<(String, f64)>,
    /// Top users by cost with optional username
    pub top_users: Vec<TopUser>,
}

/// System metrics
#[derive(Debug, Clone, Default)]
pub struct SystemMetrics {
    /// CPU usage percentage
    pub cpu_percent: f32,
    /// Memory usage in bytes
    pub memory_bytes: u64,
    /// Memory total in bytes
    pub memory_total: u64,
    /// Database file size in bytes
    pub db_size: u64,
    /// Bot uptime in seconds
    pub uptime_seconds: u64,
}

/// Historical metrics for sparklines
#[derive(Debug, Clone, Default)]
pub struct HistoricalMetrics {
    /// CPU history (timestamp, value)
    pub cpu_history: Vec<(i64, f64)>,
    /// Memory history (timestamp, value)
    pub memory_history: Vec<(i64, f64)>,
    /// Cost history by date (date string, cost)
    pub cost_history: Vec<(String, f64)>,
}

/// Stats cache with refresh tracking
pub struct StatsCache {
    /// Usage statistics
    pub usage: UsageStats,
    /// System metrics
    pub system: SystemMetrics,
    /// Historical metrics for charts
    pub historical: HistoricalMetrics,
    /// Last refresh time
    pub last_refresh: Option<Instant>,
    /// Refresh interval in seconds
    pub refresh_interval: u64,
    /// Whether a refresh is in progress
    pub refreshing: bool,
    /// Selected time period for stats
    pub time_period: TimePeriod,
}

/// Time period for stats filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimePeriod {
    Today,
    Week,
    Month,
    AllTime,
}

impl TimePeriod {
    pub fn label(&self) -> &'static str {
        match self {
            TimePeriod::Today => "Today",
            TimePeriod::Week => "This Week",
            TimePeriod::Month => "This Month",
            TimePeriod::AllTime => "All Time",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            TimePeriod::Today => TimePeriod::Week,
            TimePeriod::Week => TimePeriod::Month,
            TimePeriod::Month => TimePeriod::AllTime,
            TimePeriod::AllTime => TimePeriod::Today,
        }
    }

    pub fn days(&self) -> Option<u32> {
        match self {
            TimePeriod::Today => Some(1),
            TimePeriod::Week => Some(7),
            TimePeriod::Month => Some(30),
            TimePeriod::AllTime => None,
        }
    }
}

impl StatsCache {
    pub fn new() -> Self {
        StatsCache {
            usage: UsageStats::default(),
            system: SystemMetrics::default(),
            historical: HistoricalMetrics::default(),
            last_refresh: None,
            refresh_interval: 30, // Default 30 seconds
            refreshing: false,
            time_period: TimePeriod::Week,
        }
    }

    /// Check if cache needs refresh
    pub fn needs_refresh(&self) -> bool {
        match self.last_refresh {
            None => true,
            Some(t) => t.elapsed().as_secs() >= self.refresh_interval,
        }
    }

    /// Mark refresh as started
    pub fn start_refresh(&mut self) {
        self.refreshing = true;
    }

    /// Mark refresh as complete
    pub fn complete_refresh(&mut self) {
        self.refreshing = false;
        self.last_refresh = Some(Instant::now());
    }

    /// Update usage stats
    pub fn update_usage(&mut self, stats: UsageStats) {
        self.usage = stats;
    }

    /// Update system metrics
    pub fn update_system(&mut self, metrics: SystemMetrics) {
        self.system = metrics;
    }

    /// Cycle to next time period
    pub fn cycle_time_period(&mut self) {
        self.time_period = self.time_period.next();
        self.last_refresh = None; // Force refresh on period change
    }

    /// Get memory usage as percentage
    pub fn memory_percent(&self) -> f32 {
        if self.system.memory_total > 0 {
            (self.system.memory_bytes as f32 / self.system.memory_total as f32) * 100.0
        } else {
            0.0
        }
    }

    /// Format uptime as human-readable string
    pub fn format_uptime(&self) -> String {
        let secs = self.system.uptime_seconds;
        let days = secs / 86400;
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;

        if days > 0 {
            format!("{}d {}h {}m", days, hours, mins)
        } else if hours > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}m", mins)
        }
    }

    /// Set historical data from IPC response
    pub fn set_historical_data(&mut self, metric_type: &str, data_points: Vec<(i64, f64)>) {
        match metric_type {
            "cpu" => self.historical.cpu_history = data_points,
            "memory" => self.historical.memory_history = data_points,
            _ => {}
        }
    }

    /// Get CPU history as sparkline data (values only)
    pub fn cpu_sparkline_data(&self) -> Vec<u64> {
        self.historical.cpu_history.iter()
            .map(|(_, v)| *v as u64)
            .collect()
    }

    /// Get memory history as sparkline data (percentage)
    pub fn memory_sparkline_data(&self) -> Vec<u64> {
        let total = self.system.memory_total as f64;
        if total > 0.0 {
            self.historical.memory_history.iter()
                .map(|(_, v)| ((v / total) * 100.0) as u64)
                .collect()
        } else {
            self.historical.memory_history.iter()
                .map(|(_, v)| *v as u64)
                .collect()
        }
    }

    /// Get cost history as sparkline data
    pub fn cost_sparkline_data(&self) -> Vec<u64> {
        // Scale costs to visible range (multiply by 10000 for small values)
        self.usage.daily_breakdown.iter()
            .map(|(_, cost)| (cost * 10000.0) as u64)
            .collect()
    }
}

impl Default for StatsCache {
    fn default() -> Self {
        Self::new()
    }
}
