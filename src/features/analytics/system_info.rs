//! # Feature: System Information
//!
//! System diagnostics and historical metrics tracking for the /sysinfo command.
//!
//! - **Version**: 1.1.0
//! - **Since**: 0.3.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.1.0: Added OpenAI usage data cleanup integration
//! - 1.0.0: Initial implementation with current metrics and historical tracking

use crate::database::Database;
use log::{debug, info, warn};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Disks, ProcessRefreshKind, ProcessesToUpdate, System};

/// Information about a disk/mount point
pub struct DiskInfo {
    pub mount: String,
    pub total: u64,
    pub used: u64,
}

/// Current system metrics snapshot
pub struct CurrentMetrics {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel: String,
    pub architecture: String,
    pub cpu_usage: f32,
    pub cpu_cores_physical: usize,
    pub cpu_cores_logical: usize,
    pub load_avg: (f64, f64, f64),
    pub memory_total: u64,
    pub memory_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disks: Vec<DiskInfo>,
    pub bot_memory: u64,
    pub db_size: u64,
}

impl CurrentMetrics {
    /// Gather all current system metrics
    /// Note: For accurate CPU usage, caller should wait ~200ms between System refreshes
    pub fn gather(sys: &System, db_path: &str) -> Self {
        let load = System::load_average();

        // Get bot process memory
        let bot_memory = if let Ok(pid) = sysinfo::get_current_pid() {
            sys.process(pid).map(|p| p.memory()).unwrap_or(0)
        } else {
            0
        };

        // Get disk information for major mount points
        let disks_info = Disks::new_with_refreshed_list();
        let disks: Vec<DiskInfo> = disks_info
            .iter()
            .filter(|d| {
                let mount = d.mount_point().to_string_lossy();
                mount == "/" || mount.starts_with("/home") || mount.starts_with("/var")
            })
            .map(|d| {
                let total = d.total_space();
                let available = d.available_space();
                DiskInfo {
                    mount: d.mount_point().to_string_lossy().to_string(),
                    total,
                    used: total.saturating_sub(available),
                }
            })
            .collect();

        CurrentMetrics {
            hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
            os_name: System::name().unwrap_or_else(|| "unknown".to_string()),
            os_version: System::os_version().unwrap_or_default(),
            kernel: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
            architecture: std::env::consts::ARCH.to_string(),
            cpu_usage: sys.global_cpu_usage(),
            cpu_cores_physical: sys.physical_core_count().unwrap_or(0),
            cpu_cores_logical: sys.cpus().len(),
            load_avg: (load.one, load.five, load.fifteen),
            memory_total: sys.total_memory(),
            memory_used: sys.used_memory(),
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
            disks,
            bot_memory,
            db_size: get_db_file_size(db_path),
        }
    }

    /// Format current metrics as a Discord-ready markdown string
    pub fn format(&self, bot_uptime_secs: u64) -> String {
        let mem_percent = if self.memory_total > 0 {
            (self.memory_used as f64 / self.memory_total as f64) * 100.0
        } else {
            0.0
        };

        let swap_line = if self.swap_total > 0 {
            let swap_percent = (self.swap_used as f64 / self.swap_total as f64) * 100.0;
            format!(
                "Swap:    {} / {} ({:.1}%)\n",
                format_bytes(self.swap_used),
                format_bytes(self.swap_total),
                swap_percent
            )
        } else {
            String::new()
        };

        let mut disk_lines = String::new();
        for disk in &self.disks {
            let usage_percent = if disk.total > 0 {
                (disk.used as f64 / disk.total as f64) * 100.0
            } else {
                0.0
            };
            disk_lines.push_str(&format!(
                "{:<8} {} / {} ({:.1}%)\n",
                disk.mount,
                format_bytes(disk.used),
                format_bytes(disk.total),
                usage_percent
            ));
        }

        format!(
            "**System Information**\n```\n\
            Host:    {} ({} {})\n\
            Arch:    {} | Kernel: {}\n\
            \n\
            CPU:     {:.1}% | Cores: {}/{} | Load: {:.2}/{:.2}/{:.2}\n\
            RAM:     {} / {} ({:.1}%)\n\
            {}\
            \n\
            DB:      {}\n\
            {}\
            \n\
            Bot:     v{} | Up: {}\n\
            Process: {}\n\
            Rust:    {} | Serenity: v0.11.6\n\
            ```",
            self.hostname,
            self.os_name,
            self.os_version,
            self.architecture,
            self.kernel,
            self.cpu_usage,
            self.cpu_cores_physical,
            self.cpu_cores_logical,
            self.load_avg.0,
            self.load_avg.1,
            self.load_avg.2,
            format_bytes(self.memory_used),
            format_bytes(self.memory_total),
            mem_percent,
            swap_line,
            format_bytes(self.db_size),
            disk_lines,
            crate::features::get_bot_version(),
            format_duration(bot_uptime_secs),
            format_bytes(self.bot_memory),
            rustc_version_runtime::version(),
        )
    }
}

