//! # Feature: Plugin System
//!
//! Extensible plugin architecture for executing CLI commands via Discord slash commands.
//! Supports YAML-based configuration, secure execution, background jobs, and thread-based output.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.9.0
//! - **Toggleable**: true
//!
//! ## Changelog
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
    pub async fn execute_plugin(
        &self,
        http: Arc<Http>,
        plugin: Plugin,
        params: HashMap<String, String>,
        user_id: String,
        guild_id: Option<String>,
        channel_id: ChannelId,
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

            // Create output thread if configured
            let output_channel = if plugin.output.create_thread {
                let thread_name = plugin
                    .output
                    .thread_name_template
                    .as_deref()
                    .unwrap_or("Plugin Output")
                    .to_string();

                // Substitute params in thread name
                let thread_name = substitute_params(&thread_name, &params);

                // Truncate thread name to 100 chars (Discord limit)
                let thread_name = if thread_name.len() > 100 {
                    format!("{}...", &thread_name[..97])
                } else {
                    thread_name
                };

                // Create a starter message and thread from it
                let starter_msg = format!("🔄 Processing `{}` command...", plugin.command.name);
                match output_handler
                    .create_thread_with_starter(
                        &http,
                        channel_id,
                        &thread_name,
                        &starter_msg,
                        plugin.output.auto_archive_minutes,
                    )
                    .await
                {
                    Ok(thread) => {
                        job_manager.set_thread_id(&job_id_clone, thread.id.to_string());
                        ChannelId(thread.id.0)
                    }
                    Err(e) => {
                        warn!("Failed to create thread, using channel: {}", e);
                        channel_id
                    }
                }
            } else {
                channel_id
            };

            // Execute the command
            let result = executor.execute(&plugin.execution, &params).await;

            match result {
                Ok(exec_result) => {
                    if exec_result.success {
                        // Post successful output
                        if let Err(e) = output_handler
                            .post_result(&http, output_channel, &exec_result.stdout, &plugin.output)
                            .await
                        {
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
