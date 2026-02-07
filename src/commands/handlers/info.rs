//! Info/analytics command handler
//!
//! Handles: introspect, commits, features, toggle, sysinfo, usage, dm_stats, session_history
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from command_handler.rs

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, error, info, warn};
use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::sync::Arc;
use uuid::Uuid;

use crate::commands::context::CommandContext;
use crate::commands::handler::SlashCommandHandler;
use crate::commands::slash::{get_integer_option, get_string_option};
use crate::features::analytics::CostBucket;
use crate::features::introspection::get_component_snippet;

/// Handler for info/analytics commands: introspect, commits, features, toggle,
/// sysinfo, usage, dm_stats, session_history
pub struct InfoHandler;

#[async_trait]
impl SlashCommandHandler for InfoHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &[
            "introspect",
            "commits",
            "features",
            "toggle",
            "sysinfo",
            "usage",
            "dm_stats",
            "session_history",
        ]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();
        match command.data.name.as_str() {
            "introspect" => self.handle_introspect(&ctx, serenity_ctx, command, request_id).await,
            "commits" => self.handle_commits(&ctx, serenity_ctx, command, request_id).await,
            "features" => self.handle_features(&ctx, serenity_ctx, command, request_id).await,
            "toggle" => self.handle_toggle(&ctx, serenity_ctx, command, request_id).await,
            "sysinfo" => self.handle_sysinfo(&ctx, serenity_ctx, command, request_id).await,
            "usage" => self.handle_usage(&ctx, serenity_ctx, command, request_id).await,
            "dm_stats" => self.handle_dm_stats(serenity_ctx, command, request_id, &ctx).await,
            "session_history" => {
                self.handle_session_history(serenity_ctx, command, request_id, &ctx)
                    .await
            }
            _ => Ok(()),
        }
    }
}

impl InfoHandler {
    // ── introspect ──────────────────────────────────────────────────────

    /// Handle /introspect command - explain bot internals via OpenAI
    async fn handle_introspect(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        let component = get_string_option(&command.data.options, "component")
            .ok_or_else(|| anyhow::anyhow!("Missing component parameter"))?;

        info!("[{request_id}] Introspect requested for component: {component} by user: {user_id}");

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        let persona_name = if let Some(gid) = &guild_id {
            ctx.database
                .get_persona_with_channel(&user_id, gid, &channel_id)
                .await?
        } else {
            ctx.database.get_user_persona(&user_id).await?
        };

        let (component_title, code_snippet) = get_component_snippet(&component);
        let persona = ctx.persona_manager.get_persona(&persona_name);
        let persona_prompt = persona.map(|p| p.system_prompt.as_str()).unwrap_or("");

        let introspection_prompt = format!(
            "{persona_prompt}\n\n\
            You are now being asked to explain your own implementation. \
            The user wants to understand how you work internally.\n\n\
            Here is actual code from your implementation - {component_title}:\n\n\
            ```rust\n{code_snippet}\n```\n\n\
            Explain this code in your characteristic style and personality. \
            Use metaphors and analogies that fit your character. \
            Make it entertaining and educational. \
            Keep it conversational, not too technical. \
            Aim for 2-3 paragraphs."
        );

        let chat_completion = ChatCompletion::builder(
            &ctx.openai_model,
            vec![
                ChatCompletionMessage {
                    role: ChatCompletionMessageRole::System,
                    content: Some(introspection_prompt),
                    name: None,
                    function_call: None,
                    tool_call_id: None,
                    tool_calls: None,
                },
                ChatCompletionMessage {
                    role: ChatCompletionMessageRole::User,
                    content: Some(format!(
                        "Explain how your {component_title} system works, in your own words."
                    )),
                    name: None,
                    function_call: None,
                    tool_call_id: None,
                    tool_calls: None,
                },
            ],
        )
        .create()
        .await;

        let channel_id_str = command.channel_id.to_string();

        let response = match chat_completion {
            Ok(completion) => {
                if let Some(usage) = &completion.usage {
                    ctx.usage_tracker.log_chat(
                        &ctx.openai_model,
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        usage.total_tokens,
                        &user_id,
                        guild_id.as_deref(),
                        Some(&channel_id_str),
                        Some(&request_id.to_string()),
                        CostBucket::Introspect,
                    );
                }
                completion
                    .choices
                    .first()
                    .and_then(|choice| choice.message.content.clone())
                    .unwrap_or_else(|| {
                        "I seem to be having trouble reflecting on myself right now.".to_string()
                    })
            }
            Err(e) => {
                warn!("[{request_id}] OpenAI error during introspection: {e}");
                format!(
                    "I encountered an error while attempting to explain my {component} system: {e}"
                )
            }
        };

        command
            .edit_original_interaction_response(&serenity_ctx.http, |msg| {
                msg.content(format!(
                    "## Introspection: {component_title}\n\n{response}"
                ))
            })
            .await?;

        ctx.database
            .log_usage(&user_id, "introspect", Some(&persona_name))
            .await?;
        info!("[{request_id}] Introspection complete for component: {component}");
        Ok(())
    }