/// Historical metrics summary for display
pub struct HistoricalSummary {
    pub current: f64,
    pub oldest: f64,
    pub average: f64,
    pub peak: f64,
    pub has_data: bool,
}

impl HistoricalSummary {
    pub fn from_data(data: &[(i64, f64)]) -> Self {
        if data.is_empty() {
            return Self {
                current: 0.0,
                oldest: 0.0,
                average: 0.0,
                peak: 0.0,
                has_data: false,
            };
        }

        let current = data.last().map(|(_, v)| *v).unwrap_or(0.0);
        let oldest = data.first().map(|(_, v)| *v).unwrap_or(0.0);
        let sum: f64 = data.iter().map(|(_, v)| *v).sum();
        let average = sum / data.len() as f64;
        let peak = data.iter().map(|(_, v)| *v).fold(0.0_f64, |a, b| a.max(b));

        Self {
            current,
            oldest,
            average,
            peak,
            has_data: true,
        }
    }
}

/// Format historical metrics as a Discord-ready markdown string
pub fn format_history(
    db_size: HistoricalSummary,
    bot_memory: HistoricalSummary,
    system_memory: HistoricalSummary,
    system_cpu: HistoricalSummary,
    period_label: &str,
) -> String {
    let mut output = format!("**Metrics History ({period_label})**\n```\n");

    // Header row
    output.push_str("Metric       Current     Avg         Peak        Growth\n");
    output.push_str("─────────────────────────────────────────────────────────\n");

    // Database Size
    if db_size.has_data {
        let growth = db_size.current - db_size.oldest;
        let growth_percent = if db_size.oldest > 0.0 {
            (growth / db_size.oldest) * 100.0
        } else {
            0.0
        };
        output.push_str(&format!(
            "DB Size      {:<11} {:<11} {:<11} {:+.1}%\n",
            format_bytes(db_size.current as u64),
            format_bytes(db_size.average as u64),
            format_bytes(db_size.peak as u64),
            growth_percent
        ));
    } else {
        output.push_str("DB Size      (no data)\n");
    }

    // Bot Memory
    if bot_memory.has_data {
        output.push_str(&format!(
            "Bot Mem      {:<11} {:<11} {:<11} -\n",
            format_bytes(bot_memory.current as u64),
            format_bytes(bot_memory.average as u64),
            format_bytes(bot_memory.peak as u64),
        ));
    } else {
        output.push_str("Bot Mem      (no data)\n");
    }

    // System Memory
    if system_memory.has_data {
        output.push_str(&format!(
            "Sys RAM      {:<11} {:<11} {:<11} -\n",
            format!("{:.1}%", system_memory.current),
            format!("{:.1}%", system_memory.average),
            format!("{:.1}%", system_memory.peak),
        ));
    } else {
        output.push_str("Sys RAM      (no data)\n");
    }

    // System CPU
    if system_cpu.has_data {
        output.push_str(&format!(
            "Sys CPU      {:<11} {:<11} {:<11} -\n",
            format!("{:.1}%", system_cpu.current),
            format!("{:.1}%", system_cpu.average),
            format!("{:.1}%", system_cpu.peak),
        ));
    } else {
        output.push_str("Sys CPU      (no data)\n");
    }

    output.push_str("```");
    output
}

/// Get the size of the database file in bytes
pub fn get_db_file_size(path: &str) -> u64 {
    Path::new(path).metadata().map(|m| m.len()).unwrap_or(0)
}

