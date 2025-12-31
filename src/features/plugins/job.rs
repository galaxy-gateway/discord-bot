//! # Background Job System
//!
//! Track long-running plugin executions with database persistence for crash recovery.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.9.0

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
}

/// Manager for tracking plugin jobs
pub struct JobManager {
    /// In-memory job cache for fast lookups
    jobs: DashMap<String, Job>,

    /// Database for persistence
    database: Database,
}

impl JobManager {
    /// Create a new job manager
    pub fn new(database: Database) -> Self {
        Self {
            jobs: DashMap::new(),
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
        };

        // Store in memory
        self.jobs.insert(id.clone(), job.clone());

        // Persist to database
        self.persist_job(&job).await?;

        info!(
            "Created job {} for plugin {} by user {}",
            id, plugin_name, user_id
        );

        Ok(id)
    }

    /// Mark a job as running
    pub async fn start_job(&self, job_id: &str) -> Result<()> {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.status = JobStatus::Running;
            self.update_job_in_db(&job).await?;
            debug!("Job {} marked as running", job_id);
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
            info!("Job {} completed successfully", job_id);
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
            warn!("Job {} failed", job_id);
        }
        Ok(())
    }

    /// Set the thread ID for a job
    pub fn set_thread_id(&self, job_id: &str, thread_id: String) {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.thread_id = Some(thread_id);
            debug!("Job {} thread ID set", job_id);
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
            job.user_id == user_id
                && job.plugin_name == plugin_name
                && job.started_at > cutoff
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
            let keep = job.completed_at.map_or(true, |completed| completed > cutoff);
            if !keep {
                removed += 1;
            }
            keep
        });

        if removed > 0 {
            debug!("Cleaned up {} old jobs from memory", removed);
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
