//! Image generation command handlers
//!
//! Handles: imagine
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from command_handler.rs

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, error, info};
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::channel::AttachmentType;
use serenity::prelude::Context;
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Instant;

use crate::commands::context::CommandContext;
use crate::commands::handler::SlashCommandHandler;
use crate::commands::slash::get_string_option;
use crate::features::analytics::CostBucket;
use crate::features::image_gen::generator::{ImageSize, ImageStyle};

/// Handler for DALL-E image generation command
pub struct ImagineHandler;

#[async_trait]
impl SlashCommandHandler for ImagineHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["imagine"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        match command.data.name.as_str() {
            "imagine" => self.handle_imagine(&ctx, serenity_ctx, command).await,
            _ => Ok(()),
        }
    }
}

impl ImagineHandler {
    /// Handle /imagine command - generate an image with DALL-E
    async fn handle_imagine(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let start_time = Instant::now();
        let user_id = command.user.id.to_string();

        // Check if image_generation feature is enabled for this guild
        let guild_id = command.guild_id.map(|id| id.to_string());
        let guild_id_opt = guild_id.as_deref();
        let image_gen_enabled = if let Some(gid) = guild_id_opt {
            ctx.database
                .is_feature_enabled("image_generation", None, Some(gid))
                .await?
        } else {
            true // Always enabled in DMs
        };

        if !image_gen_enabled {
            command
                .create_interaction_response(&serenity_ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| {
                            msg.content("Image generation is disabled on this server.")
                        })
                })
                .await?;
            return Ok(());
        }

        debug!("Starting image generation | Command: imagine");

        // Get the prompt (required)
        let prompt = get_string_option(&command.data.options, "prompt")
            .ok_or_else(|| anyhow::anyhow!("Missing prompt parameter"))?;

        // Get optional size (default: square)
        let size = get_string_option(&command.data.options, "size")
            .and_then(|s| ImageSize::parse(&s))
            .unwrap_or(ImageSize::Square);

        // Get optional style (default: vivid)
        let style = get_string_option(&command.data.options, "style")
            .and_then(|s| ImageStyle::parse(&s))
            .unwrap_or(ImageStyle::Vivid);

        info!(
            "Generating image | User: {} | Size: {} | Style: {} | Prompt: '{}'",
            user_id,
            size.as_str(),
            style.as_str(),
            prompt.chars().take(100).collect::<String>()
        );

        // Log usage
        ctx.database.log_usage(&user_id, "imagine", None).await?;

        // Defer the response immediately (DALL-E can take 10-30 seconds)
        info!("Deferring Discord interaction response (DALL-E generation)");
        command
            .create_interaction_response(&serenity_ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .map_err(|e| {
                error!("Failed to defer interaction response: {e}");
                anyhow::anyhow!("Failed to defer interaction: {}", e)
            })?;

        // Generate the image
        let channel_id_str = command.channel_id.to_string();
        match ctx
            .image_generator
            .generate_image(&prompt, size, style)
            .await
        {
            Ok(generated_image) => {
                let generation_time = start_time.elapsed();
                info!("Image generated | Time: {generation_time:?}");

                // Log DALL-E usage
                ctx.usage_tracker.log_dalle(
                    size.as_str(),
                    "standard", // DALL-E 3 via this bot uses standard quality
                    1,          // One image per request
                    &user_id,
                    guild_id_opt,
                    Some(&channel_id_str),
                    CostBucket::Imagine,
                );

                // Download the image
                match ctx.image_generator.download_image(&generated_image.url).await {
                    Ok(image_bytes) => {
                        debug!("Image downloaded | Size: {} bytes", image_bytes.len());

                        // Build the response message
                        let mut response_text = format!("**Generated Image**\n> {prompt}");
                        if let Some(revised) = &generated_image.revised_prompt {
                            if revised != &prompt {
                                response_text
                                    .push_str(&format!("\n\n*DALL-E revised prompt:* _{revised}_"));
                            }
                        }

                        // Edit the deferred response to show we're sending the image
                        command
                            .edit_original_interaction_response(&serenity_ctx.http, |response| {
                                response.content(&response_text)
                            })
                            .await
                            .map_err(|e| {
                                error!("Failed to edit interaction response: {e}");
                                anyhow::anyhow!("Failed to edit response: {}", e)
                            })?;

                        // Send the image as a followup message with attachment
                        command
                            .create_followup_message(&serenity_ctx.http, |message| {
                                message.add_file(AttachmentType::Bytes {
                                    data: Cow::Owned(image_bytes),
                                    filename: "generated_image.png".to_string(),
                                })
                            })
                            .await
                            .map_err(|e| {
                                error!("Failed to send image attachment: {e}");
                                anyhow::anyhow!("Failed to send image: {}", e)
                            })?;

                        let total_time = start_time.elapsed();
                        info!("Image sent successfully | Total time: {total_time:?}");
                    }
                    Err(e) => {
                        error!("Failed to download image: {e}");
                        command
                            .edit_original_interaction_response(&serenity_ctx.http, |response| {
                                response.content(
                                    "**Error** - Failed to download the generated image. Please try again.",
                                )
                            })
                            .await?;
                    }
                }
            }
            Err(e) => {
                let processing_time = start_time.elapsed();
                error!("DALL-E error after {processing_time:?}: {e}");

                let error_message = if e.to_string().contains("content_policy")
                    || e.to_string().contains("safety")
                {
                    "**Content Policy Violation** - Your prompt was rejected by DALL-E's safety system. Please try a different prompt."
                } else if e.to_string().contains("rate") || e.to_string().contains("limit") {
                    "**Rate Limited** - Too many image requests. Please wait a moment and try again."
                } else if e.to_string().contains("billing") || e.to_string().contains("quota") {
                    "**Quota Exceeded** - The image generation quota has been reached. Please try again later."
                } else {
                    "**Error** - Failed to generate image. Please try again with a different prompt."
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
    fn test_imagine_handler_commands() {
        let handler = ImagineHandler;
        let names = handler.command_names();

        assert!(names.contains(&"imagine"));
        assert_eq!(names.len(), 1);
    }
}
