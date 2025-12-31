//! # Feature: Plugin System
//!
//! Extensible plugin architecture for executing CLI commands via Discord slash commands.
//! Supports YAML-based configuration, secure execution, background jobs, and thread-based output.
//!
//! - **Version**: 1.5.0
//! - **Since**: 0.9.0
//! - **Toggleable**: true
//!
//! ## Changelog
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

pub use commands::create_plugin_commands;
pub use config::{Plugin, PluginConfig};
pub use executor::{ExecutionResult, PluginExecutor};
pub use job::{Job, JobManager, JobStatus};
pub use output::OutputHandler;

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

                // Format the starter message: **Video Title**\nURL
                let starter_content = if let Some(ref url) = source_url {
                    format!("**{}**\n{}", thread_name, url)
                } else {
                    format!("🔄 `{}` output", plugin.command.name)
                };

                // Use interaction response as thread starter (one consolidated post)
                let thread_channel = if let Some((app_id, ref token)) = interaction_info {
                    let client = reqwest::Client::new();
                    let edit_url = format!(
                        "https://discord.com/api/v10/webhooks/{}/{}/messages/@original",
                        app_id, token
                    );

                    // Edit the deferred response to show the URL (this becomes thread starter)
                    let edit_resp = client
                        .patch(&edit_url)
                        .header("Content-Type", "application/json")
                        .json(&serde_json::json!({ "content": starter_content }))
                        .send()
                        .await;

                    match edit_resp {
                        Ok(resp) => {
                            match resp.json::<serde_json::Value>().await {
                                Ok(msg_json) => {
                                    if let Some(msg_id_str) = msg_json.get("id").and_then(|v| v.as_str()) {
                                        if let Ok(msg_id) = msg_id_str.parse::<u64>() {
                                            let message_id = serenity::model::id::MessageId(msg_id);
                                            info!("Edited interaction response, creating thread from message {}", msg_id);

                                            match output_handler
                                                .create_output_thread(&http, channel_id, message_id, &thread_name, plugin.output.auto_archive_minutes)
                                                .await
                                            {
                                                Ok(thread) => {
                                                    info!("Created thread: {} ({})", thread_name, thread.id);
                                                    job_manager.set_thread_id(&job_id_clone, thread.id.to_string());
                                                    Some(ChannelId(thread.id.0))
                                                }
                                                Err(e) => {
                                                    error!("Failed to create thread from interaction: {}", e);
                                                    None
                                                }
                                            }
                                        } else {
                                            error!("Failed to parse message ID");
                                            None
                                        }
                                    } else {
                                        error!("No message ID in response: {:?}", msg_json);
                                        None
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to parse edit response: {}", e);
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to edit interaction response: {}", e);
                            None
                        }
                    }
                } else {
                    // Fallback: send new message and create thread from it
                    match channel_id.say(&http, &starter_content).await {
                        Ok(msg) => {
                            match output_handler
                                .create_output_thread(&http, channel_id, msg.id, &thread_name, plugin.output.auto_archive_minutes)
                                .await
                            {
                                Ok(thread) => {
                                    job_manager.set_thread_id(&job_id_clone, thread.id.to_string());
                                    Some(ChannelId(thread.id.0))
                                }
                                Err(e) => {
                                    error!("Failed to create thread: {}", e);
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to send message: {}", e);
                            None
                        }
                    }
                };

                thread_channel.unwrap_or(channel_id)
            } else if is_thread && plugin.output.create_thread {
                // Already in a thread - post the URL here and edit interaction response
                if let Some(ref url) = source_url {
                    // Fetch title for display
                    let title = if url.contains("youtube.com") || url.contains("youtu.be") {
                        fetch_youtube_title(url).await.unwrap_or_else(|| "Video".to_string())
                    } else {
                        "Content".to_string()
                    };
                    let content = format!("**{}**\n{}", title, url);

                    // Edit interaction response to show we're posting to this thread
                    if let Some((app_id, ref token)) = interaction_info {
                        let client = reqwest::Client::new();
                        let edit_url = format!(
                            "https://discord.com/api/v10/webhooks/{}/{}/messages/@original",
                            app_id, token
                        );
                        let _ = client
                            .patch(&edit_url)
                            .header("Content-Type", "application/json")
                            .json(&serde_json::json!({ "content": content }))
                            .send()
                            .await;
                    } else {
                        // No interaction, just post the URL
                        let _ = channel_id.say(&http, &content).await;
                    }
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
