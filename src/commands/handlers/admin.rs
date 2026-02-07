//! Admin command handlers
//!
//! Handles: set_channel, set_guild, settings, admin_role, set_user
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from command_handler.rs

use anyhow::Result;
use async_trait::async_trait;
use log::info;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::sync::Arc;
use uuid::Uuid;

use crate::commands::context::CommandContext;
use crate::commands::handler::SlashCommandHandler;
use crate::commands::slash::{
    admin::{validate_channel_setting, validate_guild_setting, validate_user_setting},
    get_channel_option, get_role_option, get_string_option,
};

/// Handler for admin/settings commands
pub struct AdminHandler;

#[async_trait]
impl SlashCommandHandler for AdminHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["set_channel", "set_guild", "settings", "admin_role", "set_user"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();
        match command.data.name.as_str() {
            "set_channel" => self.handle_set_channel(&ctx, serenity_ctx, command, request_id).await,
            "set_guild" => self.handle_set_guild(&ctx, serenity_ctx, command, request_id).await,
            "settings" => self.handle_settings(&ctx, serenity_ctx, command, request_id).await,
            "admin_role" => self.handle_admin_role(&ctx, serenity_ctx, command, request_id).await,
            "set_user" => self.handle_set_user(&ctx, serenity_ctx, command, request_id).await,
            _ => Ok(()),
        }
    }
}

