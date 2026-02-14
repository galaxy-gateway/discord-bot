//! Council and conclude command handlers
//!
//! Handles: council, conclude
//!
//! - **Version**: 1.1.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.1.0: Use shared persona embed builders from core::embeds
//! - 1.0.0: Extracted from command_handler.rs

use anyhow::Result;
use async_trait::async_trait;
use log::{error, info};
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::sync::Arc;
use uuid::Uuid;

use crate::commands::context::{is_in_thread_channel, CommandContext};
use crate::commands::handler::SlashCommandHandler;
use crate::commands::slash::get_string_option;
use crate::core::persona_embed;
use crate::features::analytics::CostBucket;
use crate::features::council::{get_active_councils, CouncilState};
use crate::features::debate::get_active_debates;

/// Handler for /council and /conclude commands
pub struct CouncilHandler;

#[async_trait]
impl SlashCommandHandler for CouncilHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["council", "conclude"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();
        match command.data.name.as_str() {
            "council" => {
                self.handle_council(&ctx, serenity_ctx, command, request_id)
                    .await
            }
            "conclude" => {
                self.handle_conclude(&ctx, serenity_ctx, command, request_id)
                    .await
            }
            _ => Ok(()),
        }
    }
}

impl CouncilHandler {
    /// Handle /council command - convene multiple personas on a topic
    async fn handle_council(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        // Extract prompt
        let prompt = get_string_option(&command.data.options, "prompt")
            .ok_or_else(|| anyhow::anyhow!("Missing prompt argument"))?;

        // Extract optional rules parameter
        let rules = get_string_option(&command.data.options, "rules");

        // Collect all selected personas (persona1 and persona2 are required, 3-6 optional)
        let mut persona_ids: Vec<String> = Vec::new();
        for i in 1..=6 {
            let option_name = format!("persona{i}");
            if let Some(persona_id) = get_string_option(&command.data.options, &option_name) {
                // Skip duplicates
                if !persona_ids.contains(&persona_id) {
                    persona_ids.push(persona_id);
                }
            }
        }

        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id;
        let guild_id = command.guild_id.map(|id| id.to_string());

        info!(
            "[{request_id}] /council command | Personas: {persona_ids:?} | User: {user_id}"
        );

        // Validate we have at least 2 personas
        if persona_ids.len() < 2 {
            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.content("The council requires at least 2 different personas.")
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        // Validate all personas exist and collect their data
        let mut personas: Vec<crate::features::personas::Persona> = Vec::new();
        for persona_id in &persona_ids {
            if let Some(persona) = ctx.persona_manager.get_persona_with_portrait(persona_id) {
                personas.push(persona);
            } else {
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
        }

        // Build persona names list for display
        let persona_names: Vec<&str> = personas.iter().map(|p| p.name.as_str()).collect();
        let persona_list = persona_names.join(", ");

        info!(
            "[{request_id}] Council convened: {} personas on topic: '{}'",
            personas.len(),
            prompt.chars().take(50).collect::<String>()
        );

        // Check if we're already in a thread
        let in_thread = is_in_thread_channel(serenity_ctx, channel_id)
            .await
            .unwrap_or(false);

        // Check for existing council in this channel
        let existing_council = get_active_councils().contains_key(&channel_id.0);

        // Determine the thread_id to use
        let thread_id = if in_thread {
            if existing_council {
                command
                    .create_interaction_response(&serenity_ctx.http, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|m| {
                                m.content("A council is already active in this thread.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }

            // Run council directly in existing thread
            info!("[{request_id}] Running council in existing thread");

            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.content(format!(
                                "**Council Convening!**\n\n\
                                **Topic:** {prompt}\n\
                                **Council Members:** {persona_list}"
                            ))
                        })
                })
                .await?;

            let intro_embed = serenity::builder::CreateEmbed::default()
                .title("Council in Session")
                .description(format!(
                    "**Topic:** {prompt}\n\n\
                    **Council Members:** {persona_list}\n\n\
                    Each council member will now share their perspective."
                ))
                .color(0x9B59B6)
                .to_owned();

            let _ = channel_id
                .send_message(&serenity_ctx.http, |m| m.set_embed(intro_embed))
                .await;

            channel_id
        } else {
            // Not in a thread - create one
            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.content(format!(
                                "**Council Convening!**\n\n\
                                **Topic:** {prompt}\n\
                                **Council Members:** {persona_list}\n\n\
                                Creating thread for the council discussion..."
                            ))
                        })
                })
                .await?;

            let message = command
                .get_interaction_response(&serenity_ctx.http)
                .await?;

