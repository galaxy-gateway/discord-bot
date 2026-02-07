//! Utility command handlers
//!
//! Handles: ping, help, status, version, uptime
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

use crate::commands::context::CommandContext;
use crate::commands::handler::SlashCommandHandler;
use crate::message_components::MessageComponentHandler;

/// Handler for utility commands: ping, help, status, version, uptime
pub struct UtilityHandler;

#[async_trait]
impl SlashCommandHandler for UtilityHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["ping", "help", "status", "version", "uptime"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        match command.data.name.as_str() {
            "ping" => self.handle_ping(&ctx, serenity_ctx, command).await,
            "help" => self.handle_help(serenity_ctx, command).await,
            "status" => self.handle_status(&ctx, serenity_ctx, command).await,
            "version" => self.handle_version(&ctx, serenity_ctx, command).await,
            "uptime" => self.handle_uptime(&ctx, serenity_ctx, command).await,
            _ => Ok(()),
        }
    }
}

impl UtilityHandler {
    /// Handle /ping command
    async fn handle_ping(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        ctx.database.log_usage(&user_id, "ping", None).await?;

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| message.content("Pong!"))
            })
            .await?;

        info!("Ping command completed for user {user_id}");
        Ok(())
    }

    /// Handle /help command
    async fn handle_help(
        &self,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let help_text = r#"**Available Slash Commands:**
`/ping` - Test bot responsiveness
`/help` - Show this help message
`/personas` - List available personas
`/set_user` - Set your personal preferences
`/hey <message>` - Chat with your current persona
`/explain <topic>` - Get an explanation
`/simple <topic>` - Get a simple explanation with analogies
`/steps <task>` - Break something into steps
`/recipe <food>` - Get a recipe for the specified food

**Available Personas:**
- `muppet` - Muppet expert (default)
- `chef` - Cooking expert
- `teacher` - Patient teacher
- `analyst` - Step-by-step analyst

**Interactive Features:**
Use the buttons below for more help or to try custom prompts!"#;

        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message
                            .content(help_text)
                            .set_components(MessageComponentHandler::create_help_buttons())
                    })
            })
            .await?;

        Ok(())
    }

    /// Handle /status command
    async fn handle_status(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        let uptime = ctx.start_time.elapsed();
        let hours = uptime.as_secs() / 3600;
        let minutes = (uptime.as_secs() % 3600) / 60;
        let seconds = uptime.as_secs() % 60;

        let response = format!(
            "**Bot Status**\n\
            ✅ Online and operational\n\
            ⏱️ Uptime: {}h {}m {}s\n\
            📦 Version: {}",
            hours,
            minutes,
            seconds,
            crate::features::get_bot_version()
        );

        command
            .create_interaction_response(&serenity_ctx.http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(response))
            })
            .await?;

        ctx.database.log_usage(&user_id, "status", None).await?;
        info!("Status command completed for user {user_id}");
        Ok(())
    }

    /// Handle /version command
    async fn handle_version(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        let mut output = format!(
            "**Persona Bot v{}**\n\n",
            crate::features::get_bot_version()
        );
        output.push_str("**Feature Versions:**\n");

        for feature in crate::features::get_features() {
            output.push_str(&format!("• {} v{}\n", feature.name, feature.version));
        }

        command
            .create_interaction_response(&serenity_ctx.http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(output))
            })
            .await?;

        ctx.database.log_usage(&user_id, "version", None).await?;
        info!("Version command completed for user {user_id}");
        Ok(())
    }

    /// Handle /uptime command
    async fn handle_uptime(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        let uptime = ctx.start_time.elapsed();
        let days = uptime.as_secs() / 86400;
        let hours = (uptime.as_secs() % 86400) / 3600;
        let minutes = (uptime.as_secs() % 3600) / 60;
        let seconds = uptime.as_secs() % 60;

        let response = if days > 0 {
            format!("⏱️ Uptime: {days}d {hours}h {minutes}m {seconds}s")
        } else if hours > 0 {
            format!("⏱️ Uptime: {hours}h {minutes}m {seconds}s")
        } else if minutes > 0 {
            format!("⏱️ Uptime: {minutes}m {seconds}s")
        } else {
            format!("⏱️ Uptime: {seconds}s")
        };

        command
            .create_interaction_response(&serenity_ctx.http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(response))
            })
            .await?;

        ctx.database.log_usage(&user_id, "uptime", None).await?;
        info!("Uptime command completed for user {user_id}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utility_handler_commands() {
        let handler = UtilityHandler;
        let names = handler.command_names();

        assert!(names.contains(&"ping"));
        assert!(names.contains(&"help"));
        assert!(names.contains(&"status"));
        assert!(names.contains(&"version"));
        assert!(names.contains(&"uptime"));
        assert_eq!(names.len(), 5);
    }
}
