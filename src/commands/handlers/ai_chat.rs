//! AI chat command handlers
//!
//! Handles: hey, explain, simple, steps, recipe
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from command_handler.rs

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, error, info};
use serenity::builder::CreateEmbed;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::commands::context::CommandContext;
use crate::commands::handler::SlashCommandHandler;
use crate::commands::slash::{get_integer_option, get_string_option};
use crate::core::{chunk_for_embed, chunk_for_message, truncate_for_embed};
use crate::features::analytics::CostBucket;
use crate::features::personas::{apply_paragraph_limit, Persona};

/// Handler for AI chat commands: hey, explain, simple, steps, recipe
pub struct AiChatHandler;

#[async_trait]
impl SlashCommandHandler for AiChatHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["hey", "explain", "simple", "steps", "recipe"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();
        match command.data.name.as_str() {
            "hey" | "explain" | "simple" | "steps" | "recipe" => {
                self.handle_ai_command(&ctx, serenity_ctx, command, request_id)
                    .await
            }
            _ => Ok(()),
        }
    }
}

impl AiChatHandler {
    /// Handle AI chat command (hey, explain, simple, steps, recipe)
    async fn handle_ai_command(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let start_time = Instant::now();

        debug!(
            "[{}] Starting AI slash command processing | Command: {}",
            request_id, command.data.name
        );

        let option_name = match command.data.name.as_str() {
            "hey" => "message",
            "explain" => "topic",
            "simple" => "topic",
            "steps" => "task",
            "recipe" => "food",
            _ => "message",
        };

        debug!("[{request_id}] Extracting option '{option_name}' from command parameters");
        let user_message = get_string_option(&command.data.options, option_name)
            .ok_or_else(|| anyhow::anyhow!("Missing message parameter"))?;

        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();
        debug!(
            "[{}] Processing for user: {} | Message: '{}'",
            request_id,
            user_id,
            user_message.chars().take(100).collect::<String>()
        );

        // Get user's persona with channel override -> user -> guild default cascade
        debug!("[{request_id}] Getting user persona from database");
        let user_persona = if let Some(guild_id) = command.guild_id {
            ctx.database
                .get_persona_with_channel(&user_id, &guild_id.to_string(), &channel_id)
                .await?
        } else {
            ctx.database.get_user_persona(&user_id).await?
        };
        debug!("[{request_id}] User persona: {user_persona}");

        let modifier = match command.data.name.as_str() {
            "explain" => Some("explain"),
            "simple" => Some("simple"),
            "steps" => Some("steps"),
            "recipe" => Some("recipe"),
            _ => None,
        };

        // Get channel verbosity (only for guild channels)
        let verbosity = if let Some(guild_id) = command.guild_id {
            ctx.database
                .get_channel_verbosity(&guild_id.to_string(), &channel_id)
                .await?
        } else {
            "concise".to_string() // Default to concise for DMs
        };

        // Get max_paragraphs: per-request overrides channel default; 0 = no limit
        let max_paragraphs = get_integer_option(&command.data.options, "paragraphs")
            .unwrap_or_else(|| {
                if let Some(guild_id) = command.guild_id {
                    // Use block_in_place to safely call async from sync context
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            ctx.database
                                .get_channel_max_paragraphs(&guild_id.to_string(), &channel_id)
                                .await
                                .unwrap_or(0)
                        })
                    })
                } else {
                    0 // No limit for DMs by default
                }
            });

        debug!(
            "[{request_id}] Building system prompt | Persona: {user_persona} | Modifier: {modifier:?} | Verbosity: {verbosity} | MaxParagraphs: {max_paragraphs}"
        );
        let system_prompt = ctx.persona_manager.get_system_prompt_with_verbosity(
            &user_persona,
            modifier,
            &verbosity,
        );
        let system_prompt = apply_paragraph_limit(&system_prompt, max_paragraphs);
        debug!(
            "[{}] System prompt generated | Length: {} chars",
            request_id,
            system_prompt.len()
        );

        debug!("[{request_id}] Logging usage to database");
        ctx.database
            .log_usage(&user_id, &command.data.name, Some(&user_persona))
            .await?;
        debug!("[{request_id}] Usage logged successfully");

        // Immediately defer the interaction to prevent timeout (required within 3 seconds)
        info!("[{request_id}] Deferring Discord interaction response (3s rule)");
        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .map_err(|e| {
                error!("[{request_id}] Failed to defer interaction response: {e}");
                anyhow::anyhow!("Failed to defer interaction: {}", e)
            })?;
        info!("[{request_id}] Interaction deferred successfully");

        // Get AI response and edit the message
        let guild_id_str = command.guild_id.map(|id| id.to_string());
        let channel_id_str = command.channel_id.to_string();
        info!("[{request_id}] Calling OpenAI API");

        // All AI chat commands use the Ask cost bucket
        let cost_bucket = CostBucket::Ask;

        match ctx
            .get_ai_response(
                &system_prompt,
                &user_message,
                Vec::new(),
                request_id,
                Some(&user_id),
                guild_id_str.as_deref(),
                Some(&channel_id_str),
                cost_bucket,
            )
            .await
        {
            Ok(ai_response) => {
                let processing_time = start_time.elapsed();
                info!(
                    "[{}] OpenAI response received | Processing time: {:?} | Response length: {}",
                    request_id,
                    processing_time,
                    ai_response.len()
                );

                // Check if embeds are enabled for this guild
                let use_embeds = if let Some(gid) = guild_id_str.as_deref() {
                    ctx.database
                        .get_guild_setting(gid, "response_embeds")
                        .await
                        .unwrap_or(None)
                        .map(|v| v != "disabled")
                        .unwrap_or(true) // Default to enabled
                } else {
                    true // DMs always use embeds
                };

                // Get persona for embed styling
                let persona = ctx.persona_manager.get_persona(&user_persona);

                if use_embeds && persona.is_some() {
                    let p = persona.unwrap();

                    // Embed description limit is 4096
                    let chunks = chunk_for_embed(&ai_response);
                    if chunks.len() > 1 {
                        debug!(
                            "[{request_id}] Response too long for single embed, splitting into {} chunks",
                            chunks.len()
                        );

                        if let Some(first_chunk) = chunks.first() {
                            debug!(
                                "[{}] Editing original interaction response with first embed chunk ({} chars)",
                                request_id,
                                first_chunk.len()
                            );
                            let embed = Self::build_persona_embed(p, first_chunk);
                            command
                                .edit_original_interaction_response(&serenity_ctx.http, |response| {
                                    response.set_embed(embed)
                                })
                                .await
                                .map_err(|e| {
                                    error!(
                                        "[{request_id}] Failed to edit original interaction response: {e}"
                                    );
                                    anyhow::anyhow!("Failed to edit original response: {}", e)
                                })?;
                            info!("[{request_id}] Original embed response edited successfully");
                        }

                        // Send remaining chunks as follow-up embeds
                        for (i, chunk) in chunks.iter().skip(1).enumerate() {
                            if !chunk.trim().is_empty() {
                                debug!(
                                    "[{}] Sending follow-up embed {} of {} ({} chars)",
                                    request_id,
                                    i + 2,
                                    chunks.len(),
                                    chunk.len()
                                );
                                let embed = Self::build_continuation_embed(p, chunk);
                                command
                                    .create_followup_message(&serenity_ctx.http, |message| {
                                        message.set_embed(embed)
                                    })
                                    .await
                                    .map_err(|e| {
                                        error!(
                                            "[{}] Failed to send follow-up embed {}: {}",
                                            request_id,
                                            i + 2,
                                            e
                                        );
                                        anyhow::anyhow!("Failed to send follow-up message: {}", e)
                                    })?;
                                debug!(
                                    "[{}] Follow-up embed {} sent successfully",
                                    request_id,
                                    i + 2
                                );
                            }
                        }
                        info!("[{request_id}] All embed response chunks sent successfully");
                    } else {
                        debug!(
                            "[{}] Editing original interaction response with embed ({} chars)",
                            request_id,
                            ai_response.len()
                        );
                        let embed = Self::build_persona_embed(p, &ai_response);
                        command
                            .edit_original_interaction_response(&serenity_ctx.http, |response| {
                                response.set_embed(embed)
                            })
                            .await
                            .map_err(|e| {
                                error!(
                                    "[{request_id}] Failed to edit original interaction response: {e}"
                                );
                                anyhow::anyhow!("Failed to edit original response: {}", e)
                            })?;
                        info!("[{request_id}] Embed response edited successfully");
                    }
                } else {
                    // Plain text fallback (legacy behavior or embeds disabled)
                    let chunks = chunk_for_message(&ai_response);
                    if chunks.len() > 1 {
                        debug!(
                            "[{request_id}] Response too long, splitting into {} chunks",
                            chunks.len()
                        );

                        if let Some(first_chunk) = chunks.first() {
                            debug!(
                                "[{}] Editing original interaction response with first chunk ({} chars)",
                                request_id,
                                first_chunk.len()
                            );
                            command
                                .edit_original_interaction_response(&serenity_ctx.http, |response| {
                                    response.content(first_chunk)
                                })
                                .await
                                .map_err(|e| {
                                    error!(
                                        "[{request_id}] Failed to edit original interaction response: {e}"
                                    );
                                    anyhow::anyhow!("Failed to edit original response: {}", e)
                                })?;
                            info!("[{request_id}] Original interaction response edited successfully");
                        }

                        // Send remaining chunks as follow-up messages
                        for (i, chunk) in chunks.iter().skip(1).enumerate() {
                            if !chunk.trim().is_empty() {
                                debug!(
                                    "[{}] Sending follow-up message {} of {} ({} chars)",
                                    request_id,
                                    i + 2,
                                    chunks.len(),
                                    chunk.len()
                                );
                                command
                                    .create_followup_message(&serenity_ctx.http, |message| {
                                        message.content(chunk)
                                    })
                                    .await
                                    .map_err(|e| {
                                        error!(
                                            "[{}] Failed to send follow-up message {}: {}",
                                            request_id,
                                            i + 2,
                                            e
                                        );
                                        anyhow::anyhow!("Failed to send follow-up message: {}", e)
                                    })?;
                                debug!(
                                    "[{}] Follow-up message {} sent successfully",
                                    request_id,
                                    i + 2
                                );
                            }
                        }
                        info!("[{request_id}] All response chunks sent successfully");
                    } else {
                        debug!(
                            "[{}] Editing original interaction response with complete response ({} chars)",
                            request_id,
                            ai_response.len()
                        );
                        command
                            .edit_original_interaction_response(&serenity_ctx.http, |response| {
                                response.content(&ai_response)
                            })
                            .await
                            .map_err(|e| {
                                error!(
                                    "[{request_id}] Failed to edit original interaction response: {e}"
                                );
                                anyhow::anyhow!("Failed to edit original response: {}", e)
                            })?;
                        info!("[{request_id}] Original interaction response edited successfully");
                    }
                }

                let total_time = start_time.elapsed();
                info!("[{request_id}] AI command completed successfully | Total time: {total_time:?}");
            }
            Err(e) => {
                let processing_time = start_time.elapsed();
                error!("[{request_id}] OpenAI API error after {processing_time:?}: {e}");

                let error_message = if e.to_string().contains("timed out") {
                    debug!("[{request_id}] Error type: timeout");
                    "**Request timed out** - The AI service is taking too long to respond. Please try again with a shorter message or try again later."
                } else if e.to_string().contains("OpenAI API error") {
                    debug!("[{request_id}] Error type: OpenAI API error");
                    "**AI service error** - There's an issue with the AI service. Please try again in a moment."
                } else {
                    debug!("[{request_id}] Error type: unknown - {e}");
                    "**Error processing request** - Something went wrong. Please try again later."
                };

                debug!("[{request_id}] Sending error message to Discord: '{error_message}'");
                command
                    .edit_original_interaction_response(&serenity_ctx.http, |response| {
                        response.content(error_message)
                    })
                    .await
                    .map_err(|discord_err| {
                        error!(
                            "[{request_id}] Failed to send error message to Discord: {discord_err}"
                        );
                        anyhow::anyhow!("Failed to send error response: {}", discord_err)
                    })?;
                info!("[{request_id}] Error message sent to Discord successfully");

                let total_time = start_time.elapsed();
                error!("[{request_id}] AI command failed | Total time: {total_time:?}");
            }
        }

        Ok(())
    }

    /// Build an embed for a persona response
    fn build_persona_embed(persona: &Persona, response_text: &str) -> CreateEmbed {
        let mut embed = CreateEmbed::default();

        // Set author with persona name and optional portrait
        embed.author(|a| {
            a.name(&persona.name);
            if let Some(url) = &persona.portrait_url {
                a.icon_url(url);
            }
            a
        });

        // Set accent color
        embed.color(persona.color);

        // Response text (max 4096 chars for embed description)
        embed.description(truncate_for_embed(response_text));

        embed
    }

    /// Build a continuation embed for long responses (no author, just content)
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
    fn test_ai_chat_handler_commands() {
        let handler = AiChatHandler;
        let names = handler.command_names();

        assert!(names.contains(&"hey"));
        assert!(names.contains(&"explain"));
        assert!(names.contains(&"simple"));
        assert!(names.contains(&"steps"));
        assert!(names.contains(&"recipe"));
        assert_eq!(names.len(), 5);
    }
}
