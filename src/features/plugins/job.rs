//! # Background Job System
//!
//! Track long-running plugin executions with database persistence for crash recovery.
//! Supports both single video jobs and multi-video playlist jobs.
//!
//! - **Version**: 2.0.0
//! - **Since**: 0.9.0
//!
//! ## Changelog
//! - 2.0.0: Added PlaylistJob for multi-video transcription with progress tracking
//! - 1.0.0: Initial release with single job tracking

use crate::database::Database;
use anyhow::Result;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Status of a plugin job
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job created but not yet started
    Pending,
    /// Job is currently executing
    Running,
    /// Job completed successfully
    Completed,
    /// Job failed with an error
    Failed,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "pending"),
            JobStatus::Running => write!(f, "running"),
            JobStatus::Completed => write!(f, "completed"),
            JobStatus::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for JobStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(JobStatus::Pending),
            "running" => Ok(JobStatus::Running),
            "completed" => Ok(JobStatus::Completed),
            "failed" => Ok(JobStatus::Failed),
            _ => Err(anyhow::anyhow!("Invalid job status: {}", s)),
        }
    }
}

/// Status of a playlist job
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlaylistJobStatus {
    /// Job created but not yet started
    Pending,
    /// Job is currently processing videos
    Running,
    /// Job paused (e.g., for rate limiting or manual pause)
    Paused,
    /// All videos completed successfully
    Completed,
    /// Some videos completed, some failed
    PartialComplete,
    /// Job failed entirely
    Failed,
    /// Job was cancelled by user
    Cancelled,
}

impl std::fmt::Display for PlaylistJobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaylistJobStatus::Pending => write!(f, "pending"),
            PlaylistJobStatus::Running => write!(f, "running"),
            PlaylistJobStatus::Paused => write!(f, "paused"),
            PlaylistJobStatus::Completed => write!(f, "completed"),
            PlaylistJobStatus::PartialComplete => write!(f, "partial_complete"),
            PlaylistJobStatus::Failed => write!(f, "failed"),
            PlaylistJobStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::str::FromStr for PlaylistJobStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(PlaylistJobStatus::Pending),
            "running" => Ok(PlaylistJobStatus::Running),
            "paused" => Ok(PlaylistJobStatus::Paused),
            "completed" => Ok(PlaylistJobStatus::Completed),
            "partial_complete" => Ok(PlaylistJobStatus::PartialComplete),
            "failed" => Ok(PlaylistJobStatus::Failed),
            "cancelled" => Ok(PlaylistJobStatus::Cancelled),
            _ => Err(anyhow::anyhow!("Invalid playlist job status: {}", s)),
        }
    }
}

/// A plugin job record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique job identifier
    pub id: String,

    /// Name of the plugin that created this job
    pub plugin_name: String,

    /// User who initiated the job
    pub user_id: String,

    /// Guild where the job was initiated (None for DMs)
    pub guild_id: Option<String>,

    /// Channel where the job was initiated
    pub channel_id: String,

    /// Thread ID if output was posted to a thread
    pub thread_id: Option<String>,

    /// Current job status
    pub status: JobStatus,

    /// Input parameters
    pub params: HashMap<String, String>,

    /// When the job was created
    pub started_at: DateTime<Utc>,

    /// When the job completed (success or failure)
    pub completed_at: Option<DateTime<Utc>>,

    /// Result preview (truncated output for completed jobs)
    pub result: Option<String>,

    /// Error message for failed jobs
    pub error: Option<String>,

    /// Parent playlist job ID (if this job is part of a playlist)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_playlist_id: Option<String>,
}

