//! Reminder command handlers
//!
//! Handles: remind, reminders, forget
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from command_handler.rs

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info};
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::sync::Arc;

use crate::commands::context::CommandContext;
use crate::commands::handler::SlashCommandHandler;
use crate::commands::slash::{get_integer_option, get_string_option};

/// Handler for reminder-related commands
pub struct RemindHandler;

#[async_trait]
impl SlashCommandHandler for RemindHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["remind", "reminders", "forget"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        match command.data.name.as_str() {
            "remind" => self.handle_remind(&ctx, serenity_ctx, command).await,
            "reminders" => self.handle_reminders(&ctx, serenity_ctx, command).await,
            "forget" => self.handle_forget(&ctx, serenity_ctx, command).await,
            _ => Ok(()),
        }
    }
}

impl RemindHandler {
    /// Handle /remind command - create a new reminder
    async fn handle_remind(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();

        // Check if reminders feature is enabled for this guild
        let guild_id = command.guild_id.map(|id| id.to_string());
        let guild_id_opt = guild_id.as_deref();
        let reminders_enabled = if let Some(gid) = guild_id_opt {
            ctx.database
                .is_feature_enabled("reminders", None, Some(gid))
                .await?
        } else {
            true // Always enabled in DMs
        };

        if !reminders_enabled {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| {
                            msg.content("❌ Reminders are disabled on this server.")
                        })
                })
                .await?;
            return Ok(());
        }

        let time_str = get_string_option(&command.data.options, "time")
            .ok_or_else(|| anyhow::anyhow!("Missing time parameter"))?;
        let message = get_string_option(&command.data.options, "message")
            .ok_or_else(|| anyhow::anyhow!("Missing message parameter"))?;

        // Parse the duration
        let duration_seconds = match Self::parse_duration(&time_str) {
            Some(secs) => secs,
            None => {
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|msg| {
                                msg.content(
                                    "❌ Invalid time format. Use formats like `30m`, `2h`, `1d`, or `1h30m`.",
                                )
                            })
                    })
                    .await?;
                return Ok(());
            }
        };

        // Calculate remind_at timestamp
        let remind_at = chrono::Utc::now() + chrono::Duration::seconds(duration_seconds);
        let remind_at_str = remind_at.format("%Y-%m-%d %H:%M:%S").to_string();

        // Store the reminder
        let reminder_id = ctx
            .database
            .add_reminder(&user_id, &channel_id, &message, &remind_at_str)
            .await?;

        info!(
            "Created reminder {} for user {} in {} ({})",
            reminder_id,
            user_id,
            Self::format_duration(duration_seconds),
            remind_at_str
        );

        // Log usage
        ctx.database.log_usage(&user_id, "remind", None).await?;

        let duration_display = Self::format_duration(duration_seconds);
        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|msg| {
                        msg.content(format!(
                            "⏰ Got it! I'll remind you in **{duration_display}** about:\n> {message}\n\n*Reminder ID: #{reminder_id}*"
                        ))
                    })
            })
            .await?;

        Ok(())
    }

    /// Handle /reminders command - list or cancel reminders
    async fn handle_reminders(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        // Check if reminders feature is enabled for this guild
        let guild_id = command.guild_id.map(|id| id.to_string());
        let guild_id_opt = guild_id.as_deref();
        let reminders_enabled = if let Some(gid) = guild_id_opt {
            ctx.database
                .is_feature_enabled("reminders", None, Some(gid))
                .await?
        } else {
            true // Always enabled in DMs
        };

        if !reminders_enabled {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| {
                            msg.content("❌ Reminders are disabled on this server.")
                        })
                })
                .await?;
            return Ok(());
        }

        let action = get_string_option(&command.data.options, "action")
            .unwrap_or_else(|| "list".to_string());

        match action.as_str() {
            "cancel" => {
                self.handle_cancel_reminder(ctx, serenity_ctx, command, &user_id)
                    .await
            }
            _ => {
                self.handle_list_reminders(ctx, serenity_ctx, command, &user_id)
                    .await
            }
        }
    }

    /// Cancel a specific reminder
    async fn handle_cancel_reminder(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        user_id: &str,
    ) -> Result<()> {
        let reminder_id = get_integer_option(&command.data.options, "id");

        if let Some(id) = reminder_id {
            let deleted = ctx.database.delete_reminder(id, user_id).await?;

            if deleted {
                info!("Deleted reminder {id} for user {user_id}");
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|msg| {
                                msg.content(format!("✅ Cancelled reminder #{id}."))
                            })
                    })
                    .await?;
            } else {
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|msg| {
                                msg.content(format!(
                                    "❌ Reminder #{id} not found or doesn't belong to you."
                                ))
                            })
                    })
                    .await?;
            }
        } else {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| {
                            msg.content(
                                "❌ Please provide a reminder ID to cancel. Use `/reminders` to see your reminder IDs.",
                            )
                        })
                })
                .await?;
        }

        Ok(())
    }

    /// List all pending reminders
    async fn handle_list_reminders(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        user_id: &str,
    ) -> Result<()> {
        let reminders = ctx.database.get_user_reminders(user_id).await?;

        if reminders.is_empty() {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| {
                            msg.content(
                                "📋 You don't have any pending reminders.\n\nUse `/remind <time> <message>` to create one!",
                            )
                        })
                })
                .await?;
        } else {
            let mut reminder_list = String::from("📋 **Your Pending Reminders:**\n\n");

            for (id, _channel_id, text, remind_at) in &reminders {
                let remind_time =
                    chrono::NaiveDateTime::parse_from_str(remind_at, "%Y-%m-%d %H:%M:%S")
                        .map(|dt| {
                            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                                dt,
                                chrono::Utc,
                            )
                        })
                        .ok();

                let time_display = if let Some(dt) = remind_time {
                    let now = chrono::Utc::now();
                    let diff = dt.signed_duration_since(now);
                    if diff.num_seconds() > 0 {
                        format!("in {}", Self::format_duration(diff.num_seconds()))
                    } else {
                        "any moment now".to_string()
                    }
                } else {
                    remind_at.clone()
                };

                reminder_list
                    .push_str(&format!("**#{id}** - {time_display} ({remind_at})\n> {text}\n\n"));
            }

            reminder_list.push_str("*Use `/reminders cancel <id>` to cancel a reminder.*");

            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| msg.content(&reminder_list))
                })
                .await?;
        }

        Ok(())
    }

    /// Handle /forget command - clear conversation history
    async fn handle_forget(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();

        debug!(
            "Processing forget command for user: {user_id} in channel: {channel_id}"
        );

        // Clear conversation history
        ctx.database
            .clear_conversation_history(&user_id, &channel_id)
            .await?;

        info!(
            "Cleared conversation history for user {user_id} in channel {channel_id}"
        );

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(
                            "🧹 Your conversation history has been cleared! I'll start fresh from now on.",
                        )
                    })
            })
            .await?;

        Ok(())
    }

    /// Parse a time duration string like "30m", "2h", "1d", "1h30m" into seconds
    fn parse_duration(time_str: &str) -> Option<i64> {
        let time_str = time_str.trim().to_lowercase();
        let mut total_seconds: i64 = 0;
        let mut current_number = String::new();

        for c in time_str.chars() {
            if c.is_ascii_digit() {
                current_number.push(c);
            } else if !current_number.is_empty() {
                let value: i64 = current_number.parse().ok()?;
                current_number.clear();

                let seconds = match c {
                    's' => value,
                    'm' => value * 60,
                    'h' => value * 60 * 60,
                    'd' => value * 60 * 60 * 24,
                    'w' => value * 60 * 60 * 24 * 7,
                    _ => return None,
                };
                total_seconds += seconds;
            }
        }

        if total_seconds > 0 {
            Some(total_seconds)
        } else {
            None
        }
    }

    /// Format a duration in seconds into a human-readable string
    fn format_duration(seconds: i64) -> String {
        if seconds < 60 {
            format!("{} second{}", seconds, if seconds == 1 { "" } else { "s" })
        } else if seconds < 3600 {
            let mins = seconds / 60;
            format!("{} minute{}", mins, if mins == 1 { "" } else { "s" })
        } else if seconds < 86400 {
            let hours = seconds / 3600;
            let mins = (seconds % 3600) / 60;
            if mins > 0 {
                format!(
                    "{} hour{} {} minute{}",
                    hours,
                    if hours == 1 { "" } else { "s" },
                    mins,
                    if mins == 1 { "" } else { "s" }
                )
            } else {
                format!("{} hour{}", hours, if hours == 1 { "" } else { "s" })
            }
        } else {
            let days = seconds / 86400;
            let hours = (seconds % 86400) / 3600;
            if hours > 0 {
                format!(
                    "{} day{} {} hour{}",
                    days,
                    if days == 1 { "" } else { "s" },
                    hours,
                    if hours == 1 { "" } else { "s" }
                )
            } else {
                format!("{} day{}", days, if days == 1 { "" } else { "s" })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remind_handler_commands() {
        let handler = RemindHandler;
        let names = handler.command_names();

        assert!(names.contains(&"remind"));
        assert!(names.contains(&"reminders"));
        assert!(names.contains(&"forget"));
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(RemindHandler::parse_duration("30s"), Some(30));
        assert_eq!(RemindHandler::parse_duration("30m"), Some(1800));
        assert_eq!(RemindHandler::parse_duration("2h"), Some(7200));
        assert_eq!(RemindHandler::parse_duration("1d"), Some(86400));
        assert_eq!(RemindHandler::parse_duration("1w"), Some(604800));
        assert_eq!(RemindHandler::parse_duration("1h30m"), Some(5400));
        assert_eq!(RemindHandler::parse_duration("invalid"), None);
        assert_eq!(RemindHandler::parse_duration(""), None);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(RemindHandler::format_duration(30), "30 seconds");
        assert_eq!(RemindHandler::format_duration(1), "1 second");
        assert_eq!(RemindHandler::format_duration(60), "1 minute");
        assert_eq!(RemindHandler::format_duration(120), "2 minutes");
        assert_eq!(RemindHandler::format_duration(3600), "1 hour");
        assert_eq!(RemindHandler::format_duration(3660), "1 hour 1 minute");
        assert_eq!(RemindHandler::format_duration(86400), "1 day");
        assert_eq!(RemindHandler::format_duration(90000), "1 day 1 hour");
    }
}
