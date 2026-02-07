//! Shared context for command handlers
//!
//! - **Version**: 1.1.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.1.0: Add ImageGenerator for imagine command
//! - 1.0.0: Initial implementation with core shared state

use crate::database::Database;
use crate::features::analytics::{CostBucket, InteractionTracker, UsageTracker};
use crate::features::image_gen::generator::ImageGenerator;
use crate::features::personas::PersonaManager;
use anyhow::Result;
use log::debug;
use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

/// Shared context for all command handlers
///
/// Contains the core services needed by most command handlers:
/// - PersonaManager for AI personality handling
/// - Database for persistence
/// - UsageTracker for cost tracking
/// - InteractionTracker for analytics
/// - ImageGenerator for DALL-E image generation
/// - OpenAI configuration
/// - Bot start time for uptime tracking
#[derive(Clone)]
pub struct CommandContext {
    pub persona_manager: PersonaManager,
    pub database: Database,
    pub usage_tracker: UsageTracker,
    pub interaction_tracker: InteractionTracker,
    pub image_generator: ImageGenerator,
    pub openai_model: String,
    pub start_time: std::time::Instant,
}

impl CommandContext {
    /// Create a new CommandContext with the given services
    pub fn new(
        persona_manager: PersonaManager,
        database: Database,
        usage_tracker: UsageTracker,
        interaction_tracker: InteractionTracker,
        image_generator: ImageGenerator,
        openai_model: String,
    ) -> Self {
        Self {
            persona_manager,
            database,
            usage_tracker,
            interaction_tracker,
            image_generator,
            openai_model,
            start_time: std::time::Instant::now(),
        }
    }

    /// Create a CommandContext with a specific start time (for sharing with existing handler)
    pub fn with_start_time(
        persona_manager: PersonaManager,
        database: Database,
        usage_tracker: UsageTracker,
        interaction_tracker: InteractionTracker,
        image_generator: ImageGenerator,
        openai_model: String,
        start_time: std::time::Instant,
    ) -> Self {
        Self {
            persona_manager,
            database,
            usage_tracker,
            interaction_tracker,
            image_generator,
            openai_model,
            start_time,
        }
    }

    /// Get AI response with conversation context
    ///
    /// This is the core OpenAI integration for command handlers.
    ///
    /// # Arguments
    ///
    /// * `system_prompt` - The system prompt defining the AI personality
    /// * `user_message` - The user's message to respond to
    /// * `history` - Conversation history as (role, content) pairs
    /// * `request_id` - Unique request ID for logging
    /// * `user_id` - Optional user ID for tracking
    /// * `guild_id` - Optional guild ID for tracking
    /// * `channel_id` - Optional channel ID for tracking
    /// * `cost_bucket` - Cost categorization for analytics
    #[allow(clippy::too_many_arguments)]
    pub async fn get_ai_response(
        &self,
        system_prompt: &str,
        user_message: &str,
        history: Vec<(String, String)>,
        request_id: Uuid,
        user_id: Option<&str>,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
        cost_bucket: CostBucket,
    ) -> Result<String> {
        debug!("[{request_id}] Building AI request with {} history messages", history.len());

        // Build messages array
        let mut messages = vec![ChatCompletionMessage {
            role: ChatCompletionMessageRole::System,
            content: Some(system_prompt.to_string()),
            name: None,
            function_call: None,
            tool_call_id: None,
            tool_calls: None,
        }];

        // Add conversation history
        for (role, content) in history {
            let role = match role.as_str() {
                "user" => ChatCompletionMessageRole::User,
                "assistant" => ChatCompletionMessageRole::Assistant,
                _ => continue,
            };
            messages.push(ChatCompletionMessage {
                role,
                content: Some(content),
                name: None,
                function_call: None,
                tool_call_id: None,
                tool_calls: None,
            });
        }

        // Add current user message
        messages.push(ChatCompletionMessage {
            role: ChatCompletionMessageRole::User,
            content: Some(user_message.to_string()),
            name: None,
            function_call: None,
            tool_call_id: None,
            tool_calls: None,
        });

        debug!("[{request_id}] Sending {} messages to OpenAI", messages.len());

        // Call OpenAI API with timeout
        let completion = timeout(
            Duration::from_secs(45),
            ChatCompletion::builder(&self.openai_model, messages).create(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("OpenAI request timed out after 45 seconds"))??;

        let response = completion
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default()
            .trim()
            .to_string();

        debug!("[{request_id}] Got response: {} chars", response.len());

        // Track usage
        if let Some(usage) = &completion.usage {
            self.usage_tracker.log_chat(
                &self.openai_model,
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens,
                user_id.unwrap_or("unknown"),
                guild_id,
                channel_id,
                Some(&request_id.to_string()),
                cost_bucket,
            );
        }

        Ok(response)
    }

    /// Get AI response without history (simple single-turn)
    pub async fn get_simple_ai_response(
        &self,
        system_prompt: &str,
        user_message: &str,
        request_id: Uuid,
        cost_bucket: CostBucket,
    ) -> Result<String> {
        self.get_ai_response(
            system_prompt,
            user_message,
            Vec::new(),
            request_id,
            None,
            None,
            None,
            cost_bucket,
        )
        .await
    }
}

/// Check if a channel is a thread (public or private)
pub async fn is_in_thread_channel(
    serenity_ctx: &serenity::prelude::Context,
    channel_id: serenity::model::id::ChannelId,
) -> Result<bool> {
    use serenity::model::channel::{Channel, ChannelType};

    match serenity_ctx.http.get_channel(channel_id.0).await {
        Ok(Channel::Guild(guild_channel)) => Ok(matches!(
            guild_channel.kind,
            ChannelType::PublicThread | ChannelType::PrivateThread
        )),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_context_clone() {
        // CommandContext should be Clone for sharing across handlers
        fn assert_clone<T: Clone>() {}
        assert_clone::<CommandContext>();
    }
}