/// A playlist job record for multi-video transcription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistJob {
    /// Unique job identifier
    pub id: String,

    /// User who initiated the job
    pub user_id: String,

    /// Guild where the job was initiated (None for DMs)
    pub guild_id: Option<String>,

    /// Channel where the job was initiated
    pub channel_id: String,

    /// Thread ID where output is posted
    pub thread_id: Option<String>,

    /// Original playlist URL
    pub playlist_url: String,

    /// Extracted playlist ID
    pub playlist_id: String,

    /// Playlist title
    pub playlist_title: Option<String>,

    /// Total number of videos in playlist (may be limited by max_videos)
    pub total_videos: u32,

    /// Number of videos completed successfully
    pub completed_videos: u32,

    /// Number of videos that failed
    pub failed_videos: u32,

    /// Number of videos skipped
    pub skipped_videos: u32,

    /// Current job status
    pub status: PlaylistJobStatus,

    /// Maximum videos to process (user-specified limit)
    pub max_videos: Option<u32>,

    /// Currently processing video job ID
    pub current_video_job_id: Option<String>,

    /// Error message (for failed jobs)
    pub error: Option<String>,

    /// When the job was created
    pub started_at: DateTime<Utc>,

    /// When the job completed
    pub completed_at: Option<DateTime<Utc>>,

    /// When the job was cancelled
    pub cancelled_at: Option<DateTime<Utc>>,

    /// Who cancelled the job
    pub cancelled_by: Option<String>,
}

impl PlaylistJob {
    /// Check if the job is still active (can process more videos)
    pub fn is_active(&self) -> bool {
        matches!(
            self.status,
            PlaylistJobStatus::Running | PlaylistJobStatus::Pending | PlaylistJobStatus::Paused
        )
    }

    /// Check if the job has been cancelled
    pub fn is_cancelled(&self) -> bool {
        matches!(self.status, PlaylistJobStatus::Cancelled)
    }

    /// Get the progress percentage
    pub fn progress_percent(&self) -> f32 {
        if self.total_videos == 0 {
            0.0
        } else {
            ((self.completed_videos + self.failed_videos + self.skipped_videos) as f32
                / self.total_videos as f32)
                * 100.0
        }
    }

    /// Get processed count (completed + failed + skipped)
    pub fn processed_count(&self) -> u32 {
        self.completed_videos + self.failed_videos + self.skipped_videos
    }
}

/// Manager for tracking plugin jobs
pub struct JobManager {
    /// In-memory job cache for fast lookups
    jobs: DashMap<String, Job>,

    /// In-memory playlist job cache
    playlist_jobs: DashMap<String, PlaylistJob>,

    /// Database for persistence
    database: Database,
}

impl JobManager {
    /// Create a new job manager
    pub fn new(database: Database) -> Self {
        Self {
            jobs: DashMap::new(),
            playlist_jobs: DashMap::new(),
            database,
        }
    }

    /// Create a new pending job
    pub async fn create_job(
        &self,
        plugin_name: &str,
        user_id: &str,
        guild_id: Option<&str>,
        channel_id: &str,
        params: HashMap<String, String>,
    ) -> Result<String> {
        self.create_job_with_parent(plugin_name, user_id, guild_id, channel_id, params, None)
            .await
    }

    /// Create a new pending job with optional parent playlist
    pub async fn create_job_with_parent(
        &self,
        plugin_name: &str,
        user_id: &str,
        guild_id: Option<&str>,
        channel_id: &str,
        params: HashMap<String, String>,
        parent_playlist_id: Option<&str>,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let job = Job {
            id: id.clone(),
            plugin_name: plugin_name.to_string(),
            user_id: user_id.to_string(),
            guild_id: guild_id.map(String::from),
            channel_id: channel_id.to_string(),
            thread_id: None,
            status: JobStatus::Pending,
            params,
            started_at: Utc::now(),
            completed_at: None,
            result: None,
            error: None,
            parent_playlist_id: parent_playlist_id.map(String::from),
        };

        // Store in memory
        self.jobs.insert(id.clone(), job.clone());

        // Persist to database
        self.persist_job(&job).await?;

        info!(
            "Created job {} for plugin {} by user {}{}",
            id,
            plugin_name,
            user_id,
            parent_playlist_id
                .map(|p| format!(" (playlist: {p})"))
                .unwrap_or_default()
        );

        Ok(id)
    }