            let thread_name = format!(
                "Council: {}",
                if prompt.len() > 50 {
                    format!("{}...", &prompt[..47])
                } else {
                    prompt.clone()
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
                    error!("[{request_id}] Failed to create council thread: {e}");
                    command
                        .edit_original_interaction_response(&serenity_ctx.http, |r| {
                            r.content(format!(
                                "**Council Failed**\n\n\
                                Could not create thread. Make sure I have permission to create threads.\n\
                                Error: {e}"
                            ))
                        })
                        .await?;
                    return Ok(());
                }
            };

            command
                .edit_original_interaction_response(&serenity_ctx.http, |r| {
                    r.content(format!(
                        "**Council Convened!**\n\n\
                        **Topic:** {prompt}\n\
                        **Council Members:** {persona_list}\n\n\
                        The council is deliberating in the thread below!"
                    ))
                })
                .await?;

            let intro_embed = serenity::builder::CreateEmbed::default()
                .title("Council in Session")
                .description(format!(
                    "**Topic:** {prompt}\n\n\
                    **Council Members:** {persona_list}\n\n\
                    Each council member will now share their perspective."
                ))
                .color(0x9B59B6)
                .to_owned();

            let _ = thread
                .id
                .send_message(&serenity_ctx.http, |m| m.set_embed(intro_embed))
                .await;

