//! Debate command handler
//!
//! Handles: debate
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from command_handler.rs

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, error, info, warn};
use serenity::builder::GetMessages;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::id::ChannelId;
use serenity::prelude::Context;
use std::sync::Arc;
use uuid::Uuid;

use crate::commands::context::{is_in_thread_channel, CommandContext};
use crate::commands::handler::SlashCommandHandler;
use crate::commands::slash::{
    debate::{DEFAULT_RESPONSES, MAX_RESPONSES, MIN_RESPONSES},
    get_integer_option, get_string_option,
};
use crate::features::analytics::CostBucket;
use crate::features::debate::{
    get_active_debates, orchestrator::DebateConfig, DebateOrchestrator,
};

/// Handler for /debate command - multi-persona debates
pub struct DebateHandler;

#[async_trait]
impl SlashCommandHandler for DebateHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["debate"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();
        self.handle_debate(&ctx, serenity_ctx, command, request_id)
            .await
    }
}

impl DebateHandler {
    /// Handle /debate command
    async fn handle_debate(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        // Extract command options
        let persona1_id = get_string_option(&command.data.options, "persona1")
            .ok_or_else(|| anyhow::anyhow!("Missing persona1 argument"))?;
        let persona2_id = get_string_option(&command.data.options, "persona2")
            .ok_or_else(|| anyhow::anyhow!("Missing persona2 argument"))?;
        let topic = get_string_option(&command.data.options, "topic")
            .ok_or_else(|| anyhow::anyhow!("Missing topic argument"))?;
        let rounds = get_integer_option(&command.data.options, "rounds")
            .unwrap_or(DEFAULT_RESPONSES)
            .clamp(MIN_RESPONSES, MAX_RESPONSES);

        // Extract optional rules parameter
        let rules = get_string_option(&command.data.options, "rules");

        // Determine if this is opening-only mode (default: 2 rounds = opening statements)
        let opening_only = rounds <= 2;

        // Validate personas are different
        if persona1_id == persona2_id {
            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .content("Please select two different personas for the debate.")
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        // Validate personas exist
        let orchestrator = DebateOrchestrator::new();
        let persona1 = ctx.persona_manager.get_persona(&persona1_id);
        let persona2 = ctx.persona_manager.get_persona(&persona2_id);

        if persona1.is_none() || persona2.is_none() {
            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .content("One or both selected personas are invalid.")
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        let persona1_name = persona1.unwrap().name.clone();
        let persona2_name = persona2.unwrap().name.clone();

        // Check if we're already in a thread with an existing debate
        let channel_id = command.channel_id;
        let existing_debate = get_active_debates().get(&channel_id.0).map(|d| d.clone());

        // Determine if this is a tag-team debate (joining an existing one)
        let (thread_id, initial_history, previous_debaters) = if let Some(prev_state) =
            existing_debate
        {
            // We're in a thread with an existing debate - this is a tag-team!
            let prev_p1 = ctx
                .persona_manager
                .get_persona(&prev_state.config.persona1_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| prev_state.config.persona1_id.clone());
            let prev_p2 = ctx
                .persona_manager
                .get_persona(&prev_state.config.persona2_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| prev_state.config.persona2_id.clone());

            info!(
                "[{request_id}] Tag-team debate: {persona1_name} & {persona2_name} taking over from {prev_p1} & {prev_p2} on '{topic}'"
            );

            // Fetch ALL messages from the thread
            let thread_history =
                Self::fetch_thread_history(serenity_ctx, channel_id, request_id)
                    .await
                    .unwrap_or_else(|e| {
                        warn!(
                            "[{request_id}] Failed to fetch thread history, falling back to debate state: {e}"
                        );
                        prev_state.history.clone()
                    });

            // Send response in the thread
            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.embed(|e| {
                                e.title("New Debaters Entering!")
                                    .description(format!(
                                        "**{prev_p1} and {prev_p2}** are stepping aside.\n\n\
                                        **{persona1_name} vs {persona2_name}** will now continue the debate on:\n\
                                        *{topic}*\n\n\
                                        The new debaters have reviewed everything said so far."
                                    ))
                                    .color(0xF39C12) // Gold for transition
                            })
                        })
                })
                .await?;

