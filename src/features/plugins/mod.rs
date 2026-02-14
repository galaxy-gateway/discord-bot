//! # Feature: Plugin System
//!
//! Extensible plugin architecture for executing CLI commands via Discord slash commands.
//! Supports YAML-based configuration, secure execution, background jobs, and thread-based output.
//! Now includes multi-video playlist transcription with progress tracking and chunked streaming
//! for long videos with per-chunk summaries.
//!
//! - **Version**: 3.19.0
//! - **Since**: 0.9.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 3.19.0: Export create_plugins_command (single /plugins command with subcommands)
//! - 3.18.0: Fix playlist transcription downloading playlist instead of individual videos by using chunked path
//! - 3.17.0: Fix race condition in concurrent thread creation with retry logic and proper error reporting
//! - 3.16.0: Simplified thread starter format with cleaner YouTube heading and proper embed support
//! - 3.15.0: Escape markdown special characters in video titles to fix thread creation failures
//! - 3.14.0: Fix video download for URLs with playlist parameters by using clean video URLs
//! - 3.13.0: Improved Discord markdown formatting with headings, block quotes, and styled links
//! - 3.12.0: Display job ID in thread starter and final stats messages for easy reference
//! - 3.11.0: Renamed summary options for clarity: summary_style→summaries (each/periodic/all/none),
//!           transcript_interval→transcript_file_interval, added user-configurable summary_interval
//! - 3.10.0: Thread starter includes title, author, and URL; first thread message is description only
//! - 3.7.0: Refined transcription output: URL in thread starter for embed, distinct emojis
//!          (📜 transcripts, 💡 summaries), windowed middle summaries, chunk_summary_prompt
//! - 3.6.0: Added output_format parameter, improved transcription complete heading with word count
//! - 3.5.0: Added customizable chunk_duration, flexible playlist limits, video description upfront
//! - 3.4.0: Configurable summary options (summary_style, transcript_interval, custom_prompt),
//!          fixed overall/cumulative summary bug where content was passed incorrectly to AI
//! - 3.3.0: Added optional cumulative "story so far" summaries during chunked transcription
//! - 3.2.0: Added per-user AI usage tracking for plugin summary operations
//! - 3.1.0: Activate chunked transcription path, add per-chunk transcript files and AI summaries
//! - 3.0.0: Added chunked streaming transcription for long videos with progressive output
//! - 2.1.1: Use video title as thread starter message instead of generic "Transcription"
//! - 2.1.0: Thread-first output - ephemeral responses, minimal starter messages, content in thread
//! - 2.0.0: Added playlist support with multi-video transcription and progress tracking
//! - 1.5.0: If command used inside thread, append to existing thread instead of creating new
//! - 1.4.0: Consolidated to single post - interaction response becomes thread starter
//! - 1.3.0: Fetch YouTube title via oEmbed for thread name, simpler thread creation flow
//! - 1.2.0: Thread created immediately with URL, progress status posted before execution
//! - 1.1.0: Added structured output posting with source_param, thread from interaction response
//! - 1.0.0: Initial release with config-based plugins, CLI executor, and job system

pub mod chunker;
pub mod commands;
pub mod config;
pub mod executor;
pub mod job;
pub mod output;
pub mod youtube;

pub use chunker::{AudioChunker, ChunkProgress, ChunkStatus, ChunkerConfig};
pub use commands::create_plugins_command;
pub use config::{ChunkingConfig, Plugin, PluginConfig};
pub use executor::{ExecutionResult, PluginExecutor};
pub use job::{Job, JobManager, JobStatus, PlaylistJob, PlaylistJobStatus};
pub use output::{
    count_words, format_transcript_sentences, format_word_count, OutputFormat, OutputHandler,
    UserContext,
};
pub use youtube::{
    enumerate_playlist, fetch_video_metadata, format_description_preview, parse_youtube_url,
    PlaylistInfo, PlaylistItem, VideoMetadata, YouTubeUrl, YouTubeUrlType,
};

use crate::database::Database;

