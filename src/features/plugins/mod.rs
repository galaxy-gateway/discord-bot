//! # Feature: Plugin System
//!
//! Extensible plugin architecture for executing CLI commands via Discord slash commands.
//! Supports YAML-based configuration, secure execution, background jobs, and thread-based output.
//! Now includes multi-video playlist transcription with progress tracking.
//!
//! - **Version**: 2.1.0
//! - **Since**: 0.9.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 2.1.0: Thread-first output - ephemeral responses, minimal starter messages, content in thread
//! - 2.0.0: Added playlist support with multi-video transcription and progress tracking
//! - 1.5.0: If command used inside thread, append to existing thread instead of creating new
//! - 1.4.0: Consolidated to single post - interaction response becomes thread starter
//! - 1.3.0: Fetch YouTube title via oEmbed for thread name, simpler thread creation flow
//! - 1.2.0: Thread created immediately with URL, progress status posted before execution
//! - 1.1.0: Added structured output posting with source_param, thread from interaction response
//! - 1.0.0: Initial release with config-based plugins, CLI executor, and job system

pub mod commands;
pub mod config;
pub mod executor;
pub mod job;
pub mod output;
pub mod youtube;

pub use commands::create_plugin_commands;
pub use config::{Plugin, PluginConfig};
pub use executor::{ExecutionResult, PluginExecutor};
pub use job::{Job, JobManager, JobStatus, PlaylistJob, PlaylistJobStatus};
pub use output::OutputHandler;
pub use youtube::{parse_youtube_url, enumerate_playlist, PlaylistInfo, PlaylistItem, YouTubeUrl, YouTubeUrlType};

use crate::database::Database;
use anyhow::Result;
use log::{error, info, warn};
use serenity::http::Http;
use serenity::model::id::ChannelId;
use std::collections::HashMap;
use std::sync::Arc;

/// Central manager for the plugin system
#[derive(Clone)]
pub struct PluginManager {
    pub config: PluginConfig,
    pub executor: PluginExecutor,
    pub job_manager: Arc<JobManager>,
    pub output_handler: OutputHandler,
}

impl PluginManager {
    /// Create a new PluginManager with the given configuration
    pub fn new(
        config: PluginConfig,
        database: Database,
        openai_model: String,
        allowed_commands: Vec<String>,
    ) -> Self {
        Self {
            config,
            executor: PluginExecutor::new(allowed_commands),
            job_manager: Arc::new(JobManager::new(database)),
            output_handler: OutputHandler::new(openai_model),
        }
    }

    /// Load plugin configuration from a YAML file
    pub fn from_file(
        path: &str,
        database: Database,
        openai_model: String,
        allowed_commands: Vec<String>,
    ) -> Result<Self> {
        let config = PluginConfig::load(path)?;
        info!("Loaded {} plugin(s) from {}", config.plugins.len(), path);
        Ok(Self::new(config, database, openai_model, allowed_commands))
    }

    /// Get a plugin by name
    pub fn get_plugin(&self, name: &str) -> Option<&Plugin> {
        self.config.plugins.iter().find(|p| p.name == name)
    }

    /// Get a plugin by command name
    pub fn get_plugin_by_command(&self, command_name: &str) -> Option<&Plugin> {
        self.config
            .plugins
            .iter()
            .find(|p| p.enabled && p.command.name == command_name)
    }

    /// Check if a user can use a plugin (security checks)
    pub fn check_access(
        &self,
        plugin: &Plugin,
        user_id: &str,
        user_roles: &[String],
    ) -> Result<()> {
        // Check blocked users
        if plugin.security.blocked_users.contains(&user_id.to_string()) {
            return Err(anyhow::anyhow!("You are blocked from using this plugin"));
        }

        // Check allowed users (if specified)
        if !plugin.security.allowed_users.is_empty()
            && !plugin.security.allowed_users.contains(&user_id.to_string())
        {
            return Err(anyhow::anyhow!(
                "You are not authorized to use this plugin"
            ));
        }

        // Check allowed roles (if specified)
        if !plugin.security.allowed_roles.is_empty() {
            let has_role = user_roles
                .iter()
                .any(|r| plugin.security.allowed_roles.contains(r));
            if !has_role {
                return Err(anyhow::anyhow!(
                    "You don't have a required role to use this plugin"
                ));
            }
        }

        Ok(())
    }