            thread.id
        };

        // Log usage
        ctx.database.log_usage(&user_id, "council", None).await?;

        // Check for prior discussion context in this thread
        let prior_context = crate::features::detect_thread_context(thread_id.0);
        let prior_context_text = prior_context
            .as_ref()
            .map(|tc| crate::features::format_prior_context(tc, &ctx.persona_manager));

        // Create initial council state with rules and store it
        let council_state = CouncilState::with_rules(
            prompt.clone(),
            persona_ids.clone(),
            user_id.clone(),
            guild_id.clone(),
            rules.clone(),
        );
        get_active_councils().insert(thread_id.0, council_state);

        // Clone values needed for the async task
        let openai_model = ctx.openai_model.clone();
        let usage_tracker = ctx.usage_tracker.clone();
        let persona_manager = ctx.persona_manager.clone();
        let ctx_clone = serenity_ctx.clone();
        let prompt_clone = prompt.clone();
        let guild_id_clone = guild_id.clone();
        let channel_id_str = channel_id.to_string();
        let rules_clone = rules.clone();
        let prior_context_clone = prior_context_text.clone();

        // Spawn a task to get responses from each persona
        tokio::spawn(async move {
            // Add the initial user message to history
            if let Some(mut state) = get_active_councils().get_mut(&thread_id.0) {
                state.add_user_message(prompt_clone.clone());
            }

            for (i, persona_id) in persona_ids.iter().enumerate() {
                let persona = match persona_manager.get_persona_with_portrait(persona_id) {
                    Some(p) => p,
                    None => continue,
                };

                let system_prompt = persona_manager.get_system_prompt(persona_id, None);

                // Build rules section if provided
                let rules_section = rules_clone
                    .as_ref()
                    .map(|r| {
                        format!(
                            "\n\n## Ground Rules\nThe following rules and definitions apply to this discussion:\n{r}\n"
                        )
                    })
                    .unwrap_or_default();

                // Build prior context section if available
                let prior_section = prior_context_clone
                    .as_ref()
                    .map(|c| format!("\n\n{c}\n"))
                    .unwrap_or_default();

                let council_context = format!(
                    "{}{}{}\n\nYou are participating in a council discussion with other personas. \
                    Share your unique perspective on the topic. Be concise but thoughtful. \
                    You are speaking as {} - stay true to your character.",
                    system_prompt, rules_section, prior_section, persona.name
                );

                let messages = vec![
                    openai::chat::ChatCompletionMessage {
                        role: openai::chat::ChatCompletionMessageRole::System,
                        content: Some(council_context),
                        name: None,
                        function_call: None,
                        tool_call_id: None,
                        tool_calls: None,
                    },
                    openai::chat::ChatCompletionMessage {
                        role: openai::chat::ChatCompletionMessageRole::User,
                        content: Some(prompt_clone.clone()),
                        name: None,
                        function_call: None,
                        tool_call_id: None,
                        tool_calls: None,
                    },
                ];

                let response =
                    match openai::chat::ChatCompletion::builder(&openai_model, messages)
                        .create()
                        .await
                    {
                        Ok(completion) => {
                            if let Some(usage) = &completion.usage {
                                usage_tracker.log_chat(
                                    &openai_model,
                                    usage.prompt_tokens,
                                    usage.completion_tokens,
                                    usage.total_tokens,
                                    &user_id,
                                    guild_id_clone.as_deref(),
                                    Some(&channel_id_str),
                                    Some(&request_id.to_string()),
                                    CostBucket::Council,
                                );
                            }

                            completion
                                .choices
                                .first()
                                .and_then(|c| c.message.content.clone())
                                .unwrap_or_else(|| "I have no words at this time.".to_string())
                        }
                        Err(e) => {
                            error!(
                                "[{request_id}] Council: Failed to get response from {}: {}",
                                persona.name, e
                            );
                            format!("*{} is momentarily lost in thought...*", persona.name)
                        }
                    };

                // Add response to council history
                if let Some(mut state) = get_active_councils().get_mut(&thread_id.0) {
                    state.add_persona_response(persona_id, response.clone());
                }

                // Build embed for this persona's response
                let embed = persona_embed(&persona, &response);

                if let Err(e) = thread_id
                    .send_message(&ctx_clone.http, |m| m.set_embed(embed.clone()))
                    .await
                {
                    error!(
                        "[{request_id}] Council: Failed to send message from {}: {}",
                        persona.name, e
                    );
                }

                // Small delay between responses for natural pacing
                if i < persona_ids.len() - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }

            // Mark opening statements as complete
            if let Some(mut state) = get_active_councils().get_mut(&thread_id.0) {
                state.mark_opening_complete();
            }

            // Post interactive control buttons
            let control_embed = serenity::builder::CreateEmbed::default()
                .title("Council Awaiting Direction")
                .description(
                    "All council members have shared their opening perspectives.\n\n\
                    **Select a council member** to hear more from them, or:\n\
                    - **Continue Discussion** - All members respond to what has been said\n\
                    - **Dismiss Council** - End this council session\n\n\
                    You can also mention me with a follow-up question!",
                )
                .color(0x9B59B6)
                .to_owned();

            let buttons = crate::features::create_council_buttons(
                thread_id.0,
                &persona_ids,
                &persona_manager,
            );

            let _ = thread_id
                .send_message(&ctx_clone.http, |m| {
                    m.set_embed(control_embed).set_components(buttons)
                })
                .await;

            info!("[{request_id}] Council opening statements completed, awaiting interaction");
        });

        Ok(())
    }

    /// Handle /conclude command - end active council or debate session
    async fn handle_conclude(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let channel_id = command.channel_id.0;

        info!("[{request_id}] /conclude command in channel {channel_id}");

        // Check for active council
        if let Some((_, council_state)) = get_active_councils().remove(&channel_id) {
            info!(
                "[{request_id}] Concluding council session on topic: {}",
                council_state.topic
            );

            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.embed(|e| {
                                e.title("Council Concluded")
                                    .description(format!(
                                        "**Topic:** {}\n\n\
                                        **Participants:** {}\n\n\
                                        This council session has been formally concluded.",
                                        council_state.topic,
                                        council_state
                                            .persona_ids
                                            .iter()
                                            .filter_map(|id| ctx
                                                .persona_manager
                                                .get_persona(id)
                                                .map(|p| p.name.clone()))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ))
                                    .color(0x9B59B6)
                            })
                        })
                })
                .await?;

            return Ok(());
        }

        // Check for active debate
        if let Some((_, debate_state)) = get_active_debates().remove(&channel_id) {
            info!(
                "[{request_id}] Concluding debate session on topic: {}",
                debate_state.config.topic
            );

            let p1_name = ctx
                .persona_manager
                .get_persona(&debate_state.config.persona1_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| debate_state.config.persona1_id.clone());
            let p2_name = ctx
                .persona_manager
                .get_persona(&debate_state.config.persona2_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| debate_state.config.persona2_id.clone());

            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.embed(|e| {
                                e.title("Debate Concluded")
                                    .description(format!(
                                        "**Topic:** {}\n\n\
                                        **Debaters:** {} vs {}\n\
                                        **Total Rounds:** {}\n\n\
                                        This debate has been formally concluded.",
                                        debate_state.config.topic,
                                        p1_name,
                                        p2_name,
                                        debate_state.total_rounds_completed
                                    ))
                                    .color(0x7289DA)
                            })
                        })
                })
                .await?;

            return Ok(());
        }

        // No active session found
        command
            .create_interaction_response(&serenity_ctx.http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| {
                        m.content("No active council or debate session found in this thread.")
                            .ephemeral(true)
                    })
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_council_handler_commands() {
        let handler = CouncilHandler;
        let names = handler.command_names();

        assert!(names.contains(&"council"));
        assert!(names.contains(&"conclude"));
        assert_eq!(names.len(), 2);
    }
}