/// Get the first 8 characters of a job ID for display
pub fn short_job_id(id: &str) -> &str {
    if id.len() >= 8 {
        &id[..8]
    } else {
        id
    }
}
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
            return Err(anyhow::anyhow!("You are not authorized to use this plugin"));
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
        if plugin.security.cooldown_seconds > 0
            && !self.job_manager.check_cooldown(
                user_id,
                &plugin.name,
                plugin.security.cooldown_seconds,
            ) {
                return Err(anyhow::anyhow!(
                    "Please wait {} seconds before using this plugin again",
                    plugin.security.cooldown_seconds
                ));
            }
        Ok(())
    }

    /// Validate input parameters against plugin schema
    pub fn validate_params(&self, plugin: &Plugin, params: &HashMap<String, String>) -> Result<()> {
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
        let job_id = self
            .job_manager
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
        let user_id_clone = user_id.clone();
        let guild_id_clone = guild_id.clone();
        let channel_id_str = channel_id.to_string();

        tokio::spawn(async move {
            // Create user context for usage tracking
            let user_context = UserContext {
                user_id: user_id_clone,
                guild_id: guild_id_clone,
                channel_id: Some(channel_id_str),
            };
            // Mark as running
            if let Err(e) = job_manager.start_job(&job_id_clone).await {
                warn!("Failed to mark job as running: {e}");
            }

            // Determine source URL for structured output
            let source_url = plugin
                .output
                .source_param
                .as_ref()
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
                                info!("Fetched YouTube title: {title}");
                                title
                            }
                            None => {
                                let template = plugin
                                    .output
                                    .thread_name_template
                                    .as_deref()
                                    .unwrap_or("Plugin Output");
                                substitute_params(template, &params)
                            }
                        }
                    } else {
                        let template = plugin
                            .output
                            .thread_name_template
                            .as_deref()
                            .unwrap_or("Plugin Output");
                        substitute_params(template, &params)
                    }
                } else {
                    let template = plugin
                        .output
                        .thread_name_template
                        .as_deref()
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
                        "https://discord.com/api/v10/webhooks/{app_id}/{token}/messages/@original"
                    );
                    let _ = client
                        .patch(&edit_url)
                        .header("Content-Type", "application/json")
                        .json(&serde_json::json!({ "content": format!("Starting transcription for \"{}\"...", thread_name) }))
                        .send()
                        .await;
                }

                // Send thread starter message to channel with video title (this is what others see)
                let starter_content = if source_url
                    .as_ref()
                    .map(|u| u.contains("youtube.com") || u.contains("youtu.be"))
                    .unwrap_or(false)
                {
                    format!("Transcribing YouTube video: {thread_name}")
                } else {
                    thread_name.clone()
                };

                // Create thread from a new message (not the ephemeral response)
                let thread_channel = match channel_id.say(&http, starter_content).await {
                    Ok(msg) => {
                        match create_thread_with_retry(
                            &output_handler,
                            &http,
                            channel_id,
                            msg.id,
                            &thread_name,
                            plugin.output.auto_archive_minutes,
                            3,
                        )
                        .await
                        {
                            Ok(thread) => {
                                info!("Created thread: {} ({})", thread_name, thread.id);
                                job_manager.set_thread_id(&job_id_clone, thread.id.to_string());

                                // Post URL inside the thread (first message in thread)
                                let thread_id = ChannelId(thread.id.0);
                                if let Some(ref url) = source_url {
                                    let url_content = format!("**{thread_name}**\n{url}");
                                    let _ = thread_id.say(&http, &url_content).await;
                                }

                                Some(thread_id)
                            }
                            Err(e) => {
                                error!("Failed to create thread after retries: {e}");
                                finalize_interaction_response(
                                    &interaction_info,
                                    &format!(
                                        "Failed to create thread: {e}. Posting to channel instead."
                                    ),
                                )
                                .await;
                                None
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to send thread starter message: {e}");
                        finalize_interaction_response(
                            &interaction_info,
                            &format!("Failed to start transcription: {e}"),
                        )
                        .await;
                        None
                    }
                };

                thread_channel.unwrap_or(channel_id)
            } else if is_thread && plugin.output.create_thread {
                // Already in a thread - edit ephemeral response and post to this thread
                if let Some(ref url) = source_url {
                    // Fetch title for display
                    let title = if url.contains("youtube.com") || url.contains("youtu.be") {
                        fetch_youtube_title(url)
                            .await
                            .unwrap_or_else(|| "Video".to_string())
                    } else {
                        "Content".to_string()
                    };

                    // Edit ephemeral interaction response with confirmation (only user sees this)
                    if let Some((app_id, ref token)) = interaction_info {
                        let client = reqwest::Client::new();
                        let edit_url = format!(
                            "https://discord.com/api/v10/webhooks/{app_id}/{token}/messages/@original"
                        );
                        let _ = client
                            .patch(&edit_url)
                            .header("Content-Type", "application/json")
                            .json(&serde_json::json!({ "content": format!("Transcribing \"{}\" in this thread...", title) }))
                            .send()
                            .await;
                    }

                    // Post the URL to the thread (this is visible to everyone in the thread)
                    let content = format!("**{title}**\n{url}");
                    let _ = channel_id.say(&http, &content).await;
                }
                channel_id
            } else {
                channel_id
            };

            // STEP 2: Post progress status BEFORE execution
            // Post to thread (either new or existing)
            let should_post_status =
                plugin.output.create_thread && (is_thread || output_channel != channel_id);
            if should_post_status {
                let status_msg = "⏳ Transcribing video... This may take a few minutes.";
                if let Err(e) = output_channel.say(&http, status_msg).await {
                    warn!("Failed to post progress status: {e}");
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
                                .post_structured_result(
                                    &http,
                                    output_channel,
                                    url,
                                    &exec_result.stdout,
                                    &plugin.output,
                                    url_already_posted,
                                    Some(&user_context),
                                )
                                .await
                        } else {
                            output_handler
                                .post_result(
                                    &http,
                                    output_channel,
                                    &exec_result.stdout,
                                    &plugin.output,
                                    Some(&user_context),
                                )
                                .await
                        };

                        if let Err(e) = post_result {
                            error!("Failed to post result: {e}");
                        }

                        // Mark job complete
                        let preview = exec_result.stdout.chars().take(500).collect::<String>();
                        if let Err(e) = job_manager.complete_job(&job_id_clone, preview).await {
                            warn!("Failed to mark job complete: {e}");
                        }
                    } else {
                        // Command failed
                        let error_msg = if exec_result.timed_out {
                            format!(
                                "Command timed out after {} seconds",
                                plugin.execution.timeout_seconds
                            )
                        } else {
                            format!(
                                "Command failed (exit code: {:?})\n{}",
                                exec_result.exit_code, exec_result.stderr
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
                            error!("Failed to post error: {e}");
                        }

                        if let Err(e) = job_manager.fail_job(&job_id_clone, error_msg).await {
                            warn!("Failed to mark job as failed: {e}");
                        }
                    }
                }
                Err(e) => {
                    let error_msg = format!("Execution error: {e}");
                    if let Err(post_err) = output_handler
                        .post_error(
                            &http,
                            output_channel,
                            &error_msg,
                            plugin.output.error_template.as_deref(),
                        )
                        .await
                    {
                        error!("Failed to post error: {post_err}");
                    }

                    if let Err(fail_err) = job_manager.fail_job(&job_id_clone, error_msg).await {
                        warn!("Failed to mark job as failed: {fail_err}");
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

        // Apply limits: if max_videos_per_request is 0, no hard limit applies
        let effective_max = if playlist_config.max_videos_per_request > 0 {
            // Admin has set a hard limit
            max_videos
                .unwrap_or(playlist_config.default_max_videos)
                .min(playlist_config.max_videos_per_request)
        } else {
            // No hard limit - use user's value or default
            max_videos.unwrap_or(playlist_config.default_max_videos)
        };

        let videos: Vec<_> = playlist_info
            .items
            .into_iter()
            .take(effective_max as usize)
            .collect();
        let total_videos = videos.len() as u32;

        // Create playlist job record
        let playlist_job_id = self
            .job_manager
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
        let user_id_clone = user_id.clone();
        let guild_id_clone = guild_id.clone();
        let channel_id_str = channel_id.to_string();
        let chunking_config = plugin.execution.chunking.clone().unwrap_or_default();
        let max_output_bytes = plugin.execution.max_output_bytes;

        tokio::spawn(async move {
            // Create user context for usage tracking
            let user_context = UserContext {
                user_id: user_id_clone,
                guild_id: guild_id_clone,
                channel_id: Some(channel_id_str),
            };

            // Mark as running
            if let Err(e) = job_manager.start_playlist_job(&playlist_job_id_clone).await {
                warn!("Failed to mark playlist job as running: {e}");
            }

            // STEP 1: Create thread for playlist
            let thread_name = format!(
                "Playlist: {} ({} videos)",
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
                    "https://discord.com/api/v10/webhooks/{app_id}/{token}/messages/@original"
                );
                let _ = client
                    .patch(&edit_url)
                    .header("Content-Type", "application/json")
                    .json(&serde_json::json!({ "content": format!("Starting playlist transcription for \"{}\" ({} videos)...", playlist_title, total_videos) }))
                    .send()
                    .await;
            }

            // Send thread starter message to channel with playlist title and job ID
            let short_id = short_job_id(&playlist_job_id_clone);
            let starter_content = format!(
                "Transcribing YouTube playlist: {playlist_title} | job: {short_id}"
            );

            // Create thread from a new message (not the ephemeral response)
            let thread_channel = match channel_id.say(&http, starter_content).await {
                Ok(msg) => {
                    match create_thread_with_retry(
                        &output_handler,
                        &http,
                        channel_id,
                        msg.id,
                        &thread_name,
                        1440,
                        3,
                    )
                    .await
                    {
                        Ok(thread) => {
                            info!("Created playlist thread: {} ({})", thread_name, thread.id);
                            job_manager.set_playlist_thread_id(
                                &playlist_job_id_clone,
                                thread.id.to_string(),
                            );

                            // Post playlist URL inside the thread (first message in thread)
                            let thread_id = ChannelId(thread.id.0);
                            let url_content = format!("**{playlist_title}**\n{playlist_url}");
                            let _ = thread_id.say(&http, &url_content).await;

                            Some(thread_id)
                        }
                        Err(e) => {
                            error!("Failed to create playlist thread after retries: {e}");
                            finalize_interaction_response(
                                &interaction_info,
                                &format!(
                                    "Failed to create thread: {e}. Posting to channel instead."
                                ),
                            )
                            .await;
                            None
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to send thread starter message: {e}");
                    finalize_interaction_response(
                        &interaction_info,
                        &format!("Failed to start playlist transcription: {e}"),
                    )
                    .await;
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
                .post_playlist_progress(
                    &http,
                    output_channel,
                    None,
                    1,
                    total_videos,
                    &videos[0].title,
                    None,
                )
                .await
            {
                progress_message_id = Some(msg_id);
            }

            for (index, video) in videos.iter().enumerate() {
                let video_index = (index + 1) as u32;

                // Check for cancellation
                if job_manager.is_playlist_cancelled(&playlist_job_id_clone) {
                    info!(
                        "Playlist job {playlist_job_id_clone} cancelled, stopping at video {video_index}"
                    );
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
                            &http,
                            output_channel,
                            Some(msg_id),
                            video_index,
                            total_videos,
                            &video.title,
                            Some(eta),
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
                        warn!("Failed to create video job: {e}");
                        failed += 1;
                        continue;
                    }
                };

                // Update playlist progress with current video
                let _ = job_manager
                    .update_playlist_progress(
                        &playlist_job_id_clone,
                        completed,
                        failed,
                        skipped,
                        Some(&video_job_id),
                    )
                    .await;

                // Mark video job as running
                let _ = job_manager.start_job(&video_job_id).await;

                // Execute transcription using chunked download path
                // This ensures --no-playlist is used for each video
                let result = Self::transcribe_single_video(
                    &executor,
                    &chunking_config,
                    &video.url,
                    &params,
                    max_output_bytes,
                )
                .await;

                match result {
                    Ok(transcript) => {
                        // Post video result
                        if let Err(e) = output_handler
                            .post_video_result(
                                &http,
                                output_channel,
                                video_index,
                                total_videos,
                                &video.title,
                                &video.url,
                                &transcript,
                                &plugin.output,
                                Some(&user_context),
                            )
                            .await
                        {
                            warn!("Failed to post video result: {e}");
                        }

                        // Add to combined transcript
                        let separator = "=".repeat(60);
                        combined_transcript.push_str(&format!(
                            "\n\n{}\n[{}/{}] {}\n{}\n{}\n\n{}",
                            separator,
                            video_index,
                            total_videos,
                            video.title,
                            video.url,
                            separator,
                            transcript
                        ));

                        let _ = job_manager
                            .complete_job(&video_job_id, "completed".to_string())
                            .await;
                        completed += 1;
                    }
                    Err(e) => {
                        let _ = output_handler
                            .post_video_failed(
                                &http,
                                output_channel,
                                video_index,
                                total_videos,
                                &video.title,
                                &video.url,
                                &e.to_string(),
                            )
                            .await;

                        let _ = job_manager.fail_job(&video_job_id, e.to_string()).await;
                        failed += 1;
                    }
                }

                // Update playlist progress
                let _ = job_manager
                    .update_playlist_progress(
                        &playlist_job_id_clone,
                        completed,
                        failed,
                        skipped,
                        None,
                    )
                    .await;

                // Delay between videos to avoid rate limits
                if index < videos.len() - 1 {
                    tokio::time::sleep(std::time::Duration::from_secs(
                        playlist_config.min_video_interval_seconds,
                    ))
                    .await;
                }
            }

            // STEP 3: Post final summary
            let runtime = start_time.elapsed();

            if job_manager.is_playlist_cancelled(&playlist_job_id_clone) {
                // Was cancelled
                if let Some(job) = job_manager.get_playlist_job(&playlist_job_id_clone) {
                    let cancelled_by = job.cancelled_by.unwrap_or_else(|| "user".to_string());
                    let _ = output_handler
                        .post_playlist_cancelled(
                            &http,
                            output_channel,
                            completed,
                            total_videos,
                            &cancelled_by,
                        )
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
                        &http,
                        output_channel,
                        &playlist_title,
                        completed,
                        failed,
                        skipped,
                        total_videos,
                        runtime,
                        combined,
                    )
                    .await;
            }

            // Mark playlist job complete
            let _ = job_manager
                .complete_playlist_job(&playlist_job_id_clone)
                .await;

            info!(
                "Playlist job {playlist_job_id_clone} completed: {completed}/{total_videos} successful, {failed} failed"
            );
        });

        Ok(playlist_job_id)
    }

    /// Transcribe a single video using the chunked download path
    ///
    /// This helper function is used by playlist transcription to process individual videos.
    /// It downloads the audio using yt-dlp with --no-playlist (via the chunking config),
    /// then transcribes the local file. This ensures that even when processing videos
    /// from a playlist, each video is downloaded individually without yt-dlp accidentally
    /// re-expanding the playlist.
    ///
    /// Returns the transcript text on success, or an error message on failure.
    async fn transcribe_single_video(
        executor: &PluginExecutor,
        chunking_config: &ChunkingConfig,
        url: &str,
        params: &HashMap<String, String>,
        max_output_bytes: usize,
    ) -> Result<String> {
        // Parse URL to get a clean video URL without playlist parameters
        // This prevents issues where yt-dlp might extract the wrong ID
        let download_url = match parse_youtube_url(url) {
            Ok(parsed) => parsed.video_url().unwrap_or_else(|| url.to_string()),
            Err(_) => url.to_string(), // Fall back to original URL if parsing fails
        };

        // Create chunker with the chunking config (which uses --no-playlist in download_args)
        let chunker_config = ChunkerConfig {
            chunk_duration_secs: chunking_config.chunk_duration_secs,
            download_timeout_secs: chunking_config.download_timeout_secs,
            split_timeout_secs: 120,
            download_command: chunking_config.download_command.clone(),
            download_args: chunking_config.download_args.clone(),
        };

        let chunker = AudioChunker::new(chunker_config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize chunker: {}", e))?;

        // Download audio
        let download_result = match chunker.download_audio(&download_url).await {
            Ok(r) => r,
            Err(e) => {
                let _ = chunker.cleanup().await;
                return Err(anyhow::anyhow!("Failed to download audio: {}", e));
            }
        };

        // Execute transcription on the downloaded file
        let result = executor
            .execute_on_file(
                chunking_config,
                &download_result.audio_path,
                chunker.temp_dir(),
                max_output_bytes,
                params,
            )
            .await;

        // Clean up temp files
        let _ = chunker.cleanup().await;

        match result {
            Ok(exec_result) => {
                if exec_result.success && !exec_result.stdout.is_empty() {
                    Ok(exec_result.stdout)
                } else if exec_result.timed_out {
                    Err(anyhow::anyhow!("Transcription timed out"))
                } else {
                    Err(anyhow::anyhow!(
                        "Transcription failed: {}",
                        exec_result.stderr
                    ))
                }
            }
            Err(e) => Err(anyhow::anyhow!("Transcription error: {}", e)),
        }
    }

    /// Execute a chunked transcription for a long video
    ///
    /// This method downloads the audio, splits it into chunks, and transcribes each chunk
    /// progressively, posting results to the Discord thread as they complete.
    /// Each chunk gets its own transcript file and AI summary, then a final overall summary
    /// is generated from all chunk summaries.
    pub async fn execute_chunked_transcription(
        &self,
        http: Arc<Http>,
        plugin: Plugin,
        url: String,
        video_title: String,
        params: HashMap<String, String>,
        user_id: String,
        guild_id: Option<String>,
        channel_id: ChannelId,
        interaction_info: Option<(u64, String)>,
        is_thread: bool,
    ) -> Result<String> {
        let chunking_config = plugin.execution.chunking.clone().unwrap_or_default();

        // Create job record - merge passed params with job-specific params
        let mut job_params = params.clone();
        job_params.insert("url".to_string(), url.clone());
        job_params.insert("mode".to_string(), "chunked".to_string());

        let job_id = self
            .job_manager
            .create_job(
                &plugin.name,
                &user_id,
                guild_id.as_deref(),
                &channel_id.to_string(),
                job_params,
            )
            .await?;

        let job_manager = self.job_manager.clone();
        let executor = self.executor.clone();
        let output_handler = self.output_handler.clone();
        let job_id_clone = job_id.clone();
        let params = params.clone(); // Clone params for the spawned task
        let user_id_clone = user_id.clone();
        let guild_id_clone = guild_id.clone();
        let channel_id_str = channel_id.to_string();

        tokio::spawn(async move {
            // Create user context for usage tracking
            let user_context = UserContext {
                user_id: user_id_clone,
                guild_id: guild_id_clone,
                channel_id: Some(channel_id_str),
            };

            // Extract user options from params
            // "summaries" replaces "summary_style" with clearer naming:
            //   each = per-chunk summaries, periodic = windowed combined, all = both, none = no summaries
            let summaries = params
                .get("summaries")
                .map(|s| s.as_str())
                .unwrap_or("each");
            // "summary_interval" controls chunks between periodic summaries (default: 5, range: 2-20)
            let summary_interval: u32 = params
                .get("summary_interval")
                .and_then(|s| s.parse().ok())
                .unwrap_or(5)
                .max(2)
                .min(20);
            // "transcript_file_interval" replaces "transcript_interval" for clarity
            let transcript_file_interval: usize = params
                .get("transcript_file_interval")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let custom_prompt = params.get("custom_prompt").cloned();

            // Extract and validate chunk_duration (convert minutes to seconds, clamp to 5-30 min)
            let chunk_duration_secs: u64 = params
                .get("chunk_duration")
                .and_then(|s| s.parse::<u64>().ok())
                .map(|mins| mins * 60) // Convert minutes to seconds
                .unwrap_or(chunking_config.chunk_duration_secs)
                .max(60) // Minimum 1 minute
                .min(1800); // Maximum 30 minutes

            // Extract output_format (text, files, or auto)
            let output_format = params
                .get("output_format")
                .map(|s| OutputFormat::from_str(s))
                .unwrap_or_default();

            // Determine what summaries to generate based on summaries option
            // "each" = per-chunk, "periodic" = windowed combined, "all" = both, "none" = none
            let generate_per_chunk = matches!(summaries, "each" | "all");
            let generate_cumulative = matches!(summaries, "periodic" | "all");

            // Mark as running
            if let Err(e) = job_manager.start_job(&job_id_clone).await {
                warn!("Failed to mark job as running: {e}");
            }

            // STEP 1: Create thread IMMEDIATELY if configured
            let output_channel = if plugin.output.create_thread && !is_thread {
                // Truncate thread name to 100 chars (Discord limit)
                let thread_name = if video_title.len() > 100 {
                    format!("{}...", &video_title[..97])
                } else {
                    video_title.clone()
                };

                // Edit ephemeral interaction response with confirmation
                if let Some((app_id, ref token)) = interaction_info {
                    let client = reqwest::Client::new();
                    let edit_url = format!(
                        "https://discord.com/api/v10/webhooks/{app_id}/{token}/messages/@original"
                    );
                    let _ = client
                        .patch(&edit_url)
                        .header("Content-Type", "application/json")
                        .json(&serde_json::json!({ "content": format!("Starting chunked transcription for \"{}\"...", thread_name) }))
                        .send()
                        .await;
                }

                // Fetch video metadata early for thread starter and description
                let metadata = youtube::fetch_video_metadata(&url).await.ok();

                // Send thread starter message to channel
                let short_id = short_job_id(&job_id_clone);
                let starter_content = if let Some(ref meta) = metadata {
                    if let Some(ref uploader) = meta.uploader {
                        format!(
                            "## YouTube: {thread_name} - by {uploader}\nTranscription job: `{short_id}`\n\n{url}"
                        )
                    } else {
                        format!(
                            "## YouTube: {thread_name}\nTranscription job: `{short_id}`\n\n{url}"
                        )
                    }
                } else {
                    format!(
                        "## YouTube: {thread_name}\nTranscription job: `{short_id}`\n\n{url}"
                    )
                };

                // Create thread from a new message
                let thread_channel = match channel_id.say(&http, starter_content).await {
                    Ok(msg) => {
                        match create_thread_with_retry(
                            &output_handler,
                            &http,
                            channel_id,
                            msg.id,
                            &thread_name,
                            plugin.output.auto_archive_minutes,
                            3,
                        )
                        .await
                        {
                            Ok(thread) => {
                                info!(
                                    "Created thread for chunked transcription: {} ({})",
                                    thread_name, thread.id
                                );
                                job_manager.set_thread_id(&job_id_clone, thread.id.to_string());

                                let thread_id = ChannelId(thread.id.0);

                                // First thread message: video description ("doobily doo")
                                if let Some(ref meta) = metadata {
                                    if let Some(ref desc) = meta.description {
                                        if !desc.is_empty() {
                                            let preview =
                                                youtube::format_description_preview(desc, 10);
                                            let desc_msg =
                                                format!("**Description:**\n>>> {preview}");
                                            let _ = thread_id.say(&http, &desc_msg).await;
                                        }
                                    }
                                }

                                Some(thread_id)
                            }
                            Err(e) => {
                                error!("Failed to create thread after retries: {e}");
                                finalize_interaction_response(
                                    &interaction_info,
                                    &format!(
                                        "Failed to create thread: {e}. Posting to channel instead."
                                    ),
                                )
                                .await;
                                None
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to send thread starter message: {e}");
                        finalize_interaction_response(
                            &interaction_info,
                            &format!("Failed to start chunked transcription: {e}"),
                        )
                        .await;
                        None
                    }
                };

                thread_channel.unwrap_or(channel_id)
            } else if is_thread && plugin.output.create_thread {
                // Already in a thread - edit ephemeral response and post to this thread
                if let Some((app_id, ref token)) = interaction_info {
                    let client = reqwest::Client::new();
                    let edit_url = format!(
                        "https://discord.com/api/v10/webhooks/{app_id}/{token}/messages/@original"
                    );
                    let _ = client
                        .patch(&edit_url)
                        .header("Content-Type", "application/json")
                        .json(&serde_json::json!({ "content": format!("Chunked transcription of \"{}\" in this thread...", video_title) }))
                        .send()
                        .await;
                }

                // Fetch video metadata for header and description
                let metadata = youtube::fetch_video_metadata(&url).await.ok();

                // Post header message
                let short_id = short_job_id(&job_id_clone);
                let header_content = if let Some(ref meta) = metadata {
                    if let Some(ref uploader) = meta.uploader {
                        format!(
                            "## YouTube: {video_title} - by {uploader}\nTranscription job: `{short_id}`\n\n{url}"
                        )
                    } else {
                        format!(
                            "## YouTube: {video_title}\nTranscription job: `{short_id}`\n\n{url}"
                        )
                    }
                } else {
                    format!(
                        "## YouTube: {video_title}\nTranscription job: `{short_id}`\n\n{url}"
                    )
                };
                let _ = channel_id.say(&http, &header_content).await;

                // Post video description
                if let Some(ref meta) = metadata {
                    if let Some(ref desc) = meta.description {
                        if !desc.is_empty() {
                            let preview = youtube::format_description_preview(desc, 10);
                            let desc_msg = format!("**Description:**\n>>> {preview}");
                            let _ = channel_id.say(&http, &desc_msg).await;
                        }
                    }
                }

                channel_id
            } else {
                channel_id
            };

            let start_time = std::time::Instant::now();

            // STEP 2: Post initial status - downloading
            let progress_msg_id = output_handler
                .post_chunking_started(&http, output_channel, &video_title)
                .await
                .ok();

            // STEP 3: Create chunker and download audio
            let chunker_config = ChunkerConfig {
                chunk_duration_secs, // Use user-specified value (already clamped)
                download_timeout_secs: chunking_config.download_timeout_secs,
                split_timeout_secs: 120,
                download_command: chunking_config.download_command.clone(),
                download_args: chunking_config.download_args.clone(),
            };

            let chunker = match AudioChunker::new(chunker_config).await {
                Ok(c) => c,
                Err(e) => {
                    let error_msg = format!("Failed to initialize chunker: {e}");
                    let _ = output_handler
                        .post_error(
                            &http,
                            output_channel,
                            &error_msg,
                            plugin.output.error_template.as_deref(),
                        )
                        .await;
                    let _ = job_manager.fail_job(&job_id_clone, error_msg).await;
                    return;
                }
            };

            // Parse URL to get a clean video URL without playlist parameters
            // This prevents issues where yt-dlp or the container might extract the wrong ID
            let download_url = match parse_youtube_url(&url) {
                Ok(parsed) => parsed.video_url().unwrap_or_else(|| url.clone()),
                Err(_) => url.clone(), // Fall back to original URL if parsing fails
            };

            let download_result = match chunker.download_audio(&download_url).await {
                Ok(r) => r,
                Err(e) => {
                    let error_msg = format!("Failed to download audio: {e}");
                    let _ = output_handler
                        .post_error(
                            &http,
                            output_channel,
                            &error_msg,
                            plugin.output.error_template.as_deref(),
                        )
                        .await;
                    let _ = job_manager.fail_job(&job_id_clone, error_msg).await;
                    let _ = chunker.cleanup().await;
                    return;
                }
            };

            // STEP 4: Check if chunking is needed
            let needs_chunking = chunker
                .needs_chunking(&download_result.audio_path)
                .await
                .unwrap_or(true);

            if !needs_chunking {
                // Audio is short enough - use standard execution
                info!("Audio doesn't need chunking, using standard execution");

                if let Some(msg_id) = progress_msg_id {
                    let _ = output_channel
                        .edit_message(&http, msg_id, |m| {
                            m.content("⏳ Transcribing short video...")
                        })
                        .await;
                }

                // Execute standard transcription on the downloaded file
                let result = executor
                    .execute_on_file(
                        &chunking_config,
                        &download_result.audio_path,
                        chunker.temp_dir(),
                        plugin.execution.max_output_bytes,
                        &params,
                    )
                    .await;

                let _ = chunker.cleanup().await;

                match result {
                    Ok(exec_result) => {
                        if exec_result.success {
                            let _ = output_handler
                                .post_structured_result(
                                    &http,
                                    output_channel,
                                    &url,
                                    &exec_result.stdout,
                                    &plugin.output,
                                    true,
                                    Some(&user_context),
                                )
                                .await;
                            let _ = job_manager
                                .complete_job(&job_id_clone, "completed".to_string())
                                .await;
                        } else {
                            let error_msg = if exec_result.timed_out {
                                "Transcription timed out".to_string()
                            } else {
                                exec_result.stderr
                            };
                            let _ = output_handler
                                .post_error(
                                    &http,
                                    output_channel,
                                    &error_msg,
                                    plugin.output.error_template.as_deref(),
                                )
                                .await;
                            let _ = job_manager.fail_job(&job_id_clone, error_msg).await;
                        }
                    }
                    Err(e) => {
                        let _ = output_handler
                            .post_error(
                                &http,
                                output_channel,
                                &e.to_string(),
                                plugin.output.error_template.as_deref(),
                            )
                            .await;
                        let _ = job_manager.fail_job(&job_id_clone, e.to_string()).await;
                    }
                }
                return;
            }

            // STEP 5: Split audio into chunks
            let split_result = match chunker.split_into_chunks(&download_result.audio_path).await {
                Ok(r) => r,
                Err(e) => {
                    let error_msg = format!("Failed to split audio: {e}");
                    let _ = output_handler
                        .post_error(
                            &http,
                            output_channel,
                            &error_msg,
                            plugin.output.error_template.as_deref(),
                        )
                        .await;
                    let _ = job_manager.fail_job(&job_id_clone, error_msg).await;
                    let _ = chunker.cleanup().await;
                    return;
                }
            };

            let total_chunks = split_result.total_chunks;

            // Estimate total time (assume 5 minutes per chunk max)
            let estimated_duration = std::time::Duration::from_secs(
                (total_chunks as u64) * chunking_config.chunk_timeout_secs / 2,
            );

            // Post chunks ready status
            let _ = output_handler
                .post_chunks_ready(
                    &http,
                    output_channel,
                    progress_msg_id,
                    total_chunks,
                    Some(estimated_duration),
                )
                .await;

            // STEP 6: Process chunks sequentially with progress updates
            let mut completed_chunks = 0usize;
            let mut failed_chunks = 0usize;
            let mut combined_transcript = String::new();
            let mut chunk_summaries: Vec<String> = Vec::new();
            let mut last_summary_chunk: usize = 0; // Track last chunk included in a cumulative summary
            let mut progress_message_id: Option<serenity::model::id::MessageId> = None;

            for (index, chunk_path) in split_result.chunk_paths.iter().enumerate() {
                let chunk_num = index + 1;

                // Calculate ETA
                let elapsed = start_time.elapsed();
                let avg_time_per_chunk = if index > 0 {
                    elapsed / (index as u32)
                } else {
                    std::time::Duration::from_secs(chunking_config.chunk_timeout_secs / 2)
                };
                let remaining_chunks = total_chunks - chunk_num;
                let eta = avg_time_per_chunk * (remaining_chunks as u32);

                // Update progress
                if let Ok(msg_id) = output_handler
                    .post_chunk_progress(
                        &http,
                        output_channel,
                        progress_message_id,
                        chunk_num,
                        total_chunks,
                        "Processing...",
                        Some(eta),
                    )
                    .await
                {
                    progress_message_id = Some(msg_id);
                }

                // Execute transcription on this chunk
                let result = executor
                    .execute_on_file(
                        &chunking_config,
                        chunk_path,
                        chunker.temp_dir(),
                        plugin.execution.max_output_bytes,
                        &params,
                    )
                    .await;

                match result {
                    Ok(exec_result) => {
                        if exec_result.success && !exec_result.stdout.is_empty() {
                            // Success - post chunk transcript based on output_format
                            let chunk_content = &exec_result.stdout;
                            if output_format.should_use_file(chunk_content.len()) {
                                // Format transcript with sentences on separate lines
                                let formatted = format_transcript_sentences(chunk_content);
                                let chunk_filename =
                                    format!("part-{chunk_num}-of-{total_chunks}.txt");
                                let _ = output_handler
                                    .post_file(&http, output_channel, &formatted, &chunk_filename)
                                    .await;
                            } else {
                                // Post as text message with block quote formatting
                                let msg = format!(
                                    "### 📜 Part {chunk_num}/{total_chunks}\n\n>>> {chunk_content}"
                                );
                                let _ = output_channel.say(&http, &msg).await;
                            }

                            // Generate chunk summary for accumulation (needed for periodic or overall)
                            // Skip entirely if summaries is "none"
                            if summaries != "none" {
                                // Use chunk_summary_prompt if available, fallback to summary_prompt
                                let prompt_to_use = plugin
                                    .output
                                    .chunk_summary_prompt
                                    .as_ref()
                                    .or(plugin.output.summary_prompt.as_ref());

                                if let Some(base_prompt) = prompt_to_use {
                                    // Build prompt with custom instructions if provided
                                    let full_prompt = if let Some(ref custom) = custom_prompt {
                                        format!(
                                            "{base_prompt}\n\nAdditional instructions: {custom}"
                                        )
                                    } else {
                                        base_prompt.clone()
                                    };

                                    if let Some(chunk_summary) = output_handler
                                        .generate_summary_for_text_with_context(
                                            &exec_result.stdout,
                                            &full_prompt,
                                            Some(&user_context),
                                            Some("chunk_summary"),
                                        )
                                        .await
                                    {
                                        // Always collect summaries for overall summary generation
                                        chunk_summaries.push(chunk_summary.clone());

                                        // Post per-chunk summary if enabled
                                        if generate_per_chunk {
                                            let summary_msg = format!(
                                                "### 💡 Summary (Part {chunk_num}/{total_chunks})\n\n{chunk_summary}"
                                            );
                                            let _ = output_channel.say(&http, &summary_msg).await;
                                        }

                                        // Generate periodic windowed summary if enabled (covers only chunks since last summary)
                                        // Uses user's summary_interval parameter (default: 5, range: 2-20)
                                        if generate_cumulative
                                            && chunk_summaries.len() > 1
                                            && chunk_num as u32 % summary_interval == 0
                                        {
                                            // Use windowed summaries: only summarize chunks since last summary
                                            let start_idx = last_summary_chunk;
                                            let end_idx = chunk_summaries.len();
                                            let window_summaries =
                                                &chunk_summaries[start_idx..end_idx];
                                            let summaries_window =
                                                window_summaries.join("\n\n---\n\n");

                                            // Calculate part range for display (1-indexed)
                                            let start_part = start_idx + 1;
                                            let end_part = chunk_num;

                                            let base_template = format!(
                                                "Summarize what was covered in parts {start_part}-{end_part} of this video transcript. \
                                                Be conversational - capture the main topics and key points discussed. \
                                                No formal structure or conclusions needed.\n\nSection summaries:\n${{output}}"
                                            );

                                            // Add custom instructions to windowed summary too
                                            let cumulative_template =
                                                if let Some(ref custom) = custom_prompt {
                                                    format!(
                                                        "{base_template}\n\nAdditional instructions: {custom}"
                                                    )
                                                } else {
                                                    base_template
                                                };

                                            if let Some(cumulative_summary) = output_handler
                                                .generate_summary_for_text_with_context(
                                                    &summaries_window,
                                                    &cumulative_template,
                                                    Some(&user_context),
                                                    Some("cumulative_summary"),
                                                )
                                                .await
                                            {
                                                let cumulative_msg = format!(
                                                    "### 💡 Summary (Parts {start_part}-{end_part})\n\n{cumulative_summary}"
                                                );
                                                let _ = output_channel
                                                    .say(&http, &cumulative_msg)
                                                    .await;
                                                info!(
                                                    "Posted windowed summary for parts {start_part}-{end_part}"
                                                );

                                                // Update last_summary_chunk to current position
                                                last_summary_chunk = chunk_summaries.len();
                                            }
                                        }
                                    }
                                }
                            }

                            // Add to combined transcript
                            if !combined_transcript.is_empty() {
                                combined_transcript.push_str("\n\n");
                            }
                            combined_transcript.push_str(&format!(
                                "--- Part {}/{} ---\n{}",
                                chunk_num, total_chunks, exec_result.stdout
                            ));

                            // Post transcript file at interval if configured (after adding chunk)
                            if transcript_file_interval > 0
                                && chunk_num % transcript_file_interval == 0
                            {
                                let partial_filename =
                                    format!("transcript_parts_1-{chunk_num}.txt");
                                // Format with sentences on separate lines
                                let formatted = format_transcript_sentences(&combined_transcript);
                                let _ = output_handler
                                    .post_file(&http, output_channel, &formatted, &partial_filename)
                                    .await;
                                let _ = output_channel
                                    .say(
                                        &http,
                                        &format!(
                                            "📜 **Partial transcript** (parts 1-{}, {} words)",
                                            chunk_num,
                                            count_words(&combined_transcript)
                                        ),
                                    )
                                    .await;
                            }

                            completed_chunks += 1;
                        } else {
                            // Failed
                            let error_msg = if exec_result.timed_out {
                                "Chunk timed out".to_string()
                            } else {
                                exec_result.stderr
                            };

                            let _ = output_handler
                                .post_chunk_failed(
                                    &http,
                                    output_channel,
                                    chunk_num,
                                    total_chunks,
                                    &error_msg,
                                )
                                .await;

                            failed_chunks += 1;
                        }
                    }
                    Err(e) => {
                        let _ = output_handler
                            .post_chunk_failed(
                                &http,
                                output_channel,
                                chunk_num,
                                total_chunks,
                                &e.to_string(),
                            )
                            .await;
                        failed_chunks += 1;
                    }
                }
            }

            // STEP 7: Post final summary
            let runtime = start_time.elapsed();
            let status_emoji = if failed_chunks == 0 { "📝" } else { "⚠️" };
            let runtime_str = crate::features::plugins::youtube::format_duration(runtime);
            let word_count = count_words(&combined_transcript);
            let word_count_str = format_word_count(word_count);

            // Post final stats with improved heading, including job ID for reference
            let short_id = short_job_id(&job_id_clone);
            let title_display = if video_title.len() > 60 {
                format!("{}...", &video_title[..57])
            } else {
                video_title.clone()
            };
            let stats_msg = format!(
                "---\n\n## {status_emoji} Transcription Complete: {title_display}\nJob: `{short_id}`\n\n\
                 **Stats:** {completed_chunks}/{total_chunks} parts • {word_count_str} words • **Runtime:** {runtime_str}\n\n{url}"
            );
            let _ = output_channel.say(&http, &stats_msg).await;

            // Generate final overall summary from chunk summaries (skip if "none" style)
            if !chunk_summaries.is_empty() && summaries != "none" {
                let combined_summaries = chunk_summaries.join("\n\n---\n\n");
                let base_template = "Based on these section summaries from a longer video, \
                    provide a comprehensive overall summary that synthesizes the key themes, \
                    main points, and conclusions across all sections:\n\n${output}";

                // Add custom instructions to overall summary if provided
                let overall_template = if let Some(ref custom) = custom_prompt {
                    format!("{base_template}\n\nAdditional instructions: {custom}")
                } else {
                    base_template.to_string()
                };

                if let Some(final_summary) = output_handler
                    .generate_summary_for_text_with_context(
                        &combined_summaries,
                        &overall_template,
                        Some(&user_context),
                        Some("overall_summary"),
                    )
                    .await
                {
                    let _ = output_channel
                        .say(
                            &http,
                            &format!("### 💡 Overall Summary\n\n{final_summary}"),
                        )
                        .await;
                }
            }

            // Post full transcript based on output_format
            if !combined_transcript.is_empty() {
                if output_format.should_use_file(combined_transcript.len()) {
                    let filename = plugin
                        .output
                        .file_name_template
                        .as_deref()
                        .unwrap_or("transcript.txt")
                        .replace(
                            "${timestamp}",
                            &chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string(),
                        );
                    // Format transcript with sentences on separate lines
                    let formatted = format_transcript_sentences(&combined_transcript);
                    let _ = output_handler
                        .post_file(&http, output_channel, &formatted, &filename)
                        .await;
                    let _ = output_channel
                        .say(
                            &http,
                            &format!("📜 **Full transcript** ({word_count} words)"),
                        )
                        .await;
                } else {
                    // Post as text messages (split if needed)
                    let _ = output_channel.say(&http, "📜 **Full Transcript:**").await;
                    // Split into 1900-char chunks to stay under Discord limit
                    let transcript_chunks: Vec<&str> = combined_transcript
                        .as_bytes()
                        .chunks(1900)
                        .map(|c| std::str::from_utf8(c).unwrap_or(""))
                        .collect();
                    for chunk in transcript_chunks {
                        let _ = output_channel.say(&http, chunk).await;
                    }
                }
            }

            // STEP 8: Cleanup and complete job
            let _ = chunker.cleanup().await;

            if failed_chunks == 0 {
                let _ = job_manager
                    .complete_job(
                        &job_id_clone,
                        format!("Completed {completed_chunks} parts"),
                    )
                    .await;
            } else {
                let _ = job_manager
                    .fail_job(
                        &job_id_clone,
                        format!("{failed_chunks}/{total_chunks} parts failed"),
                    )
                    .await;
            }

            info!(
                "Chunked transcription {job_id_clone} completed: {completed_chunks}/{total_chunks} successful, {failed_chunks} failed, runtime: {runtime:?}"
            );
        });

        Ok(job_id)
    }

    /// Determine if a video should use chunked transcription
    ///
    /// Returns true if chunking is enabled and configured for this plugin.
    pub fn should_use_chunking(&self, plugin: &Plugin) -> bool {
        plugin
            .execution
            .chunking
            .as_ref()
            .map(|c| c.enabled)
            .unwrap_or(false)
    }
}

/// Create a thread with retry logic for rate limiting
///
/// Retries thread creation with exponential backoff to handle Discord rate limits
/// that can occur when multiple threads are created in rapid succession.
async fn create_thread_with_retry(
    output_handler: &OutputHandler,
    http: &Arc<Http>,
    channel_id: ChannelId,
    message_id: serenity::model::id::MessageId,
    thread_name: &str,
    auto_archive_minutes: u64,
    max_retries: u32,
) -> Result<serenity::model::channel::GuildChannel> {
    let mut last_error = None;
    for attempt in 0..max_retries {
        if attempt > 0 {
            let delay = std::time::Duration::from_secs(2u64.pow(attempt));
            info!(
                "Retrying thread creation after {:?} (attempt {})",
                delay,
                attempt + 1
            );
            tokio::time::sleep(delay).await;
        }

        match output_handler
            .create_output_thread(
                http,
                channel_id,
                message_id,
                thread_name,
                auto_archive_minutes,
            )
            .await
        {
            Ok(thread) => return Ok(thread),
            Err(e) => {
                warn!("Thread creation attempt {} failed: {}", attempt + 1, e);
                last_error = Some(e);
            }
        }
    }
    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("Thread creation failed after {} retries", max_retries)))
}

/// Finalize the ephemeral interaction response
///
/// Updates the ephemeral "thinking" message to show the final status.
/// This ensures users always see feedback even if thread creation fails.
async fn finalize_interaction_response(interaction_info: &Option<(u64, String)>, message: &str) {
    if let Some((app_id, token)) = interaction_info {
        let client = reqwest::Client::new();
        let edit_url = format!(
            "https://discord.com/api/v10/webhooks/{app_id}/{token}/messages/@original"
        );
        let _ = client
            .patch(&edit_url)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "content": message }))
            .send()
            .await;
    }
}

/// Substitute ${param} placeholders in a string
fn substitute_params(template: &str, params: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in params {
        let placeholder = format!("${{{key}}}");
        result = result.replace(&placeholder, value);
    }
    result
}

/// Fetch YouTube video title via oEmbed API
pub async fn fetch_youtube_title(url: &str) -> Option<String> {
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
            warn!("Failed to fetch YouTube title: {e}");
            None
        }
    }
}
