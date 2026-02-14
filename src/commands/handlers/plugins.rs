//! Plugin command handler
//!
//! Handles: plugins (with subcommands for each plugin)
//!
//! - **Version**: 1.0.0
//! - **Since**: 4.0.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation - migrated from command_handler.rs plugin dispatch

use anyhow::Result;
use async_trait::async_trait;
use log::{error, info, warn};
use serenity::model::application::interaction::application_command::{
    ApplicationCommandInteraction, CommandDataOption,
};
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::commands::context::CommandContext;
use crate::commands::handler::SlashCommandHandler;
use crate::features::plugins::{short_job_id, PluginManager};

/// Handler for all plugin commands via /plugins <subcommand>
pub struct PluginsHandler;

#[async_trait]
impl SlashCommandHandler for PluginsHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["plugins"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();

        let plugin_manager = match ctx.plugin_manager {
            Some(ref pm) => pm.clone(),
            None => {
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content("Plugin system is not configured.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
        };

        // Extract subcommand name and its options
        let (subcommand_name, sub_options) = match extract_subcommand(&command.data.options) {
            Some(result) => result,
            None => {
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content(
                                        "Please specify a plugin subcommand. Use `/plugins` to see available options.",
                                    )
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
        };

        // Find the plugin matching the subcommand
        let plugin = match plugin_manager
            .config
            .plugins
            .iter()
            .find(|p| p.enabled && p.command.name == subcommand_name)
        {
            Some(p) => p.clone(),
            None => {
                warn!("[{request_id}] Unknown plugin subcommand: {subcommand_name}");
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content("Unknown plugin command.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
        };

        self.handle_plugin_command(
            &ctx,
            serenity_ctx,
            command,
            plugin,
            plugin_manager,
            sub_options,
            request_id,
        )
        .await
    }
}

impl PluginsHandler {
    /// Handle a plugin-based slash command
    #[allow(clippy::too_many_arguments)]
    async fn handle_plugin_command(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        plugin: crate::features::plugins::Plugin,
        plugin_manager: Arc<PluginManager>,
        sub_options: Vec<CommandDataOption>,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        info!(
            "[{}] 🔌 Processing plugin command: {} | User: {} | Plugin: {}",
            request_id, plugin.command.name, user_id, plugin.name
        );

        // Check guild_only restriction
        if plugin.security.guild_only && guild_id.is_none() {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .content("This command can only be used in a server, not in DMs.")
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        // Check cooldown
        if plugin.security.cooldown_seconds > 0
            && !plugin_manager.job_manager.check_cooldown(
                &user_id,
                &plugin.name,
                plugin.security.cooldown_seconds,
            ) {
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content(format!(
                                    "Please wait before using `{}` again. Cooldown: {} seconds.",
                                    plugin.command.name, plugin.security.cooldown_seconds
                                ))
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }

        // Extract command parameters from subcommand options
        let mut params = extract_params(&sub_options);

        // Add defaults for missing optional parameters
        for opt_def in &plugin.command.options {
            if !params.contains_key(&opt_def.name) {
                if let Some(ref default) = opt_def.default {
                    params.insert(opt_def.name.clone(), default.clone());
                }
            }
        }

        // Validate parameters
        for opt_def in &plugin.command.options {
            if let Some(value) = params.get(&opt_def.name) {
                if let Some(ref validation) = opt_def.validation {
                    // Check pattern
                    if let Some(ref pattern) = validation.pattern {
                        let re = regex::Regex::new(pattern)?;
                        if !re.is_match(value) {
                            command
                                .create_interaction_response(&serenity_ctx.http, |response| {
                                    response
                                        .kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content(format!(
                                                "Invalid value for `{}`: doesn't match expected format.",
                                                opt_def.name
                                            ))
                                            .ephemeral(true)
                                        })
                                })
                                .await?;
                            return Ok(());
                        }
                    }
                    // Check length constraints
                    if let Some(min_len) = validation.min_length {
                        if value.len() < min_len {
                            command
                                .create_interaction_response(&serenity_ctx.http, |response| {
                                    response
                                        .kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content(format!(
                                                "Value for `{}` is too short (minimum {} characters).",
                                                opt_def.name, min_len
                                            ))
                                            .ephemeral(true)
                                        })
                                })
                                .await?;
                            return Ok(());
                        }
                    }
                    if let Some(max_len) = validation.max_length {
                        if value.len() > max_len {
                            command
                                .create_interaction_response(&serenity_ctx.http, |response| {
                                    response
                                        .kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content(format!(
                                                "Value for `{}` is too long (maximum {} characters).",
                                                opt_def.name, max_len
                                            ))
                                            .ephemeral(true)
                                        })
                                })
                                .await?;
                            return Ok(());
                        }
                    }
                }
            } else if opt_def.required {
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content(format!(
                                        "Missing required parameter: `{}`.",
                                        opt_def.name
                                    ))
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
        }

        // Check if plugins feature is enabled for this guild
        if let Some(ref gid) = guild_id {
            let enabled = ctx
                .database
                .is_feature_enabled("plugins", None, Some(gid))
                .await?;
            if !enabled {
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content("Plugin commands are disabled in this server.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
        }

        // Handle virtual plugins (no CLI execution, handled internally)
        if plugin.is_virtual() {
            info!(
                "[{}] 🔧 Handling virtual plugin: {}",
                request_id, plugin.command.name
            );
            return self
                .handle_virtual_plugin(
                    serenity_ctx,
                    command,
                    &plugin,
                    &plugin_manager,
                    &params,
                    &user_id,
                    request_id,
                )
                .await;
        }

        // Defer the response (command will take a while)
        // Use ephemeral response so only the thread appears in the channel
        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::DeferredChannelMessageWithSource)
                    .interaction_response_data(|data| data.ephemeral(true))
            })
            .await?;

        info!(
            "[{}] ⏳ Deferred response for plugin command: {}",
            request_id, plugin.command.name
        );

        // Clone values needed for the background task
        let http = serenity_ctx.http.clone();
        let plugin_manager = plugin_manager.clone();
        let plugin = plugin.clone();
        let discord_channel_id = command.channel_id;
        let interaction_token = command.token.clone();
        let application_id = command.application_id.0;
        let user_id_owned = user_id.clone();
        let guild_id_owned = guild_id.clone();

        // Check if we're already in a thread
        let is_thread = match discord_channel_id.to_channel(&serenity_ctx.http).await {
            Ok(channel) => {
                use serenity::model::channel::Channel;
                matches!(channel, Channel::Guild(gc) if gc.kind.name() == "public_thread" || gc.kind.name() == "private_thread")
            }
            Err(_) => false,
        };

        // Spawn background task to execute the plugin
        // Pass interaction info so the thread can be created from the interaction response
        let interaction_info = Some((application_id, interaction_token.clone()));

        tokio::spawn(async move {
            // Check if this should use chunked transcription
            let use_chunking = plugin_manager.should_use_chunking(&plugin);
            let is_youtube = params
                .get("url")
                .map(|u| u.contains("youtube.com") || u.contains("youtu.be"))
                .unwrap_or(false);
            let is_playlist = params
                .get("url")
                .map(|u| u.contains("playlist?list=") || u.contains("&list="))
                .unwrap_or(false);

            let result = if use_chunking && is_youtube && !is_playlist {
                // Use chunked transcription for YouTube videos (not playlists)
                let url = params.get("url").cloned().unwrap_or_default();
                let video_title = crate::features::plugins::fetch_youtube_title(&url)
                    .await
                    .unwrap_or_else(|| "Video".to_string());

                info!(
                    "[{request_id}] 📦 Using chunked transcription for: {video_title}"
                );

                plugin_manager
                    .execute_chunked_transcription(
                        http,
                        plugin.clone(),
                        url,
                        video_title,
                        params,
                        user_id_owned,
                        guild_id_owned,
                        discord_channel_id,
                        interaction_info,
                        is_thread,
                    )
                    .await
            } else {
                // Use regular execution
                plugin_manager
                    .execute_plugin(
                        http,
                        plugin.clone(),
                        params,
                        user_id_owned,
                        guild_id_owned,
                        discord_channel_id,
                        interaction_info,
                        is_thread,
                    )
                    .await
            };

            match result {
                Ok(job_id) => {
                    info!(
                        "[{}] ✅ Plugin job started: {} (job_id: {})",
                        request_id, plugin.name, job_id
                    );
                }
                Err(e) => {
                    error!(
                        "[{}] ❌ Plugin execution failed: {} - {}",
                        request_id, plugin.name, e
                    );

                    // Edit the deferred response with error
                    let edit_url = format!(
                        "https://discord.com/api/v10/webhooks/{application_id}/{interaction_token}/messages/@original"
                    );

                    let client = reqwest::Client::new();
                    let _ = client
                        .patch(&edit_url)
                        .header("Content-Type", "application/json")
                        .json(&serde_json::json!({
                            "content": format!("❌ Command failed: {}", e)
                        }))
                        .send()
                        .await;
                }
            }
        });

        Ok(())
    }

    /// Handle virtual plugins (commands handled internally without CLI execution)
    #[allow(clippy::too_many_arguments)]
    async fn handle_virtual_plugin(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        plugin: &crate::features::plugins::Plugin,
        plugin_manager: &Arc<PluginManager>,
        params: &HashMap<String, String>,
        user_id: &str,
        request_id: Uuid,
    ) -> Result<()> {
        match plugin.command.name.as_str() {
            "transcribe_cancel" => {
                self.handle_transcribe_cancel(
                    ctx,
                    command,
                    plugin_manager,
                    params,
                    user_id,
                    request_id,
                )
                .await
            }
            "transcribe_status" => {
                self.handle_transcribe_status(ctx, command, plugin_manager, user_id, request_id)
                    .await
            }
            _ => {
                // Unknown virtual plugin
                warn!(
                    "[{}] ❓ Unknown virtual plugin: {}",
                    request_id, plugin.command.name
                );
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content("This command is not yet implemented.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                Ok(())
            }
        }
    }

    /// Handle /plugins transcribe_cancel command - cancel an active transcription job
    async fn handle_transcribe_cancel(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        plugin_manager: &Arc<PluginManager>,
        params: &HashMap<String, String>,
        user_id: &str,
        request_id: Uuid,
    ) -> Result<()> {
        info!(
            "[{request_id}] 🛑 Processing transcribe_cancel for user {user_id}"
        );

        // Get optional job_id parameter
        let job_id_param = params.get("job_id").cloned();

        // Find the job to cancel
        let job_to_cancel = if let Some(job_id) = job_id_param {
            // User specified a job ID - look for it
            // Try to find by full ID or short ID prefix
            let active_jobs = plugin_manager
                .job_manager
                .get_user_active_playlist_jobs(user_id);
            active_jobs
                .into_iter()
                .find(|j| j.id == job_id || j.id.starts_with(&job_id))
        } else {
            // No job ID specified - get user's most recent active job
            let active_jobs = plugin_manager
                .job_manager
                .get_user_active_playlist_jobs(user_id);
            active_jobs.into_iter().next()
        };

        match job_to_cancel {
            Some(job) => {
                let job_id = job.id.clone();
                let job_title = job
                    .playlist_title
                    .clone()
                    .unwrap_or_else(|| "Untitled".to_string());

                // Cancel the job
                match plugin_manager
                    .job_manager
                    .cancel_playlist_job(&job_id, user_id)
                    .await
                {
                    Ok(true) => {
                        info!(
                            "[{request_id}] ✅ Cancelled playlist job {job_id} for user {user_id}"
                        );
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message
                                            .content(format!(
                                                "✅ Cancelled transcription job `{}` ({})\n\
                                             Progress: {}/{} videos completed",
                                                short_job_id(&job_id),
                                                job_title,
                                                job.completed_videos,
                                                job.total_videos
                                            ))
                                            .ephemeral(true)
                                    })
                            })
                            .await?;
                    }
                    Ok(false) => {
                        // Job wasn't active (already completed or cancelled)
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message
                                            .content(format!(
                                                "⚠️ Job `{}` is no longer active (status: {})",
                                                short_job_id(&job_id),
                                                job.status
                                            ))
                                            .ephemeral(true)
                                    })
                            })
                            .await?;
                    }
                    Err(e) => {
                        error!("[{request_id}] ❌ Failed to cancel job {job_id}: {e}");
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message
                                            .content(format!("❌ Failed to cancel job: {e}"))
                                            .ephemeral(true)
                                    })
                            })
                            .await?;
                    }
                }
            }
            None => {
                // No active job found
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content(
                                        "❌ No active transcription job found to cancel.\n\
                                                 Use `/plugins transcribe_status` to view your jobs.",
                                    )
                                    .ephemeral(true)
                            })
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle /plugins transcribe_status command - show user's transcription jobs
    async fn handle_transcribe_status(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        plugin_manager: &Arc<PluginManager>,
        user_id: &str,
        request_id: Uuid,
    ) -> Result<()> {
        info!(
            "[{request_id}] 📊 Processing transcribe_status for user {user_id}"
        );

        // Get user's active playlist jobs
        let active_jobs = plugin_manager
            .job_manager
            .get_user_active_playlist_jobs(user_id);

        // Get user's regular (single video) jobs
        let all_jobs = plugin_manager.job_manager.get_user_jobs(user_id);
        let active_video_jobs: Vec<_> = all_jobs
            .into_iter()
            .filter(|j| {
                matches!(
                    j.status,
                    crate::features::plugins::JobStatus::Running
                        | crate::features::plugins::JobStatus::Pending
                )
            })
            .collect();

        if active_jobs.is_empty() && active_video_jobs.is_empty() {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .content(
                                    "📭 You have no active transcription jobs.\n\
                                            Use `/plugins transcribe <url>` to start a transcription.",
                                )
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        // Build status message
        let mut status_lines = vec!["**Your Transcription Jobs:**".to_string()];

        if !active_jobs.is_empty() {
            status_lines.push("\n**Playlist Jobs:**".to_string());
            for job in &active_jobs {
                let progress_pct = job.progress_percent();
                let title = job.playlist_title.as_deref().unwrap_or("Untitled playlist");
                let truncated_title = if title.len() > 40 {
                    format!("{}...", &title[..37])
                } else {
                    title.to_string()
                };
                status_lines.push(format!(
                    "• `{}` \"{}\" - {}/{} videos ({:.0}%)",
                    short_job_id(&job.id),
                    truncated_title,
                    job.completed_videos + job.failed_videos,
                    job.total_videos,
                    progress_pct
                ));
            }
        }

        if !active_video_jobs.is_empty() {
            status_lines.push("\n**Video Jobs:**".to_string());
            for job in &active_video_jobs {
                let url = job
                    .params
                    .get("url")
                    .map(|u| {
                        if u.len() > 50 {
                            format!("{}...", &u[..47])
                        } else {
                            u.clone()
                        }
                    })
                    .unwrap_or_else(|| "Unknown".to_string());
                let status = match job.status {
                    crate::features::plugins::JobStatus::Running => "🔄 Running",
                    crate::features::plugins::JobStatus::Pending => "⏳ Pending",
                    _ => "❓ Unknown",
                };
                status_lines.push(format!(
                    "• `{}` {} - {}",
                    short_job_id(&job.id),
                    url,
                    status
                ));
            }
        }

        status_lines
            .push("\n*To cancel a job, use `/plugins transcribe_cancel [job_id]`*".to_string());

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(status_lines.join("\n")).ephemeral(true)
                    })
            })
            .await?;

        Ok(())
    }
}

/// Extract subcommand name and its nested options from the top-level command options
fn extract_subcommand(options: &[CommandDataOption]) -> Option<(String, Vec<CommandDataOption>)> {
    options.first().map(|opt| {
        (
            opt.name.clone(),
            opt.options.clone(),
        )
    })
}

/// Extract parameters as a HashMap from subcommand options
fn extract_params(options: &[CommandDataOption]) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for opt in options {
        if let Some(value) = &opt.value {
            let value_str = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string().trim_matches('"').to_string(),
            };
            params.insert(opt.name.clone(), value_str);
        }
    }
    params
}
