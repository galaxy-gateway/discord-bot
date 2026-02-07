//! Context menu command handler
//!
//! Handles: Analyze Message, Explain Message, Analyze User
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from command_handler.rs

use anyhow::Result;
use async_trait::async_trait;
use log::{error, info};
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::sync::Arc;
use uuid::Uuid;

use crate::commands::context::CommandContext;
use crate::commands::handler::SlashCommandHandler;
use crate::features::analytics::CostBucket;

/// Handler for context menu commands: Analyze Message, Explain Message, Analyze User
pub struct ContextMenuHandler;

#[async_trait]
impl SlashCommandHandler for ContextMenuHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["Analyze Message", "Explain Message", "Analyze User"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();
        match command.data.name.as_str() {
            "Analyze Message" | "Explain Message" => {
                self.handle_context_menu_message(&ctx, serenity_ctx, command, request_id)
                    .await
            }
            "Analyze User" => {
                self.handle_context_menu_user(&ctx, serenity_ctx, command, request_id)
                    .await
            }
            _ => Ok(()),
        }
    }
}

impl ContextMenuHandler {
    /// Handle message context menu commands (Analyze Message, Explain Message)
    async fn handle_context_menu_message(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let message_content = "Message content will be analyzed".to_string();
        let user_persona = ctx.database.get_user_persona(&user_id).await?;

        info!(
            "[{request_id}] Context menu: {} | User: {user_id}",
            command.data.name
        );

        let system_prompt = match command.data.name.as_str() {
            "Analyze Message" => {
                ctx.persona_manager
                    .get_system_prompt(&user_persona, Some("steps"))
            }
            "Explain Message" => {
                ctx.persona_manager
                    .get_system_prompt(&user_persona, Some("explain"))
            }
            _ => ctx.persona_manager.get_system_prompt(&user_persona, None),
        };

        let prompt = format!("Please analyze this message: \"{message_content}\"");

        ctx.database
            .log_usage(&user_id, &command.data.name, Some(&user_persona))
            .await?;

        // Defer interaction (AI calls may take time)
        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        match ctx
            .get_simple_ai_response(&system_prompt, &prompt, request_id, CostBucket::Ask)
            .await
        {
            Ok(ai_response) => {
                let response_text = format!("**{}:**\n{}", command.data.name, ai_response);
                command
                    .edit_original_interaction_response(&serenity_ctx.http, |response| {
                        response.content(&response_text)
                    })
                    .await?;
            }
            Err(e) => {
                error!("[{request_id}] AI response error in context menu: {e}");
                let error_message = if e.to_string().contains("timed out") {
                    "**Analysis timed out** - The AI service is taking too long. Please try again."
                } else {
                    "**Error analyzing message** - Something went wrong. Please try again later."
                };
                command
                    .edit_original_interaction_response(&serenity_ctx.http, |response| {
                        response.content(error_message)
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle user context menu command (Analyze User)
    async fn handle_context_menu_user(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let target_user = "Discord User".to_string();
        let user_persona = ctx.database.get_user_persona(&user_id).await?;

        info!("[{request_id}] Context menu: Analyze User | User: {user_id}");

        let system_prompt = ctx
            .persona_manager
            .get_system_prompt(&user_persona, Some("explain"));
        let prompt = format!(
            "Please provide general information about Discord users and their roles in communities. \
             The user being analyzed is: {target_user}"
        );

        ctx.database
            .log_usage(&user_id, "analyze_user", Some(&user_persona))
            .await?;

        // Defer interaction (AI calls may take time)
        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        match ctx
            .get_simple_ai_response(&system_prompt, &prompt, request_id, CostBucket::Ask)
            .await
        {
            Ok(ai_response) => {
                let response_text = format!("**User Analysis:**\n{ai_response}");
                command
                    .edit_original_interaction_response(&serenity_ctx.http, |response| {
                        response.content(&response_text)
                    })
                    .await?;
            }
            Err(e) => {
                error!("[{request_id}] AI response error in user context menu: {e}");
                let error_message = if e.to_string().contains("timed out") {
                    "**Analysis timed out** - The AI service is taking too long. Please try again."
                } else {
                    "**Error analyzing user** - Something went wrong. Please try again later."
                };
                command
                    .edit_original_interaction_response(&serenity_ctx.http, |response| {
                        response.content(error_message)
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
    fn test_context_menu_handler_commands() {
        let handler = ContextMenuHandler;
        let names = handler.command_names();

        assert_eq!(names.len(), 3);
        assert!(names.contains(&"Analyze Message"));
        assert!(names.contains(&"Explain Message"));
        assert!(names.contains(&"Analyze User"));
    }
}
