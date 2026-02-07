//! Persona command handlers
//!
//! Handles: personas
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

/// Handler for persona listing command
///
/// Note: set_user is in admin.rs since it's a settings command
/// that may expand beyond just persona selection.
pub struct PersonaHandler;

#[async_trait]
impl SlashCommandHandler for PersonaHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["personas"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        match command.data.name.as_str() {
            "personas" => self.handle_personas(&ctx, serenity_ctx, command).await,
            _ => Ok(()),
        }
    }
}

impl PersonaHandler {
    /// Handle /personas command - list available personas
    async fn handle_personas(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let personas = ctx.persona_manager.list_personas();
        let mut response = "**Available Personas:**\n".to_string();

        for (name, persona) in personas {
            response.push_str(&format!("• `{}` - {}\n", name, persona.description));
        }

        let user_id = command.user.id.to_string();
        let current_persona = ctx.database.get_user_persona(&user_id).await?;
        response.push_str(&format!("\nYour current persona: `{current_persona}`"));
        response.push_str("\n\n**Quick Switch:**\nUse the dropdown below to change your persona!");

        command
            .create_interaction_response(&serenity_ctx.http, |response_builder| {
                response_builder
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message
                            .content(response)
                            .set_components(MessageComponentHandler::create_persona_select_menu())
                    })
            })
            .await?;

        info!("Personas command completed for user {user_id}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persona_handler_commands() {
        let handler = PersonaHandler;
        let names = handler.command_names();

        assert!(names.contains(&"personas"));
        assert_eq!(names.len(), 1);
    }
}
