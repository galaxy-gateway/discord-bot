//! Ask command handler
//!
//! Handles: ask
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from command_handler.rs

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, error, info};
use serenity::builder::GetMessages;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::commands::context::{is_in_thread_channel, CommandContext};
use crate::commands::handler::SlashCommandHandler;
use crate::commands::slash::{get_bool_option, get_integer_option, get_string_option};
use crate::core::{chunk_for_embed, truncate_for_embed};
use crate::features::analytics::CostBucket;
use crate::features::personas::{apply_paragraph_limit, Persona};
use serenity::builder::CreateEmbed;

/// Handler for /ask command - ask any persona a question
pub struct AskHandler;

#[async_trait]
impl SlashCommandHandler for AskHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["ask"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();
        self.handle_ask(&ctx, serenity_ctx, command, request_id)
            .await
    }
}

impl AskHandler {
    /// Handle /ask command
    async fn handle_ask(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let start_time = Instant::now();

        // Extract command options
        let persona_id = get_string_option(&command.data.options, "persona")
            .ok_or_else(|| anyhow::anyhow!("Missing persona argument"))?;
        let prompt = get_string_option(&command.data.options, "prompt")
            .ok_or_else(|| anyhow::anyhow!("Missing prompt argument"))?;
        let ignore_context =
            get_bool_option(&command.data.options, "ignore_context").unwrap_or(false);

        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id;
        let guild_id = command.guild_id.map(|id| id.to_string());

        info!(
            "[{request_id}] /ask command | Persona: {persona_id} | User: {user_id} | Ignore context: {ignore_context}"
        );

        // Validate persona exists
        let persona = ctx.persona_manager.get_persona_with_portrait(&persona_id);
        if persona.is_none() {
            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.content(format!("Unknown persona: `{persona_id}`"))
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }
        let persona = persona.unwrap();

        // Get max_paragraphs: per-request overrides channel default; 0 = no limit
        let max_paragraphs = get_integer_option(&command.data.options, "paragraphs")
            .unwrap_or_else(|| {
                if let Some(gid) = command.guild_id {
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            ctx.database
                                .get_channel_max_paragraphs(
                                    &gid.to_string(),
                                    &channel_id.to_string(),
                                )
                                .await
                                .unwrap_or(0)
                        })
                    })
                } else {
                    0
                }
            });

        // Get system prompt for the persona with paragraph limit applied
        let system_prompt = ctx.persona_manager.get_system_prompt(&persona_id, None);
        let system_prompt = apply_paragraph_limit(&system_prompt, max_paragraphs);
        debug!(
            "[{request_id}] System prompt with paragraph limit | MaxParagraphs: {max_paragraphs}"
        );

        // Defer the interaction (required for AI calls that may take time)
        info!("[{request_id}] Deferring interaction response");
        command
            .create_interaction_response(&serenity_ctx.http, |r| {
                r.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .map_err(|e| {
                error!("[{request_id}] Failed to defer interaction: {e}");
                anyhow::anyhow!("Failed to defer interaction: {e}")
            })?;

        // Fetch context if not ignored
        let conversation_history: Vec<(String, String)> = if ignore_context {
            debug!("[{request_id}] Skipping context fetch (ignore_context=true)");
            Vec::new()
        } else {
            // Check if we're in a thread
            let in_thread = is_in_thread_channel(serenity_ctx, channel_id).await?;

            if in_thread {
                debug!("[{request_id}] Fetching thread context");
                // Fetch recent messages from thread
                let messages = channel_id
                    .messages(&serenity_ctx.http, |builder: &mut GetMessages| {
                        builder.limit(20)
                    })
                    .await
                    .unwrap_or_default();

                let bot_id = serenity_ctx.http.get_current_user().await?.id;
                messages
                    .iter()
                    .rev() // Oldest first
                    .filter(|m| !m.content.is_empty())
                    .map(|m| {
                        let role = if m.author.id == bot_id {
                            "assistant".to_string()
                        } else {
                            "user".to_string()
                        };
                        (role, m.content.clone())
                    })
                    .collect()
            } else {
                debug!("[{request_id}] Fetching channel context from database");
                ctx.database
                    .get_conversation_history(&user_id, &channel_id.to_string(), 10)
                    .await
                    .unwrap_or_default()
            }
        };

        debug!(
            "[{request_id}] Context: {} messages | Prompt length: {}",
            conversation_history.len(),
            prompt.len()
        );

        // Log usage
        ctx.database
            .log_usage(&user_id, "ask", Some(&persona_id))
            .await?;

        // Get AI response
        info!("[{request_id}] Calling OpenAI API");
        let ai_response = ctx
            .get_ai_response(
                &system_prompt,
                &prompt,
                conversation_history,
                request_id,
                Some(&user_id),
                guild_id.as_deref(),
                Some(&channel_id.to_string()),
                CostBucket::Ask,
            )
            .await;

        match ai_response {
            Ok(response) => {
                let processing_time = start_time.elapsed();
                info!(
                    "[{request_id}] Response received | Time: {:?} | Length: {}",
                    processing_time,
                    response.len()
                );

                // Build and send embed response
                let chunks = chunk_for_embed(&response);
                if chunks.len() > 1 {
                    debug!("[{request_id}] Response split into {} chunks", chunks.len());

                    if let Some(first_chunk) = chunks.first() {
                        let embed = Self::build_persona_embed(&persona, first_chunk);
                        command
                            .edit_original_interaction_response(&serenity_ctx.http, |r| {
                                r.set_embed(embed)
                            })
                            .await?;
                    }

                    // Send remaining chunks as follow-ups
                    for chunk in chunks.iter().skip(1) {
                        if !chunk.trim().is_empty() {
                            let embed = Self::build_continuation_embed(&persona, chunk);
                            command
                                .create_followup_message(&serenity_ctx.http, |m| {
                                    m.set_embed(embed)
                                })
                                .await?;
                        }
                    }
                } else {
                    let embed = Self::build_persona_embed(&persona, &response);
                    command
                        .edit_original_interaction_response(&serenity_ctx.http, |r| {
                            r.set_embed(embed)
                        })
                        .await?;
                }

                info!("[{request_id}] /ask response sent successfully");
            }
            Err(e) => {
                error!("[{request_id}] AI response failed: {e}");
                command
                    .edit_original_interaction_response(&serenity_ctx.http, |r| {
                        r.content(format!(
                            "Sorry, I couldn't get a response from {}. Please try again.",
                            persona.name
                        ))
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Build an embed for a persona response
    fn build_persona_embed(persona: &Persona, response_text: &str) -> CreateEmbed {
        let mut embed = CreateEmbed::default();
        embed.author(|a| {
            a.name(&persona.name);
            if let Some(url) = &persona.portrait_url {
                a.icon_url(url);
            }
            a
        });
        embed.color(persona.color);
        embed.description(truncate_for_embed(response_text));
        embed
    }

    /// Build a continuation embed for long responses
    fn build_continuation_embed(persona: &Persona, response_text: &str) -> CreateEmbed {
        let mut embed = CreateEmbed::default();
        embed.color(persona.color);
        embed.description(response_text);
        embed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ask_handler_commands() {
        let handler = AskHandler;
        let names = handler.command_names();

        assert!(names.contains(&"ask"));
        assert_eq!(names.len(), 1);
    }
}