            (channel_id, Some(thread_history), Some((prev_p1, prev_p2)))
        } else {
            // Check if we're already in a thread (without an existing debate)
            let in_thread = is_in_thread_channel(serenity_ctx, channel_id)
                .await
                .unwrap_or(false);

            if in_thread {
                // Already in a thread - run debate directly here
                info!(
                    "[{request_id}] Starting debate in existing thread: {persona1_name} vs {persona2_name} on '{topic}' ({rounds} rounds)"
                );

                // Send response message (no thread creation)
                command
                    .create_interaction_response(&serenity_ctx.http, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|m| {
                                m.content(format!(
                                    "**Debate Starting!**\n\n\
                                    **Topic:** {topic}\n\
                                    **Debaters:** {persona1_name} vs {persona2_name}\n\
                                    **Rounds:** {rounds}"
                                ))
                            })
                    })
                    .await?;

                // Post introduction embed in the current thread
                let intro_embed = serenity::builder::CreateEmbed::default()
                    .title("Debate Beginning")
                    .description(format!(
                        "**Topic:** {topic}\n\n\
                        **{persona1_name} vs {persona2_name}**\n\n\
                        {rounds} rounds of debate ahead. Let the discourse begin!"
                    ))
                    .color(0x7289DA) // Discord blurple
                    .to_owned();

                let _ = channel_id
                    .send_message(&serenity_ctx.http, |m| m.set_embed(intro_embed))
                    .await;

                (channel_id, None, None)
            } else {
                // Not in a thread - create one
                info!(
                    "[{request_id}] Starting debate: {persona1_name} vs {persona2_name} on '{topic}' ({rounds} rounds)"
                );

                // Send initial response message
                command
                    .create_interaction_response(&serenity_ctx.http, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|m| {
                                m.content(format!(
                                    "**Debate Starting!**\n\n\
                                    **Topic:** {topic}\n\
                                    **Debaters:** {persona1_name} vs {persona2_name}\n\
                                    **Rounds:** {rounds}"
                                ))
                            })
                    })
                    .await?;

                // Get the message we just sent to create a thread from it
                let message = command.get_interaction_response(&serenity_ctx.http).await?;

                // Create the debate thread from the message
                let thread_name = format!(
                    "{} vs {} - {}",
                    persona1_name,
                    persona2_name,
                    if topic.len() > 40 {
                        format!("{}...", &topic[..37])
                    } else {
                        topic.clone()
                    }
                );

                let thread = match channel_id
                    .create_public_thread(&serenity_ctx.http, message.id, |t| {
                        t.name(&thread_name).auto_archive_duration(60)
                    })
                    .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        error!("[{request_id}] Failed to create debate thread: {e}");
                        command
                            .edit_original_interaction_response(&serenity_ctx.http, |r| {
                                r.content(format!(
                                    "**Debate Failed**\n\n\
                                    Could not create thread. Make sure I have permission to create threads.\n\
                                    Error: {e}"
                                ))
                            })
                            .await?;
                        return Ok(());
                    }
                };

                // Update the original message
                command
                    .edit_original_interaction_response(&serenity_ctx.http, |r| {
                        r.content(format!(
                            "**Debate Started!**\n\n\
                            **Topic:** {topic}\n\
                            **Debaters:** {persona1_name} vs {persona2_name}\n\
                            **Rounds:** {rounds}\n\n\
                            The debate is happening in the thread below!"
                        ))
                    })
                    .await?;

                // Post introduction in the thread
                let intro_embed = serenity::builder::CreateEmbed::default()
                    .title("Debate Beginning")
                    .description(format!(
                        "**Topic:** {topic}\n\n\
                        **{persona1_name} vs {persona2_name}**\n\n\
                        {rounds} rounds of debate ahead. Let the discourse begin!"
                    ))
                    .color(0x7289DA)
                    .to_owned();

                let _ = thread
                    .id
                    .send_message(&serenity_ctx.http, |m| m.set_embed(intro_embed))
                    .await;

                (thread.id, None, None)
            }
        };

        // Check for prior discussion context (interoperability with council)
        let prior_context = crate::features::detect_thread_context(thread_id.0);
        let _prior_context_text = prior_context
            .as_ref()
            .map(|tc| crate::features::format_prior_context(tc, &ctx.persona_manager));

        // Create debate config with optional initial history and rules
        let config = DebateConfig {
            persona1_id: persona1_id.clone(),
            persona2_id: persona2_id.clone(),
            topic: topic.clone(),
            rounds: if rounds == 0 { 2 } else { rounds },
            initiator_id: command.user.id.to_string(),
            guild_id: command.guild_id.map(|g| g.to_string()),
            initial_history,
            previous_debaters,
            rules: rules.clone(),
            opening_only,
        };

        // Clone what we need for the async closure
        let openai_model = ctx.openai_model.clone();
        let usage_tracker = ctx.usage_tracker.clone();
        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|g| g.to_string());
        let channel_id_str = thread_id.to_string();

        // Run the debate (this spawns the orchestrator)
        let ctx_clone = serenity_ctx.clone();

        tokio::spawn(async move {
            // Create a closure for getting AI responses
            let get_response = |system_prompt: String,
                                user_message: String,
                                history: Vec<(String, String)>| {
                let model = openai_model.clone();
                let tracker = usage_tracker.clone();
                let uid = user_id.clone();
                let gid = guild_id.clone();
                let cid = channel_id_str.clone();

                async move {
                    // Build messages for OpenAI
                    let mut messages = vec![openai::chat::ChatCompletionMessage {
                        role: openai::chat::ChatCompletionMessageRole::System,
                        content: Some(system_prompt),
                        name: None,
                        function_call: None,
                        tool_call_id: None,
                        tool_calls: None,
                    }];

                    for (role, content) in history {
                        let message_role = if role == "user" {
                            openai::chat::ChatCompletionMessageRole::User
                        } else {
                            openai::chat::ChatCompletionMessageRole::Assistant
                        };
                        messages.push(openai::chat::ChatCompletionMessage {
                            role: message_role,
                            content: Some(content),
                            name: None,
                            function_call: None,
                            tool_call_id: None,
                            tool_calls: None,
                        });
                    }

                    messages.push(openai::chat::ChatCompletionMessage {
                        role: openai::chat::ChatCompletionMessageRole::User,
                        content: Some(user_message),
                        name: None,
                        function_call: None,
                        tool_call_id: None,
                        tool_calls: None,
                    });

                    let chat_completion = openai::chat::ChatCompletion::builder(&model, messages)
                        .create()
                        .await
                        .map_err(|e| anyhow::anyhow!("OpenAI API error: {}", e))?;

                    if let Some(usage) = &chat_completion.usage {
                        tracker.log_chat(
                            &model,
                            usage.prompt_tokens,
                            usage.completion_tokens,
                            usage.total_tokens,
                            &uid,
                            gid.as_deref(),
                            Some(&cid),
                            None, // No request_id for debate turns
                            CostBucket::Debate,
                        );
                    }

                    chat_completion
                        .choices
                        .first()
                        .and_then(|c| c.message.content.clone())
                        .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))
                }
            };

            if let Err(e) = orchestrator
                .run_debate(&ctx_clone, thread_id, config, get_response)
                .await
            {
                error!("Debate failed: {e}");
                let _ = thread_id
                    .send_message(&ctx_clone.http, |m| {
                        m.content("The debate encountered an error and could not continue.")
                    })
                    .await;
            }
        });

        Ok(())
    }

    /// Fetch thread history for tag-team debate context
    async fn fetch_thread_history(
        serenity_ctx: &Context,
        channel_id: ChannelId,
        request_id: Uuid,
    ) -> Result<Vec<(String, String)>> {
        debug!("[{request_id}] Fetching thread history for tag-team debate");

        let messages = channel_id
            .messages(&serenity_ctx.http, |builder: &mut GetMessages| {
                builder.limit(100)
            })
            .await?;

        debug!(
            "[{}] Retrieved {} messages from debate thread",
            request_id,
            messages.len()
        );

        let current_user = serenity_ctx.http.get_current_user().await?;
        let bot_id = current_user.id;

        let mut history: Vec<(String, String)> = Vec::new();

        for msg in messages.iter().rev() {
            if !msg.embeds.is_empty() {
                for embed in &msg.embeds {
                    if let Some(author) = &embed.author {
                        if let Some(description) = &embed.description {
                            let speaker = author.name.clone();
                            let content = description.clone();
                            if !content.is_empty() {
                                history.push((speaker, content));
                            }
                        }
                    }
                }
            } else if !msg.content.is_empty() {
                let speaker = if msg.author.id == bot_id {
                    "Assistant".to_string()
                } else {
                    format!("User ({})", msg.author.name)
                };
                history.push((speaker, msg.content.clone()));
            }
        }

        debug!(
            "[{}] Processed {} entries from debate thread for tag-team context",
            request_id,
            history.len()
        );

        Ok(history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debate_handler_commands() {
        let handler = DebateHandler;
        let names = handler.command_names();

        assert!(names.contains(&"debate"));
        assert_eq!(names.len(), 1);
    }
}