    /// Check cooldown for a user on a plugin
    pub fn check_cooldown(&self, plugin: &Plugin, user_id: &str) -> Result<()> {
        if plugin.security.cooldown_seconds > 0 {
            if !self
                .job_manager
                .check_cooldown(user_id, &plugin.name, plugin.security.cooldown_seconds)
            {
                return Err(anyhow::anyhow!(
                    "Please wait {} seconds before using this plugin again",
                    plugin.security.cooldown_seconds
                ));
            }
        }
        Ok(())
    }

    /// Validate input parameters against plugin schema
    pub fn validate_params(
        &self,
        plugin: &Plugin,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        for opt in &plugin.command.options {
            let value = params.get(&opt.name);

            // Check required
            if opt.required && value.is_none() {
                return Err(anyhow::anyhow!("Missing required parameter: {}", opt.name));
            }

            // Validate if present
            if let Some(val) = value {
                if let Some(ref validation) = opt.validation {
                    // Check pattern
                    if let Some(ref pattern) = validation.pattern {
                        let re = regex::Regex::new(pattern)?;
                        if !re.is_match(val) {
                            return Err(anyhow::anyhow!(
                                "Parameter '{}' does not match required format",
                                opt.name
                            ));
                        }
                    }

                    // Check length
                    if let Some(min_len) = validation.min_length {
                        if val.len() < min_len {
                            return Err(anyhow::anyhow!(
                                "Parameter '{}' must be at least {} characters",
                                opt.name,
                                min_len
                            ));
                        }
                    }
                    if let Some(max_len) = validation.max_length {
                        if val.len() > max_len {
                            return Err(anyhow::anyhow!(
                                "Parameter '{}' must be at most {} characters",
                                opt.name,
                                max_len
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Execute a plugin in the background
    /// Returns the job ID synchronously, then continues execution asynchronously
    ///
    /// When `interaction_info` is provided, the thread will be created from the
    /// interaction's original response message (edited to show the source URL).
    /// If `is_thread` is true, skips thread creation and posts directly to the channel.
    pub async fn execute_plugin(
        &self,
        http: Arc<Http>,
        plugin: Plugin,
        params: HashMap<String, String>,
        user_id: String,
        guild_id: Option<String>,
        channel_id: ChannelId,
        interaction_info: Option<(u64, String)>, // (application_id, interaction_token)
        is_thread: bool, // If true, we're already in a thread - skip creation
    ) -> Result<String> {
        // Create job record synchronously so we can return the job_id
        let job_id = self.job_manager
            .create_job(
                &plugin.name,
                &user_id,
                guild_id.as_deref(),
                &channel_id.to_string(),
                params.clone(),
            )
            .await?;

        let job_manager = self.job_manager.clone();
        let executor = self.executor.clone();
        let output_handler = self.output_handler.clone();
        let job_id_clone = job_id.clone();

        tokio::spawn(async move {
            // Mark as running
            if let Err(e) = job_manager.start_job(&job_id_clone).await {
                warn!("Failed to mark job as running: {}", e);
            }

            // Determine source URL for structured output
            let source_url = plugin.output.source_param.as_ref()
                .and_then(|param| params.get(param))
                .cloned();

            // STEP 1: Create thread IMMEDIATELY (before execution) if configured
            // Skip thread creation if we're already inside a thread
            let output_channel = if plugin.output.create_thread && !is_thread {
                // Fetch video title if we have a YouTube URL
                let thread_name = if let Some(ref url) = source_url {
                    if url.contains("youtube.com") || url.contains("youtu.be") {
                        match fetch_youtube_title(url).await {
                            Some(title) => {
                                info!("Fetched YouTube title: {}", title);
                                title
                            }
                            None => {
                                let template = plugin.output.thread_name_template.as_deref()
                                    .unwrap_or("Plugin Output");
                                substitute_params(template, &params)
                            }
                        }
                    } else {
                        let template = plugin.output.thread_name_template.as_deref()
                            .unwrap_or("Plugin Output");
                        substitute_params(template, &params)
                    }
                } else {
                    let template = plugin.output.thread_name_template.as_deref()
                        .unwrap_or("Plugin Output");
                    substitute_params(template, &params)
                };

                // Truncate thread name to 100 chars (Discord limit)
                let thread_name = if thread_name.len() > 100 {
                    format!("{}...", &thread_name[..97])
                } else {
                    thread_name
                };

                // Edit ephemeral interaction response with confirmation (only user sees this)
                if let Some((app_id, ref token)) = interaction_info {
                    let client = reqwest::Client::new();
                    let edit_url = format!(
                        "https://discord.com/api/v10/webhooks/{}/{}/messages/@original",
                        app_id, token
                    );
                    let _ = client
                        .patch(&edit_url)
                        .header("Content-Type", "application/json")
                        .json(&serde_json::json!({ "content": format!("Starting transcription for \"{}\"...", thread_name) }))
                        .send()
                        .await;
                }

                // Send minimal thread starter message to channel (this is what others see)
                let starter_content = "Transcription";

                // Create thread from a new message (not the ephemeral response)
                let thread_channel = match channel_id.say(&http, starter_content).await {
                    Ok(msg) => {
                        match output_handler
                            .create_output_thread(&http, channel_id, msg.id, &thread_name, plugin.output.auto_archive_minutes)
                            .await
                        {
                            Ok(thread) => {
                                info!("Created thread: {} ({})", thread_name, thread.id);
                                job_manager.set_thread_id(&job_id_clone, thread.id.to_string());

                                // Post URL inside the thread (first message in thread)
                                let thread_id = ChannelId(thread.id.0);
                                if let Some(ref url) = source_url {
                                    let url_content = format!("**{}**\n{}", thread_name, url);
                                    let _ = thread_id.say(&http, &url_content).await;
                                }

                                Some(thread_id)
                            }
                            Err(e) => {
                                error!("Failed to create thread: {}", e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to send thread starter message: {}", e);
                        None
                    }
                };

                thread_channel.unwrap_or(channel_id)
            } else if is_thread && plugin.output.create_thread {
                // Already in a thread - edit ephemeral response and post to this thread
                if let Some(ref url) = source_url {
                    // Fetch title for display
                    let title = if url.contains("youtube.com") || url.contains("youtu.be") {
                        fetch_youtube_title(url).await.unwrap_or_else(|| "Video".to_string())
                    } else {
                        "Content".to_string()
                    };

                    // Edit ephemeral interaction response with confirmation (only user sees this)
                    if let Some((app_id, ref token)) = interaction_info {
                        let client = reqwest::Client::new();
                        let edit_url = format!(
                            "https://discord.com/api/v10/webhooks/{}/{}/messages/@original",
                            app_id, token
                        );
                        let _ = client
                            .patch(&edit_url)
                            .header("Content-Type", "application/json")
                            .json(&serde_json::json!({ "content": format!("Transcribing \"{}\" in this thread...", title) }))
                            .send()
                            .await;
                    }

                    // Post the URL to the thread (this is visible to everyone in the thread)
                    let content = format!("**{}**\n{}", title, url);
                    let _ = channel_id.say(&http, &content).await;
                }
                channel_id
            } else {
                channel_id
            };

            // STEP 2: Post progress status BEFORE execution
            // Post to thread (either new or existing)
            let should_post_status = plugin.output.create_thread && (is_thread || output_channel != channel_id);
            if should_post_status {
                let status_msg = "⏳ Transcribing video... This may take a few minutes.";
                if let Err(e) = output_channel.say(&http, status_msg).await {
                    warn!("Failed to post progress status: {}", e);
                }
            }

            // STEP 3: Execute the command (this is the long-running part)
            let result = executor.execute(&plugin.execution, &params).await;

            // STEP 4: Post results in thread
            match result {
                Ok(exec_result) => {
                    if exec_result.success {
                        // URL is already posted as thread starter, so skip it in structured output
                        let url_already_posted = plugin.output.create_thread;
                        let post_result = if let Some(ref url) = source_url {
                            output_handler
                                .post_structured_result(&http, output_channel, url, &exec_result.stdout, &plugin.output, url_already_posted)
                                .await
                        } else {
                            output_handler
                                .post_result(&http, output_channel, &exec_result.stdout, &plugin.output)
                                .await
                        };

                        if let Err(e) = post_result {
                            error!("Failed to post result: {}", e);
                        }

                        // Mark job complete
                        let preview = exec_result.stdout.chars().take(500).collect::<String>();
                        if let Err(e) = job_manager.complete_job(&job_id_clone, preview).await {
                            warn!("Failed to mark job complete: {}", e);
                        }
                    } else {
                        // Command failed
                        let error_msg = if exec_result.timed_out {
                            format!("Command timed out after {} seconds", plugin.execution.timeout_seconds)
                        } else {
                            format!(
                                "Command failed (exit code: {:?})\n{}",
                                exec_result.exit_code,
                                exec_result.stderr
                            )
                        };

                        if let Err(e) = output_handler
                            .post_error(
                                &http,
                                output_channel,
                                &error_msg,
                                plugin.output.error_template.as_deref(),
                            )
                            .await
                        {
                            error!("Failed to post error: {}", e);
                        }

                        if let Err(e) = job_manager.fail_job(&job_id_clone, error_msg).await {
                            warn!("Failed to mark job as failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    let error_msg = format!("Execution error: {}", e);
                    if let Err(post_err) = output_handler
                        .post_error(
                            &http,
                            output_channel,
                            &error_msg,
                            plugin.output.error_template.as_deref(),
                        )
                        .await
                    {
                        error!("Failed to post error: {}", post_err);
                    }

                    if let Err(fail_err) = job_manager.fail_job(&job_id_clone, error_msg).await {
                        warn!("Failed to mark job as failed: {}", fail_err);
                    }
                }
            }
        });

        Ok(job_id)
    }

    /// Execute a playlist transcription operation
    ///
    /// Handles multi-video playlists with progress tracking, per-video results,
    /// and a combined transcript at the end.
    pub async fn execute_playlist(
        &self,
        http: Arc<Http>,
        plugin: Plugin,
        playlist_info: youtube::PlaylistInfo,
        user_id: String,
        guild_id: Option<String>,
        channel_id: ChannelId,
        interaction_info: Option<(u64, String)>,
        max_videos: Option<u32>,
    ) -> Result<String> {
        let playlist_config = plugin.playlist.clone().unwrap_or_default();

        // Apply limits
        let effective_max = max_videos
            .unwrap_or(playlist_config.default_max_videos)
            .min(playlist_config.max_videos_per_request);

        let videos: Vec<_> = playlist_info.items.into_iter().take(effective_max as usize).collect();
        let total_videos = videos.len() as u32;

        // Create playlist job record
        let playlist_job_id = self.job_manager
            .create_playlist_job(
                &user_id,
                guild_id.as_deref(),
                &channel_id.to_string(),
                &playlist_info.title,
                &playlist_info.id,
                Some(&playlist_info.title),
                total_videos,
                Some(effective_max),
            )
            .await?;

        let job_manager = self.job_manager.clone();
        let executor = self.executor.clone();
        let output_handler = self.output_handler.clone();
        let playlist_job_id_clone = playlist_job_id.clone();
        let playlist_title = playlist_info.title.clone();
        let playlist_url = format!("https://www.youtube.com/playlist?list={}", playlist_info.id);

        tokio::spawn(async move {
            // Mark as running
            if let Err(e) = job_manager.start_playlist_job(&playlist_job_id_clone).await {
                warn!("Failed to mark playlist job as running: {}", e);
            }

            // STEP 1: Create thread for playlist
            let thread_name = format!("Playlist: {} ({} videos)",
                if playlist_title.len() > 60 {
                    format!("{}...", &playlist_title[..57])
                } else {
                    playlist_title.clone()
                },
                total_videos
            );

            // Truncate to Discord limit
            let thread_name = if thread_name.len() > 100 {
                format!("{}...", &thread_name[..97])
            } else {
                thread_name
            };

            // Edit ephemeral interaction response with confirmation (only user sees this)
            if let Some((app_id, ref token)) = interaction_info {
                let client = reqwest::Client::new();
                let edit_url = format!(
                    "https://discord.com/api/v10/webhooks/{}/{}/messages/@original",
                    app_id, token
                );
                let _ = client
                    .patch(&edit_url)
                    .header("Content-Type", "application/json")
                    .json(&serde_json::json!({ "content": format!("Starting playlist transcription for \"{}\" ({} videos)...", playlist_title, total_videos) }))
                    .send()
                    .await;
            }

            // Send minimal thread starter message to channel
            let starter_content = "Playlist transcription";

            // Create thread from a new message (not the ephemeral response)
            let thread_channel = match channel_id.say(&http, starter_content).await {
                Ok(msg) => {
                    match output_handler
                        .create_output_thread(&http, channel_id, msg.id, &thread_name, 1440)
                        .await
                    {
                        Ok(thread) => {
                            info!("Created playlist thread: {} ({})", thread_name, thread.id);
                            job_manager.set_playlist_thread_id(&playlist_job_id_clone, thread.id.to_string());

                            // Post playlist URL inside the thread (first message in thread)
                            let thread_id = ChannelId(thread.id.0);
                            let url_content = format!("**{}**\n{}", playlist_title, playlist_url);
                            let _ = thread_id.say(&http, &url_content).await;

                            Some(thread_id)
                        }
                        Err(e) => {
                            error!("Failed to create thread: {}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to send thread starter message: {}", e);
                    None
                }
            };

            let output_channel = thread_channel.unwrap_or(channel_id);

            // STEP 2: Process videos sequentially
            let mut completed = 0u32;
            let mut failed = 0u32;
            let skipped = 0u32;
            let mut combined_transcript = String::new();
            let mut progress_message_id: Option<serenity::model::id::MessageId> = None;
            let start_time = std::time::Instant::now();

            // Post initial progress
            if let Ok(msg_id) = output_handler
                .post_playlist_progress(&http, output_channel, None, 1, total_videos, &videos[0].title, None)
                .await
            {
                progress_message_id = Some(msg_id);
            }

            for (index, video) in videos.iter().enumerate() {
                let video_index = (index + 1) as u32;

                // Check for cancellation
                if job_manager.is_playlist_cancelled(&playlist_job_id_clone) {
                    info!("Playlist job {} cancelled, stopping at video {}", playlist_job_id_clone, video_index);
                    break;
                }

                // Update progress
                let remaining_videos = total_videos - video_index + 1;
                let avg_time_per_video = if index > 0 {
                    start_time.elapsed() / (index as u32)
                } else {
                    std::time::Duration::from_secs(120) // Initial estimate: 2 min per video
                };
                let eta = avg_time_per_video * remaining_videos;

                if let Some(msg_id) = progress_message_id {
                    let _ = output_handler
                        .post_playlist_progress(
                            &http, output_channel, Some(msg_id),
                            video_index, total_videos, &video.title, Some(eta)
                        )
                        .await;
                }

                // Create child job for this video
                let mut params = HashMap::new();
                params.insert("url".to_string(), video.url.clone());

                let video_job_id = match job_manager
                    .create_job_with_parent(
                        &plugin.name,
                        &user_id,
                        guild_id.as_deref(),
                        &output_channel.to_string(),
                        params.clone(),
                        Some(&playlist_job_id_clone),
                    )
                    .await
                {
                    Ok(id) => id,
                    Err(e) => {
                        warn!("Failed to create video job: {}", e);
                        failed += 1;
                        continue;
                    }
                };

                // Update playlist progress with current video
                let _ = job_manager
                    .update_playlist_progress(&playlist_job_id_clone, completed, failed, skipped, Some(&video_job_id))
                    .await;

                // Mark video job as running
                let _ = job_manager.start_job(&video_job_id).await;

                // Execute transcription
                let result = executor.execute(&plugin.execution, &params).await;

                match result {
                    Ok(exec_result) => {
                        if exec_result.success && !exec_result.stdout.is_empty() {
                            // Post video result
                            if let Err(e) = output_handler
                                .post_video_result(
                                    &http, output_channel,
                                    video_index, total_videos,
                                    &video.title, &video.url,
                                    &exec_result.stdout, &plugin.output
                                )
                                .await
                            {
                                warn!("Failed to post video result: {}", e);
                            }

                            // Add to combined transcript
                            let separator = "=".repeat(60);
                            combined_transcript.push_str(&format!(
                                "\n\n{}\n[{}/{}] {}\n{}\n{}\n\n{}",
                                separator, video_index, total_videos, video.title, video.url,
                                separator, exec_result.stdout
                            ));

                            let _ = job_manager.complete_job(&video_job_id, "completed".to_string()).await;
                            completed += 1;
                        } else {
                            let error_msg = if exec_result.timed_out {
                                "Transcription timed out".to_string()
                            } else {
                                exec_result.stderr.clone()
                            };

                            let _ = output_handler
                                .post_video_failed(
                                    &http, output_channel,
                                    video_index, total_videos,
                                    &video.title, &video.url,
                                    &error_msg
                                )
                                .await;

                            let _ = job_manager.fail_job(&video_job_id, error_msg).await;
                            failed += 1;
                        }
                    }
                    Err(e) => {
                        let _ = output_handler
                            .post_video_failed(
                                &http, output_channel,
                                video_index, total_videos,
                                &video.title, &video.url,
                                &e.to_string()
                            )
                            .await;

                        let _ = job_manager.fail_job(&video_job_id, e.to_string()).await;
                        failed += 1;
                    }
                }

                // Update playlist progress
                let _ = job_manager
                    .update_playlist_progress(&playlist_job_id_clone, completed, failed, skipped, None)
                    .await;

                // Delay between videos to avoid rate limits
                if index < videos.len() - 1 {
                    tokio::time::sleep(std::time::Duration::from_secs(
                        playlist_config.min_video_interval_seconds
                    )).await;
                }
            }

            // STEP 3: Post final summary
            let runtime = start_time.elapsed();

            if job_manager.is_playlist_cancelled(&playlist_job_id_clone) {
                // Was cancelled
                if let Some(job) = job_manager.get_playlist_job(&playlist_job_id_clone) {
                    let cancelled_by = job.cancelled_by.unwrap_or_else(|| "user".to_string());
                    let _ = output_handler
                        .post_playlist_cancelled(&http, output_channel, completed, total_videos, &cancelled_by)
                        .await;
                }
            } else {
                // Post summary with combined transcript
                let combined = if !combined_transcript.is_empty() {
                    Some(combined_transcript.as_str())
                } else {
                    None
                };

                let _ = output_handler
                    .post_playlist_summary(
                        &http, output_channel,
                        &playlist_title,
                        completed, failed, skipped, total_videos,
                        runtime,
                        combined
                    )
                    .await;
            }

            // Mark playlist job complete
            let _ = job_manager.complete_playlist_job(&playlist_job_id_clone).await;

            info!(
                "Playlist job {} completed: {}/{} successful, {} failed",
                playlist_job_id_clone, completed, total_videos, failed
            );
        });

        Ok(playlist_job_id)
    }
}

/// Substitute ${param} placeholders in a string
fn substitute_params(template: &str, params: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in params {
        let placeholder = format!("${{{}}}", key);
        result = result.replace(&placeholder, value);
    }
    result
}

/// Fetch YouTube video title via oEmbed API
async fn fetch_youtube_title(url: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://www.youtube.com/oembed")
        .query(&[("url", url), ("format", "json")])
        .send()
        .await;

    match resp {
        Ok(r) => {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                json.get("title")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }
        Err(e) => {
            warn!("Failed to fetch YouTube title: {}", e);
            None
        }
    }
}