impl AdminHandler {
    /// Require guild context, returning an error response if in DMs
    async fn require_guild(
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<Option<String>> {
        match command.guild_id {
            Some(id) => Ok(Some(id.to_string())),
            None => {
                command
                    .create_interaction_response(&serenity_ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("This command can only be used in a server.")
                            })
                    })
                    .await?;
                Ok(None)
            }
        }
    }

    /// Handle /set_channel command
    async fn handle_set_channel(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let guild_id = match Self::require_guild(serenity_ctx, command).await? {
            Some(id) => id,
            None => return Ok(()),
        };

        let setting = get_string_option(&command.data.options, "setting")
            .ok_or_else(|| anyhow::anyhow!("Missing setting parameter"))?;
        let value = get_string_option(&command.data.options, "value")
            .ok_or_else(|| anyhow::anyhow!("Missing value parameter"))?;

        // Validate setting and value
        let (is_valid, error_msg) = validate_channel_setting(&setting, &value);
        if !is_valid {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content(error_msg.to_string())
                        })
                })
                .await?;
            return Ok(());
        }

        // Get target channel (default to current channel)
        let target_channel_id = get_channel_option(&command.data.options, "channel")
            .map(|id| id.to_string())
            .unwrap_or_else(|| command.channel_id.to_string());

        // Apply the setting
        let response_message = match setting.as_str() {
            "verbosity" => {
                ctx.database
                    .set_channel_verbosity(&guild_id, &target_channel_id, &value)
                    .await?;
                info!("[{request_id}] Set verbosity for channel {target_channel_id} to {value}");
                format!("Verbosity for <#{target_channel_id}> set to **{value}**")
            }
            "persona" => {
                if value == "clear" {
                    ctx.database
                        .set_channel_persona(&guild_id, &target_channel_id, None)
                        .await?;
                    info!(
                        "[{request_id}] Cleared persona override for channel {target_channel_id}"
                    );
                    format!("Persona override cleared for <#{target_channel_id}>. Users will use their own personas.")
                } else {
                    ctx.database
                        .set_channel_persona(&guild_id, &target_channel_id, Some(&value))
                        .await?;
                    info!("[{request_id}] Set persona for channel {target_channel_id} to {value}");
                    format!("Persona for <#{target_channel_id}> set to **{value}**. All users in this channel will use this persona.")
                }
            }
            "conflict_mediation" => {
                let enabled = value == "enabled";
                ctx.database
                    .set_channel_conflict_enabled(&guild_id, &target_channel_id, enabled)
                    .await?;
                info!("[{request_id}] Set conflict_mediation for channel {target_channel_id} to {value}");
                let status = if enabled { "Enabled" } else { "Disabled" };
                format!("Conflict mediation for <#{target_channel_id}> is now **{status}**")
            }
            "max_paragraphs" => {
                let max_paragraphs: i64 = value.parse().unwrap_or(0);
                ctx.database
                    .set_channel_max_paragraphs(&guild_id, &target_channel_id, max_paragraphs)
                    .await?;
                info!("[{request_id}] Set max_paragraphs for channel {target_channel_id} to {max_paragraphs}");
                if max_paragraphs == 0 {
                    format!("Max paragraphs for <#{target_channel_id}> set to **unlimited**")
                } else {
                    format!(
                        "Max paragraphs for <#{target_channel_id}> set to **{max_paragraphs}**"
                    )
                }
            }
            _ => {
                format!("Unknown setting: {setting}")
            }
        };

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| message.content(response_message))
            })
            .await?;

        Ok(())
    }

    /// Handle /set_guild command
    async fn handle_set_guild(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let guild_id = match Self::require_guild(serenity_ctx, command).await? {
            Some(id) => id,
            None => return Ok(()),
        };

        let setting = get_string_option(&command.data.options, "setting")
            .ok_or_else(|| anyhow::anyhow!("Missing setting parameter"))?;
        let value = get_string_option(&command.data.options, "value")
            .ok_or_else(|| anyhow::anyhow!("Missing value parameter"))?;

        // Validate setting and value using shared validation
        let (is_valid, error_msg) = validate_guild_setting(&setting, &value);
        if !is_valid {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content(error_msg.to_string())
                        })
                })
                .await?;
            return Ok(());
        }

        // Check if this is a global bot setting or a guild setting
        let is_global_setting = matches!(
            setting.as_str(),
            "startup_notification"
                | "startup_notify_owner_id"
                | "startup_notify_channel_id"
                | "startup_dm_commit_count"
                | "startup_channel_commit_count"
        );

        if is_global_setting {
            info!("[{request_id}] Setting global bot setting '{setting}' to '{value}'");
            ctx.database.set_bot_setting(&setting, &value).await?;
        } else {
            info!("[{request_id}] Setting guild {guild_id} setting '{setting}' to '{value}'");
            ctx.database
                .set_guild_setting(&guild_id, &setting, &value)
                .await?;
        }

        let scope = if is_global_setting { "Global" } else { "Guild" };
        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message
                            .content(format!("{scope} setting `{setting}` set to **{value}**"))
                    })
            })
            .await?;

        Ok(())
    }

    /// Handle /settings command - display current settings
    async fn handle_settings(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let guild_id = match Self::require_guild(serenity_ctx, command).await? {
            Some(id) => id,
            None => return Ok(()),
        };

        let channel_id = command.channel_id.to_string();

        // Get channel settings
        let (channel_verbosity, conflict_enabled, channel_persona) = ctx
            .database
            .get_channel_settings(&guild_id, &channel_id)
            .await?;
        let channel_persona_display = channel_persona
            .map(|p| format!("`{p}` (override)"))
            .unwrap_or_else(|| "Not set (uses user/guild default)".to_string());

        // Get guild settings with defaults
        let guild_default_verbosity = ctx
            .database
            .get_guild_setting(&guild_id, "default_verbosity")
            .await?
            .unwrap_or_else(|| "concise".to_string());
        let guild_default_persona = ctx
            .database
            .get_guild_setting(&guild_id, "default_persona")
            .await?
            .unwrap_or_else(|| "obi".to_string());
        let guild_conflict_mediation = ctx
            .database
            .get_guild_setting(&guild_id, "conflict_mediation")
            .await?
            .unwrap_or_else(|| "enabled".to_string());
        let guild_conflict_sensitivity = ctx
            .database
            .get_guild_setting(&guild_id, "conflict_sensitivity")
            .await?
            .unwrap_or_else(|| "medium".to_string());
        let guild_mediation_cooldown = ctx
            .database
            .get_guild_setting(&guild_id, "mediation_cooldown")
            .await?
            .unwrap_or_else(|| "5".to_string());
        let guild_max_context = ctx
            .database
            .get_guild_setting(&guild_id, "max_context_messages")
            .await?
            .unwrap_or_else(|| "40".to_string());
        let guild_audio_transcription = ctx
            .database
            .get_guild_setting(&guild_id, "audio_transcription")
            .await?
            .unwrap_or_else(|| "enabled".to_string());
        let guild_audio_mode = ctx
            .database
            .get_guild_setting(&guild_id, "audio_transcription_mode")
            .await?
            .unwrap_or_else(|| "mention_only".to_string());
        let guild_audio_output = ctx
            .database
            .get_guild_setting(&guild_id, "audio_transcription_output")
            .await?
            .unwrap_or_else(|| "transcription_only".to_string());
        let guild_mention_responses = ctx
            .database
            .get_guild_setting(&guild_id, "mention_responses")
            .await?
            .unwrap_or_else(|| "enabled".to_string());
        let guild_debate_auto_response = ctx
            .database
            .get_guild_setting(&guild_id, "debate_auto_response")
            .await?
            .unwrap_or_else(|| "disabled".to_string());

        // Get bot admin role
        let admin_role = ctx
            .database
            .get_guild_setting(&guild_id, "bot_admin_role")
            .await?;
        let admin_role_display = match admin_role {
            Some(role_id) => format!("<@&{role_id}>"),
            None => "Not set (Discord admins only)".to_string(),
        };

        let conflict_status = if conflict_enabled {
            "Enabled"
        } else {
            "Disabled"
        };

        let settings_text = format!(
            "**Bot Settings**\n\n\
            **Channel Settings** (<#{channel_id}>):\n\
            - Verbosity: `{channel_verbosity}`\n\
            - Persona: {channel_persona_display}\n\
            - Conflict Mediation: {conflict_status}\n\n\
            **Guild Settings**:\n\
            - Default Verbosity: `{guild_default_verbosity}`\n\
            - Default Persona: `{guild_default_persona}`\n\
            - Conflict Mediation: `{guild_conflict_mediation}`\n\
            - Conflict Sensitivity: `{guild_conflict_sensitivity}`\n\
            - Mediation Cooldown: `{guild_mediation_cooldown}` minutes\n\
            - Max Context Messages: `{guild_max_context}`\n\
            - Audio Transcription: `{guild_audio_transcription}`\n\
            - Audio Transcription Mode: `{guild_audio_mode}`\n\
            - Audio Transcription Output: `{guild_audio_output}`\n\
            - Mention Responses: `{guild_mention_responses}`\n\
            - Debate Auto-Response: `{guild_debate_auto_response}`\n\
            - Bot Admin Role: {admin_role_display}\n"
        );

        info!("[{request_id}] Displaying settings for guild {guild_id} channel {channel_id}");

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| message.content(&settings_text))
            })
            .await?;

        Ok(())
    }

    /// Handle /admin_role command
    async fn handle_admin_role(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let guild_id = match Self::require_guild(serenity_ctx, command).await? {
            Some(id) => id,
            None => return Ok(()),
        };

        let role_id = get_role_option(&command.data.options, "role")
            .ok_or_else(|| anyhow::anyhow!("Missing role parameter"))?;

        info!("[{request_id}] Setting bot admin role for guild {guild_id} to {role_id}");

        ctx.database
            .set_guild_setting(&guild_id, "bot_admin_role", &role_id.to_string())
            .await?;

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(format!(
                            "Bot Admin role set to <@&{role_id}>. Users with this role can now manage bot settings."
                        ))
                    })
            })
            .await?;

        Ok(())
    }

    /// Handle /set_user command
    async fn handle_set_user(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let setting = get_string_option(&command.data.options, "setting")
            .ok_or_else(|| anyhow::anyhow!("Missing setting parameter"))?;
        let value = get_string_option(&command.data.options, "value")
            .ok_or_else(|| anyhow::anyhow!("Missing value parameter"))?;

        // Validate setting and value
        let (is_valid, error_msg) = validate_user_setting(&setting, &value);
        if !is_valid {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content(error_msg.to_string())
                        })
                })
                .await?;
            return Ok(());
        }

        let user_id = command.user.id.to_string();

        // Apply the setting
        let response_message = match setting.as_str() {
            "persona" => {
                ctx.database.set_user_persona(&user_id, &value).await?;
                info!("[{request_id}] Set persona for user {user_id} to {value}");
                format!("Your persona has been set to **{value}**")
            }
            _ => {
                format!("Unknown setting: {setting}")
            }
        };

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| message.content(response_message))
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_handler_commands() {
        let handler = AdminHandler;
        let names = handler.command_names();

        assert!(names.contains(&"set_channel"));
        assert!(names.contains(&"set_guild"));
        assert!(names.contains(&"settings"));
        assert!(names.contains(&"admin_role"));
        assert!(names.contains(&"set_user"));
        assert_eq!(names.len(), 5);
    }
}
