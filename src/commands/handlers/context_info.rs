//! Context info command handler
//!
//! Handles: context
//!
//! - **Version**: 1.0.0
//! - **Since**: 4.4.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info};
use serenity::builder::GetMessages;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::sync::Arc;

use crate::commands::context::{is_in_thread_channel, CommandContext};
use crate::commands::handler::SlashCommandHandler;

/// Approximate characters per token (standard GPT heuristic)
const CHARS_PER_TOKEN: usize = 4;

/// Overhead tokens for message formatting, role tags, separators
const OVERHEAD_TOKENS: usize = 50;

/// Default context limit for mentions and DMs
const DEFAULT_MENTION_CONTEXT_LIMIT: i64 = 40;

pub struct ContextInfoHandler;

#[async_trait]
impl SlashCommandHandler for ContextInfoHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["context"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        self.handle_context_info(&ctx, serenity_ctx, command).await
    }
}

impl ContextInfoHandler {
    async fn handle_context_info(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id;
        let guild_id = command.guild_id.map(|id| id.to_string());

        info!("/context command | User: {user_id} | Channel: {channel_id}");

        // Detect channel type
        let in_thread = is_in_thread_channel(serenity_ctx, channel_id).await?;
        let is_dm = command.guild_id.is_none();

        let location_type = if in_thread {
            "Thread"
        } else if is_dm {
            "DM"
        } else {
            "Guild Channel"
        };

        // Resolve active persona
        let persona_id = if let Some(ref gid) = guild_id {
            ctx.database
                .get_persona_with_channel(&user_id, gid, &channel_id.to_string())
                .await?
        } else {
            ctx.database
                .get_user_persona_with_guild(&user_id, None)
                .await?
        };

        // Get system prompt
        let system_prompt = ctx.persona_manager.get_system_prompt(&persona_id, None);
        let system_prompt_chars = system_prompt.len();
        let system_prompt_tokens = system_prompt_chars / CHARS_PER_TOKEN;

        // Get context limit setting
        let context_limit = if let Some(ref gid) = guild_id {
            ctx.database
                .get_guild_setting(gid, "max_context_messages")
                .await?
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(DEFAULT_MENTION_CONTEXT_LIMIT)
        } else {
            DEFAULT_MENTION_CONTEXT_LIMIT
        };

        // Fetch conversation history
        let (history, context_source) = if in_thread {
            let messages = channel_id
                .messages(&serenity_ctx.http, |builder: &mut GetMessages| {
                    builder.limit(context_limit.min(100) as u64)
                })
                .await
                .unwrap_or_default();

            let bot_id = serenity_ctx.http.get_current_user().await?.id;
            let history: Vec<(String, String)> = messages
                .iter()
                .rev()
                .filter(|m| !m.content.is_empty())
                .map(|m| {
                    let role = if m.author.id == bot_id {
                        "assistant".to_string()
                    } else {
                        "user".to_string()
                    };
                    (role, m.content.clone())
                })
                .collect();

            (history, "Discord Thread")
        } else {
            let history = ctx
                .database
                .get_conversation_history(
                    &user_id,
                    &channel_id.to_string(),
                    context_limit,
                )
                .await
                .unwrap_or_default();

            (history, "Database")
        };

        let message_count = history.len();
        let history_chars: usize = history.iter().map(|(_, content)| content.len()).sum();
        let history_tokens = history_chars / CHARS_PER_TOKEN;
        let total_tokens = system_prompt_tokens + history_tokens + OVERHEAD_TOKENS;

        debug!(
            "/context | Persona: {persona_id} | Messages: {message_count} | Total tokens: ~{total_tokens}"
        );

        // Get channel name for display
        let channel_display = if is_dm {
            "Direct Message".to_string()
        } else {
            format!("<#{channel_id}>")
        };

        // Get persona info for embed styling
        let persona = ctx.persona_manager.get_persona_with_portrait(&persona_id);
        let (persona_name, persona_color, portrait_url) = match &persona {
            Some(p) => (p.name.as_str(), p.color, p.portrait_url.as_deref()),
            None => (persona_id.as_str(), 0x95a5a6, None),
        };

        // Build embed description
        let description = format!(
            "**Location:** {channel_display} ({location_type})\n\
             **Active Persona:** {persona_name}\n\
             **Model:** {model}\n\
             \n\
             **System Prompt:** ~{sys_tokens} tokens ({sys_chars} chars)\n\
             **History:** {msg_count} messages (~{hist_tokens} tokens, {hist_chars} chars)\n\
             **Overhead:** ~{overhead} tokens (formatting)\n\
             \n\
             **Total Estimate:** ~{total} tokens\n\
             **Context Limit:** {limit} messages (using {msg_count})\n\
             **Context Source:** {source}",
            model = ctx.openai_model,
            sys_tokens = format_number(system_prompt_tokens),
            sys_chars = format_number(system_prompt_chars),
            msg_count = message_count,
            hist_tokens = format_number(history_tokens),
            hist_chars = format_number(history_chars),
            overhead = OVERHEAD_TOKENS,
            total = format_number(total_tokens),
            limit = context_limit,
            source = context_source,
        );

        // Send ephemeral response
        command
            .create_interaction_response(&serenity_ctx.http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| {
                        m.ephemeral(true).embed(|e| {
                            e.title("Context Window");
                            e.description(&description);
                            e.color(persona_color);
                            if let Some(url) = portrait_url {
                                e.thumbnail(url);
                            }
                            e
                        })
                    })
            })
            .await?;

        info!("/context response sent | User: {user_id} | {message_count} messages, ~{total_tokens} tokens");
        Ok(())
    }
}

/// Format a number with comma separators for readability
fn format_number(n: usize) -> String {
    let s = n.to_string();
    if s.len() <= 3 {
        return s;
    }
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_info_handler_commands() {
        let handler = ContextInfoHandler;
        let names = handler.command_names();
        assert!(names.contains(&"context"));
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(42), "42");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1234), "1,234");
        assert_eq!(format_number(12345), "12,345");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn test_chars_per_token_constant() {
        // ~4 chars/token is the standard GPT heuristic
        assert_eq!(CHARS_PER_TOKEN, 4);
    }
}