    // ── commits ─────────────────────────────────────────────────────────

    /// Handle /commits command - show recent git commits
    async fn handle_commits(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::features::startup::notification::{
            format_commit_for_thread, get_detailed_commits, get_github_repo_url,
        };
        use serenity::model::channel::ChannelType;

        let user_id = command.user.id.to_string();
        let is_dm = command.guild_id.is_none();

        let count = get_integer_option(&command.data.options, "count").unwrap_or(1) as usize;
        let count = count.clamp(1, 10);

        let commits = get_detailed_commits(count).await;
        let repo_url = get_github_repo_url().await;

        if commits.is_empty() {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| msg.content("No commit history available."))
                })
                .await?;
            return Ok(());
        }

        let mut summary = String::new();
        for commit in &commits {
            let hash_display = if let Some(ref url) = repo_url {
                format!("[`{}`]({}/commit/{})", commit.hash, url, commit.hash)
            } else {
                format!("`{}`", commit.hash)
            };
            summary.push_str(&format!(
                "\u{2022} **{}** ({})\n",
                commit.subject, hash_display
            ));
        }

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|msg| {
                        msg.embed(|e| {
                            e.title(format!("Recent Commits ({})", commits.len()))
                                .description(&summary)
                                .color(0x57F287)
                        })
                    })
            })
            .await?;

        if !is_dm {
            if let Ok(msg) = command.get_interaction_response(&serenity_ctx.http).await {
                match command
                    .channel_id
                    .create_public_thread(&serenity_ctx.http, msg.id, |t| {
                        t.name("Commit Details")
                            .kind(ChannelType::PublicThread)
                            .auto_archive_duration(60)
                    })
                    .await
                {
                    Ok(thread) => {
                        info!(
                            "[{request_id}] Created thread '{}' for commit details",
                            thread.name()
                        );
                        for commit in &commits {
                            let formatted =
                                format_commit_for_thread(commit, repo_url.as_deref());
                            if let Err(e) = thread.say(&serenity_ctx.http, &formatted).await {
                                warn!(
                                    "[{request_id}] Failed to post commit {} to thread: {}",
                                    commit.hash, e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            "[{request_id}] Failed to create thread for commit details: {e}"
                        );
                    }
                }
            }
        }

        ctx.database.log_usage(&user_id, "commits", None).await?;
        info!("[{request_id}] Commits command completed");
        Ok(())
    }

    // ── features ────────────────────────────────────────────────────────

    /// Handle /features command - list feature flags
    async fn handle_features(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        let flags = if let Some(ref gid) = guild_id {
            ctx.database
                .get_guild_feature_flags(gid)
                .await
                .unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };

        let mut output = format!(
            "**Bot Features** (v{})\n\n",
            crate::features::get_bot_version()
        );
        output.push_str("```\n");
        output.push_str("Feature              Version  Status  Toggleable\n");
        output.push_str("\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");

        for feature in crate::features::get_features() {
            let enabled = flags.get(feature.id).copied().unwrap_or(true);
            let status_str = if enabled { "ON " } else { "OFF" };
            let toggle_str = if feature.toggleable { "Yes" } else { "No " };
            output.push_str(&format!(
                "{:<20} {:<8} {}  {}\n",
                feature.name, feature.version, status_str, toggle_str
            ));
        }

        output.push_str("```\n");
        output.push_str("Use `/toggle <feature>` to enable/disable toggleable features.");

        command
            .create_interaction_response(&serenity_ctx.http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(output))
            })
            .await?;

        ctx.database.log_usage(&user_id, "features", None).await?;
        info!("[{request_id}] Features command completed");
        Ok(())
    }

    // ── toggle ──────────────────────────────────────────────────────────

    /// Handle /toggle command - enable/disable feature flags
    async fn handle_toggle(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        let feature_id = get_string_option(&command.data.options, "feature")
            .ok_or_else(|| anyhow::anyhow!("Missing feature parameter"))?;

        let feature = crate::features::get_feature(&feature_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown feature: {}", feature_id))?;

        if !feature.toggleable {
            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.content(format!(
                                "**{}** cannot be toggled. It's a core feature.",
                                feature.name
                            ))
                        })
                })
                .await?;
            return Ok(());
        }

        let guild_id_str = guild_id.as_deref().unwrap_or("");
        let current_enabled = ctx
            .database
            .is_feature_enabled(&feature_id, None, Some(guild_id_str))
            .await?;
        let new_enabled = !current_enabled;

        ctx.database
            .set_feature_flag(&feature_id, new_enabled, None, Some(guild_id_str))
            .await?;
        ctx.database
            .record_feature_toggle(
                &feature_id,
                feature.version,
                Some(guild_id_str),
                &user_id,
                new_enabled,
            )
            .await?;

        let status = if new_enabled { "enabled" } else { "disabled" };
        let response = format!(
            "**{}** has been {}.\n\nFeature: {} v{}",
            feature.name, status, feature.id, feature.version
        );

        command
            .create_interaction_response(&serenity_ctx.http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(response))
            })
            .await?;

        ctx.database.log_usage(&user_id, "toggle", None).await?;
        info!("[{request_id}] Toggle command completed: {feature_id} -> {new_enabled}");
        Ok(())
    }

    // ── sysinfo ─────────────────────────────────────────────────────────

    /// Handle /sysinfo command - display system metrics
    async fn handle_sysinfo(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::features::analytics::system_info::{
            format_history, CurrentMetrics, HistoricalSummary,
        };

        let user_id = command.user.id.to_string();
        let view = get_string_option(&command.data.options, "view")
            .unwrap_or_else(|| "current".to_string());

        info!("[{request_id}] Sysinfo requested: view={view}");

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        let response = match view.as_str() {
            "history_24h" | "history_7d" => {
                let hours = if view == "history_24h" { 24 } else { 168 };
                let period_label = if view == "history_24h" { "24h" } else { "7d" };

                let db_size_data = ctx
                    .database
                    .get_metrics_history("db_size_bytes", hours)
                    .await?;
                let bot_memory_data = ctx
                    .database
                    .get_metrics_history("bot_memory_bytes", hours)
                    .await?;
                let system_memory_data = ctx
                    .database
                    .get_metrics_history("system_memory_percent", hours)
                    .await?;
                let system_cpu_data = ctx
                    .database
                    .get_metrics_history("system_cpu_percent", hours)
                    .await?;

                let db_size = HistoricalSummary::from_data(&db_size_data);
                let bot_memory = HistoricalSummary::from_data(&bot_memory_data);
                let system_memory = HistoricalSummary::from_data(&system_memory_data);
                let system_cpu = HistoricalSummary::from_data(&system_cpu_data);

                format_history(db_size, bot_memory, system_memory, system_cpu, period_label)
            }
            _ => {
                let mut sys = sysinfo::System::new();
                sys.refresh_cpu_usage();
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                sys.refresh_cpu_usage();
                sys.refresh_memory();

                if let Ok(pid) = sysinfo::get_current_pid() {
                    sys.refresh_processes_specifics(
                        sysinfo::ProcessesToUpdate::Some(&[pid]),
                        true,
                        sysinfo::ProcessRefreshKind::new().with_memory(),
                    );
                }

                let db_path = std::env::var("DATABASE_PATH")
                    .unwrap_or_else(|_| "persona.db".to_string());
                let metrics = CurrentMetrics::gather(&sys, &db_path);
                let bot_uptime_secs = ctx.start_time.elapsed().as_secs();
                metrics.format(bot_uptime_secs)
            }
        };

        command
            .edit_original_interaction_response(&serenity_ctx.http, |msg| msg.content(response))
            .await?;

        ctx.database.log_usage(&user_id, "sysinfo", None).await?;
        info!("[{request_id}] Sysinfo command completed");
        Ok(())
    }

    // ── usage ───────────────────────────────────────────────────────────

    /// Handle /usage command - display usage statistics
    async fn handle_usage(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        let scope = get_string_option(&command.data.options, "scope")
            .unwrap_or_else(|| "personal_today".to_string());

        info!("[{request_id}] Usage requested: scope={scope}");

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        let response = match scope.as_str() {
            "personal_today" => {
                let stats = ctx.database.get_user_usage_stats(&user_id, 1).await?;
                Self::format_usage_stats("Your Usage Today", &stats, None)
            }
            "personal_7d" => {
                let stats = ctx.database.get_user_usage_stats(&user_id, 7).await?;
                Self::format_usage_stats("Your Usage (7 days)", &stats, None)
            }
            "server_today" => {
                if let Some(gid) = &guild_id {
                    let stats = ctx.database.get_guild_usage_stats(gid, 1).await?;
                    Self::format_usage_stats("Server Usage Today", &stats, None)
                } else {
                    "Server usage is only available in guild channels.".to_string()
                }
            }
            "server_7d" => {
                if let Some(gid) = &guild_id {
                    let stats = ctx.database.get_guild_usage_stats(gid, 7).await?;
                    Self::format_usage_stats("Server Usage (7 days)", &stats, None)
                } else {
                    "Server usage is only available in guild channels.".to_string()
                }
            }
            "top_users" => {
                if let Some(gid) = &guild_id {
                    let top_users = ctx
                        .database
                        .get_guild_top_users_by_cost(gid, 7, 10)
                        .await?;
                    Self::format_top_users("Top Users by Cost (7 days)", &top_users)
                } else {
                    "Top users is only available in guild channels.".to_string()
                }
            }
            _ => "Invalid scope. Please select a valid option.".to_string(),
        };

        command
            .edit_original_interaction_response(&serenity_ctx.http, |msg| msg.content(response))
            .await?;

        ctx.database.log_usage(&user_id, "usage", None).await?;
        info!("[{request_id}] Usage command completed");
        Ok(())
    }

    /// Format usage statistics into a Discord-friendly string
    fn format_usage_stats(
        title: &str,
        stats: &[(String, i64, i64, f64, i64, f64)],
        _extra_info: Option<&str>,
    ) -> String {
        if stats.is_empty() {
            return format!("**{title}**\n\nNo usage recorded for this period.");
        }

        let mut total_requests: i64 = 0;
        let mut total_tokens: i64 = 0;
        let mut total_audio_secs: f64 = 0.0;
        let mut total_images: i64 = 0;
        let mut total_cost: f64 = 0.0;

        let mut lines = vec![format!("**{title}**\n")];

        for (service_type, requests, tokens, audio_secs, images, cost) in stats {
            total_requests += requests;
            total_cost += cost;

            let details = match service_type.as_str() {
                "chat" => {
                    total_tokens += tokens;
                    format!(
                        "**Chat (GPT)**: {requests} requests, {tokens} tokens, ${cost:.4}"
                    )
                }
                "whisper" => {
                    total_audio_secs += audio_secs;
                    let mins = audio_secs / 60.0;
                    format!(
                        "**Audio (Whisper)**: {requests} requests, {mins:.1} minutes, ${cost:.4}"
                    )
                }
                "dalle" => {
                    total_images += images;
                    format!(
                        "**Images (DALL-E)**: {requests} requests, {images} images, ${cost:.4}"
                    )
                }
                _ => format!("**{service_type}**: {requests} requests, ${cost:.4}"),
            };
            lines.push(details);
        }

        lines.push(String::new());
        lines.push(format!(
            "**Total**: {total_requests} requests, ${total_cost:.4} estimated cost"
        ));

        if total_tokens > 0 {
            lines.push(format!("Total tokens: {total_tokens}"));
        }
        if total_audio_secs > 0.0 {
            lines.push(format!("{:.1} minutes transcribed", total_audio_secs / 60.0));
        }
        if total_images > 0 {
            lines.push(format!("{total_images} images generated"));
        }

        lines.join("\n")
    }

    /// Format top users into a Discord-friendly string
    fn format_top_users(title: &str, top_users: &[(String, i64, f64)]) -> String {
        if top_users.is_empty() {
            return format!("**{title}**\n\nNo usage recorded for this period.");
        }

        let mut lines = vec![format!("**{title}**\n")];

        for (i, (user_id, requests, cost)) in top_users.iter().enumerate() {
            let medal = match i {
                0 => "1.",
                1 => "2.",
                2 => "3.",
                _ => "  ",
            };
            lines.push(format!(
                "{medal} <@{user_id}>: {requests} requests, ${cost:.4}"
            ));
        }

        lines.join("\n")
    }

    // ── dm_stats ────────────────────────────────────────────────────────

    /// Handle /dm_stats command - show DM session statistics
    async fn handle_dm_stats(
        &self,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
        ctx: &CommandContext,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        let period = get_string_option(&command.data.options, "period")
            .unwrap_or_else(|| "week".to_string());

        let days = match period.as_str() {
            "today" => 1,
            "week" => 7,
            "month" => 30,
            "all" => 36500,
            _ => 7,
        };

        let period_display = match period.as_str() {
            "today" => "Today",
            "week" => "This Week",
            "month" => "This Month",
            "all" => "All Time",
            _ => "This Week",
        };

        debug!(
            "[{request_id}] Fetching DM stats for user {user_id} (period: {period}, days: {days})"
        );

        match ctx.database.get_user_dm_stats(&user_id, days).await {
            Ok(stats) => {
                let response = if stats.session_count == 0 {
                    format!(
                        "You don't have any DM sessions recorded for {}.",
                        period_display.to_lowercase()
                    )
                } else {
                    let duration_str = if stats.avg_session_duration_min < 1.0 {
                        format!("{:.0}s", stats.avg_session_duration_min * 60.0)
                    } else {
                        format!("{:.1}m", stats.avg_session_duration_min)
                    };

                    let response_time_str = if stats.avg_response_time_ms < 1000 {
                        format!("{}ms", stats.avg_response_time_ms)
                    } else {
                        format!("{:.1}s", stats.avg_response_time_ms as f64 / 1000.0)
                    };

                    format!(
                        "**Your DM Statistics ({})**\n\n\
                        Sessions: {} conversations\n\
                        Messages: {} sent, {} received\n\
                        Avg Session: {}\n\
                        Avg Response Time: {}\n\n\
                        **API Usage**\n\
                        Chat: {} calls, {}K tokens\n\
                        Audio: {} transcriptions\n\
                        Total Cost: ${:.4}\n\n\
                        **Feature Usage**\n\
                        Slash Commands: {}",
                        period_display,
                        stats.session_count,
                        stats.user_messages,
                        stats.bot_messages,
                        duration_str,
                        response_time_str,
                        stats.chat_calls,
                        stats.total_tokens / 1000,
                        stats.whisper_calls,
                        stats.total_cost_usd,
                        stats.slash_commands_used
                    )
                };

                command
                    .create_interaction_response(&serenity_ctx.http, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content(&response).ephemeral(true)
                            })
                    })
                    .await?;
            }
            Err(e) => {
                error!("[{request_id}] Error fetching DM stats: {e}");
                command
                    .create_interaction_response(&serenity_ctx.http, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content(
                                        "Failed to fetch DM statistics. Please try again later.",
                                    )
                                    .ephemeral(true)
                            })
                    })
                    .await?;
            }
        }

        Ok(())
    }

    // ── session_history ─────────────────────────────────────────────────

    /// Handle /session_history command - show recent DM sessions
    async fn handle_session_history(
        &self,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
        ctx: &CommandContext,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let limit = get_integer_option(&command.data.options, "limit").unwrap_or(5);

        debug!(
            "[{request_id}] Fetching session history for user {user_id} (limit: {limit})"
        );

        match ctx
            .database
            .get_user_recent_sessions(&user_id, limit)
            .await
        {
            Ok(sessions) => {
                let resp_text = if sessions.is_empty() {
                    "You don't have any DM sessions recorded yet.".to_string()
                } else {
                    let mut output = format!(
                        "**Your Recent DM Sessions ({} most recent)**\n\n",
                        sessions.len()
                    );

                    for (idx, session) in sessions.iter().enumerate() {
                        let status = if session.ended_at.is_some() {
                            "Ended"
                        } else {
                            "Active"
                        };
                        let started = session
                            .started_at
                            .split('T')
                            .next()
                            .unwrap_or(&session.started_at);
                        let response_time = if session.avg_response_time_ms < 1000 {
                            format!("{}ms", session.avg_response_time_ms)
                        } else {
                            format!("{:.1}s", session.avg_response_time_ms as f64 / 1000.0)
                        };

                        output.push_str(&format!(
                            "{}. {} | {} messages | Avg response: {} | {}\n",
                            idx + 1,
                            started,
                            session.message_count,
                            response_time,
                            status
                        ));
                    }
                    output
                };

                command
                    .create_interaction_response(&serenity_ctx.http, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content(&resp_text).ephemeral(true)
                            })
                    })
                    .await?;
            }
            Err(e) => {
                error!("[{request_id}] Error fetching session history: {e}");
                command
                    .create_interaction_response(&serenity_ctx.http, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content(
                                        "Failed to fetch session history. Please try again later.",
                                    )
                                    .ephemeral(true)
                            })
                    })
                    .await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_handler_commands() {
        let handler = InfoHandler;
        let names = handler.command_names();

        assert!(names.contains(&"introspect"));
        assert!(names.contains(&"commits"));
        assert!(names.contains(&"features"));
        assert!(names.contains(&"toggle"));
        assert!(names.contains(&"sysinfo"));
        assert!(names.contains(&"usage"));
        assert!(names.contains(&"dm_stats"));
        assert!(names.contains(&"session_history"));
        assert_eq!(names.len(), 8);
    }
}