/// Format bytes into human-readable string (e.g., "1.5 GB")
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Format signed bytes (for growth/change display)
pub fn format_bytes_signed(bytes: i64) -> String {
    let sign = if bytes >= 0 { "+" } else { "" };
    format!("{}{}", sign, format_bytes(bytes.unsigned_abs()))
}

/// Format duration into human-readable string (e.g., "3d 14h 22m 15s")
pub fn format_duration(total_secs: u64) -> String {
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m {seconds}s")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// Background task that collects system metrics periodically
pub async fn metrics_collection_loop(db: Arc<Database>, db_path: String) {
    let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
    let mut sys = System::new();
    let mut cleanup_counter = 0u32;

    info!("System metrics collection task started (interval: 5 minutes)");

    loop {
        interval.tick().await;

        debug!("Collecting system metrics...");

        // Refresh CPU (needs two calls for accurate reading)
        sys.refresh_cpu_usage();
        tokio::time::sleep(Duration::from_millis(200)).await;
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        // Record database size
        let db_size = get_db_file_size(&db_path);
        if let Err(e) = db
            .store_system_metric("db_size_bytes", db_size as f64)
            .await
        {
            warn!("Failed to store db_size metric: {e}");
        }

        // Record bot process memory
        if let Ok(pid) = sysinfo::get_current_pid() {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::new().with_memory(),
            );
            if let Some(proc) = sys.process(pid) {
                if let Err(e) = db
                    .store_system_metric("bot_memory_bytes", proc.memory() as f64)
                    .await
                {
                    warn!("Failed to store bot_memory metric: {e}");
                }
            }
        }

        // Record system memory percentage
        let memory_total = sys.total_memory();
        if memory_total > 0 {
            let memory_percent = (sys.used_memory() as f64 / memory_total as f64) * 100.0;
            if let Err(e) = db
                .store_system_metric("system_memory_percent", memory_percent)
                .await
            {
                warn!("Failed to store system_memory metric: {e}");
            }
        }

        // Record system CPU percentage
        if let Err(e) = db
            .store_system_metric("system_cpu_percent", sys.global_cpu_usage() as f64)
            .await
        {
            warn!("Failed to store system_cpu metric: {e}");
        }

        debug!("System metrics recorded successfully");

        // Cleanup old metrics once per day (288 intervals at 5 min each)
        cleanup_counter += 1;
        if cleanup_counter >= 288 {
            cleanup_counter = 0;
            info!("Running daily cleanup tasks");

            // Cleanup system metrics (7 days)
            if let Err(e) = db.cleanup_old_metrics(7).await {
                warn!("Failed to cleanup old system metrics: {e}");
            }

            // Cleanup raw OpenAI usage data (7 days - detailed request-level data)
            if let Err(e) = db.cleanup_old_openai_usage(7).await {
                warn!("Failed to cleanup old OpenAI usage data: {e}");
            }

            // Cleanup OpenAI daily aggregates (90 days - for historical trends)
            if let Err(e) = db.cleanup_old_openai_usage_daily(90).await {
                warn!("Failed to cleanup old OpenAI usage daily data: {e}");
            }

            info!("Daily cleanup tasks completed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
        assert_eq!(format_duration(90061), "1d 1h 1m 1s");
    }

    #[test]
    fn test_format_bytes_signed() {
        assert_eq!(format_bytes_signed(1024), "+1.0 KB");
        assert_eq!(format_bytes_signed(-1024), "1.0 KB");
        assert_eq!(format_bytes_signed(0), "+0 B");
    }

    #[test]
    fn test_historical_summary_empty() {
        let summary = HistoricalSummary::from_data(&[]);
        assert!(!summary.has_data);
    }

    #[test]
    fn test_historical_summary_with_data() {
        let data = vec![(1, 100.0), (2, 200.0), (3, 150.0)];
        let summary = HistoricalSummary::from_data(&data);
        assert!(summary.has_data);
        assert_eq!(summary.current, 150.0);
        assert_eq!(summary.oldest, 100.0);
        assert_eq!(summary.peak, 200.0);
        assert!((summary.average - 150.0).abs() < 0.01);
    }
}