    /// Mark a job as running
    pub async fn start_job(&self, job_id: &str) -> Result<()> {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.status = JobStatus::Running;
            self.update_job_in_db(&job).await?;
            debug!("Job {job_id} marked as running");
        }
        Ok(())
    }

    /// Mark a job as completed with a result preview
    pub async fn complete_job(&self, job_id: &str, result: String) -> Result<()> {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.status = JobStatus::Completed;
            job.completed_at = Some(Utc::now());
            job.result = Some(result);
            self.update_job_in_db(&job).await?;
            info!("Job {job_id} completed successfully");
        }
        Ok(())
    }

    /// Mark a job as failed with an error message
    pub async fn fail_job(&self, job_id: &str, error: String) -> Result<()> {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.status = JobStatus::Failed;
            job.completed_at = Some(Utc::now());
            job.error = Some(error);
            self.update_job_in_db(&job).await?;
            warn!("Job {job_id} failed");
        }
        Ok(())
    }

    /// Set the thread ID for a job
    pub fn set_thread_id(&self, job_id: &str, thread_id: String) {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.thread_id = Some(thread_id);
            debug!("Job {job_id} thread ID set");
        }
    }

    /// Get a job by ID
    pub fn get_job(&self, job_id: &str) -> Option<Job> {
        self.jobs.get(job_id).map(|j| j.clone())
    }

    /// Get all jobs for a user
    pub fn get_user_jobs(&self, user_id: &str) -> Vec<Job> {
        self.jobs
            .iter()
            .filter(|j| j.user_id == user_id)
            .map(|j| j.clone())
            .collect()
    }

    /// Get recent jobs for a plugin
    pub fn get_plugin_jobs(&self, plugin_name: &str, limit: usize) -> Vec<Job> {
        let mut jobs: Vec<_> = self
            .jobs
            .iter()
            .filter(|j| j.plugin_name == plugin_name)
            .map(|j| j.clone())
            .collect();

        // Sort by start time descending
        jobs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        jobs.truncate(limit);
        jobs
    }

    /// Check if a user is within cooldown period for a plugin
    pub fn check_cooldown(&self, user_id: &str, plugin_name: &str, cooldown_seconds: u64) -> bool {
        if cooldown_seconds == 0 {
            return true;
        }

        let cutoff = Utc::now() - chrono::Duration::seconds(cooldown_seconds as i64);

        // Check if any recent job exists
        !self.jobs.iter().any(|job| {
            job.user_id == user_id && job.plugin_name == plugin_name && job.started_at > cutoff
        })
    }

    /// Persist a new job to the database
    async fn persist_job(&self, job: &Job) -> Result<()> {
        self.database.create_plugin_job(job).await
    }

    /// Update a job in the database
    async fn update_job_in_db(&self, job: &Job) -> Result<()> {
        self.database.update_plugin_job(job).await
    }

    /// Load incomplete jobs from database (for crash recovery)
    pub async fn recover_jobs(&self) -> Result<Vec<Job>> {
        let jobs = self.database.get_incomplete_plugin_jobs().await?;

        // Load into memory cache
        for job in &jobs {
            self.jobs.insert(job.id.clone(), job.clone());
        }

        if !jobs.is_empty() {
            info!("Recovered {} incomplete jobs from database", jobs.len());
        }

        Ok(jobs)
    }

    /// Cleanup old completed jobs from memory (keep database records)
    pub fn cleanup_old_jobs(&self, max_age_hours: i64) {
        let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours);
        let mut removed = 0;

        self.jobs.retain(|_, job| {
            let keep = job
                .completed_at
                .is_none_or(|completed| completed > cutoff);
            if !keep {
                removed += 1;
            }
            keep
        });

        if removed > 0 {
            debug!("Cleaned up {removed} old jobs from memory");
        }
    }

    /// Get counts of jobs by status
    pub fn get_stats(&self) -> HashMap<JobStatus, usize> {
        let mut stats = HashMap::new();
        stats.insert(JobStatus::Pending, 0);
        stats.insert(JobStatus::Running, 0);
        stats.insert(JobStatus::Completed, 0);
        stats.insert(JobStatus::Failed, 0);

        for job in self.jobs.iter() {
            *stats.entry(job.status.clone()).or_insert(0) += 1;
        }

        stats
    }

    // Playlist Job Methods

    /// Create a new playlist job
    pub async fn create_playlist_job(
        &self,
        user_id: &str,
        guild_id: Option<&str>,
        channel_id: &str,
        playlist_url: &str,
        playlist_id: &str,
        playlist_title: Option<&str>,
        total_videos: u32,
        max_videos: Option<u32>,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let job = PlaylistJob {
            id: id.clone(),
            user_id: user_id.to_string(),
            guild_id: guild_id.map(String::from),
            channel_id: channel_id.to_string(),
            thread_id: None,
            playlist_url: playlist_url.to_string(),
            playlist_id: playlist_id.to_string(),
            playlist_title: playlist_title.map(String::from),
            total_videos,
            completed_videos: 0,
            failed_videos: 0,
            skipped_videos: 0,
            status: PlaylistJobStatus::Pending,
            max_videos,
            current_video_job_id: None,
            error: None,
            started_at: Utc::now(),
            completed_at: None,
            cancelled_at: None,
            cancelled_by: None,
        };

        // Store in memory
        self.playlist_jobs.insert(id.clone(), job.clone());

        // Persist to database
        self.database.create_playlist_job(&job).await?;

        info!(
            "Created playlist job {id} for playlist {playlist_id} ({total_videos} videos) by user {user_id}"
        );

        Ok(id)
    }

    /// Mark a playlist job as running
    pub async fn start_playlist_job(&self, job_id: &str) -> Result<()> {
        if let Some(mut job) = self.playlist_jobs.get_mut(job_id) {
            job.status = PlaylistJobStatus::Running;
            self.database.update_playlist_job(&job).await?;
            debug!("Playlist job {job_id} marked as running");
        }
        Ok(())
    }

    /// Set the thread ID for a playlist job
    pub fn set_playlist_thread_id(&self, job_id: &str, thread_id: String) {
        if let Some(mut job) = self.playlist_jobs.get_mut(job_id) {
            job.thread_id = Some(thread_id);
            debug!("Playlist job {job_id} thread ID set");
        }
    }

    /// Update playlist job progress
    pub async fn update_playlist_progress(
        &self,
        job_id: &str,
        completed: u32,
        failed: u32,
        skipped: u32,
        current_video_job_id: Option<&str>,
    ) -> Result<()> {
        if let Some(mut job) = self.playlist_jobs.get_mut(job_id) {
            job.completed_videos = completed;
            job.failed_videos = failed;
            job.skipped_videos = skipped;
            job.current_video_job_id = current_video_job_id.map(String::from);
            self.database.update_playlist_job(&job).await?;
            debug!(
                "Playlist job {} progress: {}/{} completed, {} failed, {} skipped",
                job_id, completed, job.total_videos, failed, skipped
            );
        }
        Ok(())
    }

    /// Mark a playlist job as completed
    pub async fn complete_playlist_job(&self, job_id: &str) -> Result<()> {
        if let Some(mut job) = self.playlist_jobs.get_mut(job_id) {
            job.status = if job.failed_videos > 0 {
                PlaylistJobStatus::PartialComplete
            } else {
                PlaylistJobStatus::Completed
            };
            job.completed_at = Some(Utc::now());
            job.current_video_job_id = None;
            self.database.update_playlist_job(&job).await?;
            info!(
                "Playlist job {} completed: {}/{} successful, {} failed",
                job_id, job.completed_videos, job.total_videos, job.failed_videos
            );
        }
        Ok(())
    }

    /// Mark a playlist job as failed
    pub async fn fail_playlist_job(&self, job_id: &str, error: String) -> Result<()> {
        if let Some(mut job) = self.playlist_jobs.get_mut(job_id) {
            job.status = PlaylistJobStatus::Failed;
            job.completed_at = Some(Utc::now());
            job.error = Some(error.clone());
            job.current_video_job_id = None;
            self.database.update_playlist_job(&job).await?;
            warn!("Playlist job {job_id} failed: {error}");
        }
        Ok(())
    }

    /// Cancel a playlist job
    pub async fn cancel_playlist_job(&self, job_id: &str, cancelled_by: &str) -> Result<bool> {
        if let Some(mut job) = self.playlist_jobs.get_mut(job_id) {
            if job.is_active() {
                job.status = PlaylistJobStatus::Cancelled;
                job.cancelled_at = Some(Utc::now());
                job.cancelled_by = Some(cancelled_by.to_string());
                job.current_video_job_id = None;
                self.database.update_playlist_job(&job).await?;
                info!("Playlist job {job_id} cancelled by {cancelled_by}");
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Check if a playlist job has been cancelled
    pub fn is_playlist_cancelled(&self, job_id: &str) -> bool {
        self.playlist_jobs
            .get(job_id)
            .map(|j| j.is_cancelled())
            .unwrap_or(false)
    }

    /// Get a playlist job by ID
    pub fn get_playlist_job(&self, job_id: &str) -> Option<PlaylistJob> {
        self.playlist_jobs.get(job_id).map(|j| j.clone())
    }

    /// Get active playlist jobs for a user
    pub fn get_user_active_playlist_jobs(&self, user_id: &str) -> Vec<PlaylistJob> {
        self.playlist_jobs
            .iter()
            .filter(|j| j.user_id == user_id && j.is_active())
            .map(|j| j.clone())
            .collect()
    }

    /// Check if user has an active playlist job
    pub fn has_active_playlist(&self, user_id: &str) -> bool {
        self.playlist_jobs
            .iter()
            .any(|j| j.user_id == user_id && j.is_active())
    }

    /// Recover incomplete playlist jobs from database
    pub async fn recover_playlist_jobs(&self) -> Result<Vec<PlaylistJob>> {
        let jobs = self.database.get_incomplete_playlist_jobs().await?;

        // Load into memory cache
        for job in &jobs {
            self.playlist_jobs.insert(job.id.clone(), job.clone());
        }

        if !jobs.is_empty() {
            info!(
                "Recovered {} incomplete playlist jobs from database",
                jobs.len()
            );
        }

        Ok(jobs)
    }

    /// Get completed video IDs for a playlist job (for resume)
    pub async fn get_completed_video_ids(&self, playlist_job_id: &str) -> Result<Vec<String>> {
        self.database
            .get_completed_video_job_ids(playlist_job_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_display() {
        assert_eq!(JobStatus::Pending.to_string(), "pending");
        assert_eq!(JobStatus::Running.to_string(), "running");
        assert_eq!(JobStatus::Completed.to_string(), "completed");
        assert_eq!(JobStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_job_status_parse() {
        assert_eq!("pending".parse::<JobStatus>().unwrap(), JobStatus::Pending);
        assert_eq!("RUNNING".parse::<JobStatus>().unwrap(), JobStatus::Running);
        assert_eq!(
            "Completed".parse::<JobStatus>().unwrap(),
            JobStatus::Completed
        );
        assert_eq!("failed".parse::<JobStatus>().unwrap(), JobStatus::Failed);
        assert!("invalid".parse::<JobStatus>().is_err());
    }
}
