use crate::commands::context::CommandContext;
use crate::commands::handlers::create_all_handlers;
use crate::commands::registry::CommandRegistry;
use crate::core::{chunk_for_embed, chunk_for_message, truncate_for_embed};
use crate::database::Database;
use crate::features::analytics::{CostBucket, InteractionTracker, UsageTracker};
use crate::features::audio::transcriber::AudioTranscriber;
use crate::features::conflict::{ConflictDetector, ConflictMediator};
use crate::features::council::get_active_councils;
use crate::features::image_gen::generator::ImageGenerator;
use crate::features::personas::{Persona, PersonaManager};
use crate::features::plugins::PluginManager;
use crate::features::rate_limiting::RateLimiter;
use anyhow::Result;
use log::{debug, error, info, warn};
use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};
use serenity::builder::CreateEmbed;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::channel::Message;
use serenity::prelude::Context;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{timeout, Duration as TokioDuration, Instant};
use uuid::Uuid;

#[derive(Clone)]
pub struct CommandHandler {
    persona_manager: PersonaManager,
    database: Database,
    rate_limiter: RateLimiter,
    audio_transcriber: AudioTranscriber,
    openai_model: String,
    conflict_detector: ConflictDetector,
    conflict_mediator: ConflictMediator,
    conflict_enabled: bool,
    conflict_sensitivity_threshold: f32,
    usage_tracker: UsageTracker,
    interaction_tracker: InteractionTracker,
    plugin_manager: Option<Arc<PluginManager>>,
    command_registry: CommandRegistry,
    command_context: Arc<CommandContext>,
}

impl CommandHandler {
    pub fn new(
        database: Database,
        openai_api_key: String,
        openai_model: String,
        conflict_enabled: bool,
        conflict_sensitivity: &str,
        mediation_cooldown_minutes: u64,
        usage_tracker: UsageTracker,
        interaction_tracker: InteractionTracker,
    ) -> Self {
        // Map sensitivity to threshold
        let sensitivity_threshold = match conflict_sensitivity.to_lowercase().as_str() {
            "low" => 0.7,   // Only very high confidence conflicts
            "high" => 0.35, // More sensitive - catches single keywords + context
            "ultra" => 0.3, // Maximum sensitivity - triggers on single hostile keyword
            _ => 0.5,       // Medium (default)
        };

        let persona_manager = PersonaManager::new();
        let image_generator = ImageGenerator::new(openai_api_key.clone());
        let start_time = std::time::Instant::now();

        // Build shared context for modular handlers
        let command_context = Arc::new(CommandContext::with_start_time(
            persona_manager.clone(),
            database.clone(),
            usage_tracker.clone(),
            interaction_tracker.clone(),
            image_generator.clone(),
            openai_model.clone(),
            start_time,
        ));

        // Build registry from all handler modules
        let mut command_registry = CommandRegistry::new();
        for handler in create_all_handlers() {
            command_registry.register(handler);
        }

        CommandHandler {
            persona_manager,
            database,
            rate_limiter: RateLimiter::new(10, Duration::from_secs(60)),
            audio_transcriber: AudioTranscriber::new(openai_api_key),
            openai_model,
            conflict_detector: ConflictDetector::new(),
            conflict_mediator: ConflictMediator::new(999, mediation_cooldown_minutes), // High limit for testing
            conflict_enabled,
            conflict_sensitivity_threshold: sensitivity_threshold,
            usage_tracker,
            interaction_tracker,
            plugin_manager: None,
            command_registry,
            command_context,
        }
    }

    /// Set the plugin manager for handling plugin commands
    pub fn set_plugin_manager(&mut self, plugin_manager: Arc<PluginManager>) {
        self.plugin_manager = Some(plugin_manager);
    }

    /// Get a reference to the loaded plugins (if any)
    pub fn get_plugins(&self) -> Vec<crate::features::plugins::Plugin> {
        self.plugin_manager
            .as_ref()
            .map(|pm| pm.config.plugins.clone())
            .unwrap_or_default()
    }

    /// Get the usage tracker for external use
    pub fn get_usage_tracker(&self) -> UsageTracker {
        self.usage_tracker.clone()
    }

    /// Build an embed for a persona response
    /// Used for both DM and guild responses when response_embeds is enabled
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
    pub async fn handle_message(&self, ctx: &Context, msg: &Message) -> Result<()> {
        let request_id = Uuid::new_v4();
        let user_id = msg.author.id.to_string();
        let channel_id = msg.channel_id.to_string();
        let guild_id = msg
            .guild_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "DM".to_string());
        let guild_id_opt = if guild_id != "DM" {
            Some(guild_id.as_str())
        } else {
            None
        };

        info!(
            "[{}] 📥 Message received | User: {} | Channel: {} | Guild: {} | Content: '{}'",
            request_id,
            user_id,
            channel_id,
            guild_id,
            msg.content.chars().take(100).collect::<String>()
        );

        debug!("[{request_id}] 🔍 Checking rate limit for user: {user_id}");
        if !self.rate_limiter.wait_for_rate_limit(&user_id).await {
            warn!("[{request_id}] 🚫 Rate limit exceeded for user: {user_id}");
            debug!("[{request_id}] 📤 Sending rate limit message to Discord");
            msg.channel_id
                .say(
                    &ctx.http,
                    "You're sending messages too quickly! Please slow down.",
                )
                .await?;
            info!("[{request_id}] ✅ Rate limit message sent successfully");
            return Ok(());
        }
        debug!("[{request_id}] ✅ Rate limit check passed");

        // Get audio transcription mode for this guild
        let is_dm = msg.guild_id.is_none();
        let audio_mode = if let Some(gid) = guild_id_opt {
            let feature_enabled = self
                .database
                .is_feature_enabled("audio_transcription", None, Some(gid))
                .await?;
            if !feature_enabled {
                "disabled".to_string()
            } else {
                self.database
                    .get_guild_setting(gid, "audio_transcription_mode")
                    .await?
                    .unwrap_or_else(|| "mention_only".to_string())
            }
        } else {
            "always".to_string() // DMs always auto-transcribe
        };

        // Check if we should process audio based on mode
        let should_transcribe = match audio_mode.as_str() {
            "always" => true,
            "mention_only" => is_dm || self.is_bot_mentioned(ctx, msg).await?,
            _ => false, // "disabled" or unknown
        };

        let audio_handled = if !msg.attachments.is_empty() && should_transcribe {
            debug!(
                "[{}] 🎵 Processing {} audio attachments (mode: {})",
                request_id,
                msg.attachments.len(),
                audio_mode
            );
            self.handle_audio_attachments(ctx, msg, guild_id_opt)
                .await?
        } else {
            false
        };

        let content = msg.content.trim();
        debug!(
            "[{}] 🔍 Analyzing message content | Length: {} | Is DM: {} | Starts with command: {}",
            request_id,
            content.len(),
            is_dm,
            content.starts_with('/')
        );

        // Store guild messages FIRST (needed for conflict detection to have data)
        if !is_dm && !content.is_empty() && !content.starts_with('/') {
            debug!("[{request_id}] 💾 Storing guild message for analysis");
            self.database
                .store_message(&user_id, &channel_id, "user", content, None)
                .await?;
        }

        // Conflict detection - check both env var AND feature flag
        let guild_conflict_enabled = if let Some(gid) = guild_id_opt {
            self.database
                .is_feature_enabled("conflict_mediation", None, Some(gid))
                .await?
        } else {
            false // No conflict detection in DMs
        };

        if !is_dm
            && self.conflict_enabled
            && guild_conflict_enabled
            && !content.is_empty()
            && !content.starts_with('/')
        {
            debug!("[{request_id}] 🔍 Running conflict detection analysis");
            if let Err(e) = self
                .check_and_mediate_conflicts(ctx, msg, &channel_id, guild_id_opt)
                .await
            {
                warn!("[{request_id}] ⚠️ Conflict detection error: {e}");
                // Don't fail the whole message processing if conflict detection fails
            }
        }

        if content.starts_with('/') {
            info!(
                "[{}] 🎯 Processing text command: {}",
                request_id,
                content.split_whitespace().next().unwrap_or("")
            );
            self.handle_text_command_with_id(ctx, msg, request_id)
                .await?;
        } else if is_dm && !content.is_empty() && !audio_handled {
            info!("[{request_id}] 💬 Processing DM message (auto-response mode)");
            self.handle_dm_message_with_id(ctx, msg, request_id).await?;
        } else if !is_dm
            && !audio_handled
            && !content.is_empty()
            && self.is_in_active_debate_thread(msg.channel_id).await
        {
            // Check if debate_auto_response is enabled for this guild
            let auto_response_enabled = if let Some(gid) = guild_id_opt {
                self.database
                    .get_guild_setting(gid, "debate_auto_response")
                    .await?
                    .map(|v| v == "enabled")
                    .unwrap_or(false) // Default disabled
            } else {
                false
            };

            if auto_response_enabled {
                info!("[{request_id}] 🎭 Auto-responding in active debate thread");
                self.handle_mention_message_with_id(ctx, msg, request_id)
                    .await?;
            } else {
                debug!("[{request_id}] ℹ️ Message in debate thread but auto-response disabled");
            }
        } else if !is_dm
            && !audio_handled
            && !content.is_empty()
            && self.is_in_active_council_thread(msg.channel_id).await
        {
            // Council threads respond to mentions by default
            if self.is_bot_mentioned(ctx, msg).await? {
                info!("[{request_id}] 🏛️ Responding to mention in active council thread");
                self.handle_council_followup(ctx, msg, request_id).await?;
            } else {
                debug!("[{request_id}] ℹ️ Message in council thread but bot not mentioned");
            }
        } else if !is_dm
            && !audio_handled
            && self.is_bot_mentioned(ctx, msg).await?
            && !content.is_empty()
        {
            // Check mention_responses guild setting
            let mention_enabled = if let Some(gid) = guild_id_opt {
                self.database
                    .get_guild_setting(gid, "mention_responses")
                    .await?
                    .map(|v| v == "enabled")
                    .unwrap_or(true) // Default enabled
            } else {
                true
            };

            if mention_enabled {
                info!("[{request_id}] 🏷️ Bot mentioned in channel - responding");
                self.handle_mention_message_with_id(ctx, msg, request_id)
                    .await?;
            } else {
                debug!("[{request_id}] ℹ️ Bot mentioned but mention_responses disabled for guild");
            }
        } else if !is_dm && !content.is_empty() {
            debug!("[{request_id}] ℹ️ Guild message stored (no bot response needed)");
        } else {
            debug!("[{request_id}] ℹ️ Message ignored (empty or DM)");
        }

        info!("[{request_id}] ✅ Message processing completed");
        Ok(())
    }

    async fn is_bot_mentioned(&self, ctx: &Context, msg: &Message) -> Result<bool> {
        let current_user = ctx.http.get_current_user().await?;
        Ok(msg.mentions.iter().any(|user| user.id == current_user.id))
    }

    /// Check if the channel has an active debate (for auto-response feature)
    async fn is_in_active_debate_thread(&self, channel_id: serenity::model::id::ChannelId) -> bool {
        use crate::features::debate::get_active_debates;
        get_active_debates().contains_key(&channel_id.0)
    }

    /// Check if the channel has an active council (for follow-up questions)
    async fn is_in_active_council_thread(
        &self,
        channel_id: serenity::model::id::ChannelId,
    ) -> bool {
        get_active_councils().contains_key(&channel_id.0)
    }

    async fn is_in_thread(&self, ctx: &Context, msg: &Message) -> Result<bool> {
        use serenity::model::channel::{Channel, ChannelType};

        // Fetch the channel to check its type
        match ctx.http.get_channel(msg.channel_id.0).await {
            Ok(Channel::Guild(guild_channel)) => Ok(matches!(
                guild_channel.kind,
                ChannelType::PublicThread | ChannelType::PrivateThread
            )),
            _ => Ok(false),
        }
    }

    async fn fetch_thread_messages(
        &self,
        ctx: &Context,
        msg: &Message,
        limit: u8,
        request_id: Uuid,
    ) -> Result<Vec<(String, String)>> {
        use serenity::builder::GetMessages;

        debug!("[{request_id}] 🧵 Fetching up to {limit} messages from thread");

        // Fetch messages from the thread (Discord API limit is 100)
        let messages = msg
            .channel_id
            .messages(&ctx.http, |builder: &mut GetMessages| {
                builder.limit(limit as u64)
            })
            .await?;

        debug!(
            "[{}] 🧵 Retrieved {} messages from thread",
            request_id,
            messages.len()
        );

        // Get bot's user ID to identify bot messages
        let current_user = ctx.http.get_current_user().await?;
        let bot_id = current_user.id;

        // Convert messages to (role, content) format
        // Messages are returned newest first, so reverse for chronological order
        let conversation: Vec<(String, String)> = messages
            .iter()
            .rev() // Reverse to get oldest first (chronological order)
            .filter(|m| !m.content.is_empty()) // Skip empty messages
            .map(|m| {
                let role = if m.author.id == bot_id {
                    "assistant".to_string()
                } else {
                    "user".to_string()
                };
                let content = m.content.clone();
                (role, content)
            })
            .collect();

        debug!(
            "[{}] 🧵 Processed {} non-empty messages from thread",
            request_id,
            conversation.len()
        );

        Ok(conversation)
    }

    async fn handle_dm_message_with_id(
        &self,
        ctx: &Context,
        msg: &Message,
        request_id: Uuid,
    ) -> Result<()> {
        let start_time = Instant::now();
        let user_id = msg.author.id.to_string();
        let channel_id = msg.channel_id.to_string();
        let user_message = msg.content.trim();

        debug!(
            "[{}] 💬 Processing DM auto-response | User: {} | Message: '{}'",
            request_id,
            user_id,
            user_message.chars().take(100).collect::<String>()
        );

        // Get or create DM session
        let session_id = self
            .interaction_tracker
            .get_or_create_session(&user_id, &channel_id);
        debug!("[{request_id}] 📊 DM session: {session_id}");

        // Track message received
        self.interaction_tracker.track_message_received(
            &session_id,
            &user_id,
            &channel_id,
            &msg.id.to_string(),
            user_message.len(),
            !msg.attachments.is_empty(),
        );

        // Read any text attachments from the message
        let text_attachments = self.get_text_attachments_context(msg, request_id).await;
        let attachment_context = self.format_attachments_for_context(&text_attachments);

        // Enhance user message with attachment content if present
        let enhanced_message = if attachment_context.is_empty() {
            user_message.to_string()
        } else {
            info!(
                "[{request_id}] 📎 Including {} text attachment(s) in context",
                text_attachments.len()
            );
            format!("{attachment_context}{user_message}")
        };

        // Get user's persona
        debug!("[{request_id}] 🎭 Fetching user persona from database");
        let user_persona = self.database.get_user_persona(&user_id).await?;
        debug!("[{request_id}] 🎭 User persona: {user_persona}");

        // Store user message in conversation history (store original message, not enhanced)
        debug!("[{request_id}] 💾 Storing user message to conversation history");
        self.database
            .store_message(
                &user_id,
                &channel_id,
                "user",
                user_message,
                Some(&user_persona),
            )
            .await?;
        debug!("[{request_id}] ✅ User message stored successfully");

        // Retrieve conversation history (last 40 messages = ~20 exchanges)
        debug!("[{request_id}] 📚 Retrieving conversation history");
        let conversation_history = self
            .database
            .get_conversation_history(&user_id, &channel_id, 40)
            .await?;
        info!(
            "[{}] 📚 Retrieved {} historical messages",
            request_id,
            conversation_history.len()
        );

        // Show typing indicator while processing
        debug!("[{request_id}] ⌨️ Starting typing indicator");
        let typing = msg.channel_id.start_typing(&ctx.http)?;

        // Build system prompt without modifier (conversational mode)
        debug!("[{request_id}] 📝 Building system prompt | Persona: {user_persona}");
        let system_prompt = self.persona_manager.get_system_prompt(&user_persona, None);
        debug!(
            "[{}] ✅ System prompt generated | Length: {} chars",
            request_id,
            system_prompt.len()
        );

        // Log usage
        debug!("[{request_id}] 📊 Logging usage to database");
        self.database
            .log_usage(&user_id, "dm_chat", Some(&user_persona))
            .await?;
        debug!("[{request_id}] ✅ Usage logged successfully");

        // Get AI response with conversation history (use enhanced message with attachments)
        info!("[{request_id}] 🚀 Calling OpenAI API for DM response");
        let api_call_result = self
            .get_ai_response_with_context(
                &system_prompt,
                &enhanced_message,
                conversation_history,
                request_id,
                Some(&user_id),
                None,
                Some(&channel_id),
            )
            .await;

        // Track API call (estimate cost from usage tracker's pricing)
        // This will be more accurate if we can access the actual usage data, but for now we'll track it after response

        match api_call_result {
            Ok(ai_response) => {
                info!(
                    "[{}] ✅ OpenAI response received | Response length: {}",
                    request_id,
                    ai_response.len()
                );

                // Stop typing
                typing.stop();
                debug!("[{request_id}] ⌨️ Stopped typing indicator");

                // Get persona for embed styling
                let persona = self.persona_manager.get_persona(&user_persona);

                // Send response as embed (handle long messages - embed description limit is 4096)
                let chunks = chunk_for_embed(&ai_response);
                if chunks.len() > 1 {
                    debug!("[{request_id}] 📄 Response too long for single embed, splitting into {} chunks", chunks.len());

                    for (i, chunk) in chunks.iter().enumerate() {
                        if !chunk.trim().is_empty() {
                            debug!(
                                "[{}] 📤 Sending embed chunk {} of {} ({} chars)",
                                request_id,
                                i + 1,
                                chunks.len(),
                                chunk.len()
                            );

                            if let Some(p) = persona {
                                // First chunk gets full embed with author, rest are continuation
                                let embed = if i == 0 {
                                    Self::build_persona_embed(p, chunk)
                                } else {
                                    Self::build_continuation_embed(p, chunk)
                                };
                                msg.channel_id
                                    .send_message(&ctx.http, |m| m.set_embed(embed))
                                    .await?;
                            } else {
                                // Fallback to plain text if persona not found
                                msg.channel_id.say(&ctx.http, chunk).await?;
                            }
                            debug!(
                                "[{}] ✅ Embed chunk {} sent successfully",
                                request_id,
                                i + 1
                            );
                        }
                    }
                    info!("[{request_id}] ✅ All DM embed response chunks sent successfully");
                } else {
                    debug!(
                        "[{}] 📤 Sending DM embed response ({} chars)",
                        request_id,
                        ai_response.len()
                    );
                    if let Some(p) = persona {
                        let embed = Self::build_persona_embed(p, &ai_response);
                        msg.channel_id
                            .send_message(&ctx.http, |m| m.set_embed(embed))
                            .await?;
                    } else {
                        // Fallback to plain text if persona not found
                        msg.channel_id.say(&ctx.http, &ai_response).await?;
                    }
                    info!("[{request_id}] ✅ DM embed response sent successfully");
                }

                // Store assistant response in conversation history
                debug!("[{request_id}] 💾 Storing assistant response to conversation history");
                self.database
                    .store_message(
                        &user_id,
                        &channel_id,
                        "assistant",
                        &ai_response,
                        Some(&user_persona),
                    )
                    .await?;
                debug!("[{request_id}] ✅ Assistant response stored successfully");

                // Track message sent with response time
                let response_time_ms = start_time.elapsed().as_millis() as u64;
                self.interaction_tracker.track_message_sent(
                    &session_id,
                    &user_id,
                    &channel_id,
                    &request_id.to_string(),
                    ai_response.len(),
                    response_time_ms,
                );
                debug!(
                    "[{request_id}] 📊 Tracked message sent (response time: {response_time_ms}ms)"
                );
            }
            Err(e) => {
                typing.stop();
                debug!("[{request_id}] ⌨️ Stopped typing indicator");
                error!("[{request_id}] ❌ AI response error in DM: {e}");

                let error_message = if e.to_string().contains("timed out") {
                    "⏱️ Sorry, I'm taking too long to think. Please try again with a shorter message."
                } else {
                    "❌ Sorry, I encountered an error. Please try again later."
                };

                debug!("[{request_id}] 📤 Sending error message to user");
                msg.channel_id.say(&ctx.http, error_message).await?;
                warn!("[{request_id}] ⚠️ Error message sent to user after AI failure");
            }
        }

        info!("[{request_id}] ✅ DM message processing completed");
        Ok(())
    }

    async fn handle_mention_message_with_id(
        &self,
        ctx: &Context,
        msg: &Message,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = msg.author.id.to_string();
        let channel_id = msg.channel_id.to_string();
        let guild_id = msg.guild_id.map(|id| id.to_string());
        let guild_id_opt = guild_id.as_deref();
        let user_message = msg.content.trim();

        debug!(
            "[{}] 🏷️ Processing mention in channel | User: {} | Message: '{}'",
            request_id,
            user_id,
            user_message.chars().take(100).collect::<String>()
        );

        // Get user's persona with channel override -> user -> guild default cascade
        debug!("[{request_id}] 🎭 Fetching user persona from database");
        let user_persona = if let Some(gid) = guild_id_opt {
            self.database
                .get_persona_with_channel(&user_id, gid, &channel_id)
                .await?
        } else {
            self.database
                .get_user_persona_with_guild(&user_id, None)
                .await?
        };
        debug!("[{request_id}] 🎭 User persona: {user_persona}");

        // Get max_context_messages from guild settings
        let max_context = if let Some(gid) = guild_id_opt {
            self.database
                .get_guild_setting(gid, "max_context_messages")
                .await?
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(40)
        } else {
            40
        };

        // Check if message is in a thread
        let is_thread = self.is_in_thread(ctx, msg).await?;
        debug!("[{request_id}] 🧵 Is thread: {is_thread} | Max context: {max_context}");

        // Read text attachments from current message
        let mut all_attachments = self.get_text_attachments_context(msg, request_id).await;

        // If in a thread and user seems to be asking about files, fetch thread attachments
        if is_thread && self.seems_like_file_question(user_message) && all_attachments.is_empty() {
            info!(
                "[{request_id}] 📎 User asking about files in thread, fetching thread attachments"
            );
            let thread_attachments = self
                .fetch_thread_attachments(ctx, msg.channel_id, 20, request_id)
                .await?;
            all_attachments.extend(thread_attachments);
        }

        // Format attachment context
        let attachment_context = self.format_attachments_for_context(&all_attachments);

        // Enhance user message with attachment content if present
        let enhanced_message = if attachment_context.is_empty() {
            user_message.to_string()
        } else {
            info!(
                "[{request_id}] 📎 Including {} text attachment(s) in context",
                all_attachments.len()
            );
            format!("{attachment_context}{user_message}")
        };

        // Retrieve conversation history based on context type
        let conversation_history = if is_thread {
            // Thread context: Fetch messages from Discord
            info!("[{request_id}] 🧵 Fetching thread context from Discord");
            self.fetch_thread_messages(ctx, msg, max_context as u8, request_id)
                .await?
        } else {
            // Channel context: Use database history
            info!("[{request_id}] 📚 Fetching channel context from database");

            // Store user message in conversation history for channels (store original, not enhanced)
            debug!("[{request_id}] 💾 Storing user message to conversation history");
            self.database
                .store_message(
                    &user_id,
                    &channel_id,
                    "user",
                    user_message,
                    Some(&user_persona),
                )
                .await?;
            debug!("[{request_id}] ✅ User message stored successfully");

            self.database
                .get_conversation_history(&user_id, &channel_id, max_context)
                .await?
        };

        info!(
            "[{}] 📚 Retrieved {} historical messages for context",
            request_id,
            conversation_history.len()
        );

        // Show typing indicator while processing
        debug!("[{request_id}] ⌨️ Starting typing indicator");
        let typing = msg.channel_id.start_typing(&ctx.http)?;

        // Get channel verbosity for guild channels
        let verbosity = if let Some(guild_id) = msg.guild_id {
            self.database
                .get_channel_verbosity(&guild_id.to_string(), &channel_id)
                .await?
        } else {
            "concise".to_string()
        };

        // Build system prompt without modifier (conversational mode), with verbosity
        debug!("[{request_id}] 📝 Building system prompt | Persona: {user_persona} | Verbosity: {verbosity}");
        let system_prompt =
            self.persona_manager
                .get_system_prompt_with_verbosity(&user_persona, None, &verbosity);
        debug!(
            "[{}] ✅ System prompt generated | Length: {} chars",
            request_id,
            system_prompt.len()
        );

        // Log usage
        debug!("[{request_id}] 📊 Logging usage to database");
        self.database
            .log_usage(&user_id, "mention_chat", Some(&user_persona))
            .await?;
        debug!("[{request_id}] ✅ Usage logged successfully");

        // Get AI response with conversation history (use enhanced message with attachments)
        info!("[{request_id}] 🚀 Calling OpenAI API for mention response");
        match self
            .get_ai_response_with_context(
                &system_prompt,
                &enhanced_message,
                conversation_history,
                request_id,
                Some(&user_id),
                guild_id_opt,
                Some(&channel_id),
            )
            .await
        {
            Ok(ai_response) => {
                info!(
                    "[{}] ✅ OpenAI response received | Response length: {}",
                    request_id,
                    ai_response.len()
                );

                // Stop typing
                typing.stop();
                debug!("[{request_id}] ⌨️ Stopped typing indicator");

                // Check if embeds are enabled for this guild
                let use_embeds = if let Some(gid) = guild_id_opt {
                    self.database
                        .get_guild_setting(gid, "response_embeds")
                        .await
                        .unwrap_or(None)
                        .map(|v| v != "disabled")
                        .unwrap_or(true) // Default to enabled
                } else {
                    true // DMs always use embeds
                };

                // Get persona for embed styling
                let persona = self.persona_manager.get_persona(&user_persona);

                // Send response as embed or plain text depending on setting
                if use_embeds && persona.is_some() {
                    let p = persona.unwrap();

                    // Embed description limit is 4096
                    let chunks = chunk_for_embed(&ai_response);
                    if chunks.len() > 1 {
                        debug!("[{request_id}] 📄 Response too long for single embed, splitting into {} chunks", chunks.len());

                        for (i, chunk) in chunks.iter().enumerate() {
                            if !chunk.trim().is_empty() {
                                debug!(
                                    "[{}] 📤 Sending embed chunk {} of {} ({} chars)",
                                    request_id,
                                    i + 1,
                                    chunks.len(),
                                    chunk.len()
                                );

                                // First chunk gets full embed with author, rest are continuation
                                let embed = if i == 0 {
                                    Self::build_persona_embed(p, chunk)
                                } else {
                                    Self::build_continuation_embed(p, chunk)
                                };
                                msg.channel_id
                                    .send_message(&ctx.http, |m| m.set_embed(embed))
                                    .await?;
                                debug!(
                                    "[{}] ✅ Embed chunk {} sent successfully",
                                    request_id,
                                    i + 1
                                );
                            }
                        }
                        info!(
                            "[{request_id}] ✅ All mention embed response chunks sent successfully"
                        );
                    } else {
                        debug!(
                            "[{}] 📤 Sending mention embed response ({} chars)",
                            request_id,
                            ai_response.len()
                        );
                        let embed = Self::build_persona_embed(p, &ai_response);
                        msg.channel_id
                            .send_message(&ctx.http, |m| m.set_embed(embed))
                            .await?;
                        info!("[{request_id}] ✅ Mention embed response sent successfully");
                    }
                } else {
                    // Plain text fallback (legacy behavior or embeds disabled)
                    let chunks = chunk_for_message(&ai_response);
                    if chunks.len() > 1 {
                        debug!("[{request_id}] 📄 Response too long, splitting into {} chunks", chunks.len());

                        // First chunk as threaded reply
                        if let Some(first_chunk) = chunks.first() {
                            if !first_chunk.trim().is_empty() {
                                debug!(
                                    "[{}] 📤 Sending first chunk as reply ({} chars)",
                                    request_id,
                                    first_chunk.len()
                                );
                                msg.reply(&ctx.http, first_chunk).await?;
                                debug!("[{request_id}] ✅ First chunk sent as reply");
                            }
                        }

                        // Remaining chunks as regular messages in the thread
                        for (i, chunk) in chunks.iter().skip(1).enumerate() {
                            if !chunk.trim().is_empty() {
                                debug!(
                                    "[{}] 📤 Sending chunk {} of {} ({} chars)",
                                    request_id,
                                    i + 2,
                                    chunks.len(),
                                    chunk.len()
                                );
                                msg.channel_id.say(&ctx.http, chunk).await?;
                                debug!("[{}] ✅ Chunk {} sent successfully", request_id, i + 2);
                            }
                        }
                        info!("[{request_id}] ✅ All mention response chunks sent successfully");
                    } else {
                        debug!(
                            "[{}] 📤 Sending mention response as reply ({} chars)",
                            request_id,
                            ai_response.len()
                        );
                        msg.reply(&ctx.http, &ai_response).await?;
                        info!("[{request_id}] ✅ Mention response sent successfully");
                    }
                }

                // Store assistant response in conversation history (only for channels, not threads)
                if !is_thread {
                    debug!("[{request_id}] 💾 Storing assistant response to conversation history");
                    self.database
                        .store_message(
                            &user_id,
                            &channel_id,
                            "assistant",
                            &ai_response,
                            Some(&user_persona),
                        )
                        .await?;
                    debug!("[{request_id}] ✅ Assistant response stored successfully");
                } else {
                    debug!("[{request_id}] 🧵 Skipping database storage for thread (will fetch from Discord next time)");
                }
            }
            Err(e) => {
                typing.stop();
                debug!("[{request_id}] ⌨️ Stopped typing indicator");
                error!("[{request_id}] ❌ AI response error in mention: {e}");

                let error_message = if e.to_string().contains("timed out") {
                    "⏱️ Sorry, I'm taking too long to think. Please try again with a shorter message."
                } else {
                    "❌ Sorry, I encountered an error. Please try again later."
                };

                debug!("[{request_id}] 📤 Sending error message to user as reply");
                msg.reply(&ctx.http, error_message).await?;
                warn!("[{request_id}] ⚠️ Error message sent to user after AI failure");
            }
        }

        info!("[{request_id}] ✅ Mention message processing completed");
        Ok(())
    }

    pub async fn handle_slash_command(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();
        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();
        let guild_id = command
            .guild_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "DM".to_string());

        info!(
            "[{}] 📥 Slash command received | Command: {} | User: {} | Channel: {} | Guild: {}",
            request_id, command.data.name, user_id, channel_id, guild_id
        );

        debug!("[{request_id}] 🔍 Checking rate limit for user: {user_id}");
        if !self.rate_limiter.wait_for_rate_limit(&user_id).await {
            warn!("[{request_id}] 🚫 Rate limit exceeded for user: {user_id} in slash command");
            debug!("[{request_id}] 📤 Sending rate limit response to Discord");
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content("You're sending commands too quickly! Please slow down.")
                        })
                })
                .await?;
            info!("[{request_id}] ✅ Rate limit response sent successfully");
            return Ok(());
        }
        debug!("[{request_id}] ✅ Rate limit check passed");

        info!(
            "[{}] 🎯 Processing slash command: {} from user: {}",
            request_id, command.data.name, user_id
        );

        let cmd_name = command.data.name.as_str();

        // Look up handler in the registry first
        if let Some(handler) = self.command_registry.get(cmd_name) {
            debug!("[{request_id}] Dispatching to registered handler: {cmd_name}");
            handler
                .handle(Arc::clone(&self.command_context), ctx, command)
                .await?;
        } else if let Some(ref pm) = self.plugin_manager {
            // Fall through to plugin handling
            if let Some(plugin) = pm
                .config
                .plugins
                .iter()
                .find(|p| p.enabled && p.command.name == cmd_name)
            {
                debug!("[{request_id}] Handling plugin command: {cmd_name}");
                self.handle_plugin_command(ctx, command, plugin.clone(), pm.clone(), request_id)
                    .await?;
            } else {
                warn!("[{request_id}] Unknown slash command: {cmd_name}");
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("Unknown command. Use `/help` to see available commands.")
                            })
                    })
                    .await?;
            }
        } else {
            warn!("[{request_id}] Unknown slash command: {cmd_name}");
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content("Unknown command. Use `/help` to see available commands.")
                        })
                })
                .await?;
        }

        info!("[{request_id}] ✅ Slash command processing completed");
        Ok(())
    }

    /// Handle a plugin-based slash command
    async fn handle_plugin_command(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        plugin: crate::features::plugins::Plugin,
        plugin_manager: Arc<PluginManager>,
        request_id: Uuid,
    ) -> Result<()> {
        use serenity::model::application::interaction::InteractionResponseType;

        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        info!(
            "[{}] 🔌 Processing plugin command: {} | User: {} | Plugin: {}",
            request_id, plugin.command.name, user_id, plugin.name
        );

        // Check guild_only restriction
        if plugin.security.guild_only && guild_id.is_none() {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .content("This command can only be used in a server, not in DMs.")
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        // Check cooldown
        if plugin.security.cooldown_seconds > 0
            && !plugin_manager.job_manager.check_cooldown(
                &user_id,
                &plugin.name,
                plugin.security.cooldown_seconds,
            ) {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content(format!(
                                    "Please wait before using `{}` again. Cooldown: {} seconds.",
                                    plugin.command.name, plugin.security.cooldown_seconds
                                ))
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }

        // Extract command parameters
        let mut params: HashMap<String, String> = HashMap::new();
        for opt in &command.data.options {
            if let Some(value) = &opt.value {
                let value_str = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string().trim_matches('"').to_string(),
                };
                params.insert(opt.name.clone(), value_str);
            }
        }

        // Add defaults for missing optional parameters
        for opt_def in &plugin.command.options {
            if !params.contains_key(&opt_def.name) {
                if let Some(ref default) = opt_def.default {
                    params.insert(opt_def.name.clone(), default.clone());
                }
            }
        }

        // Validate parameters
        for opt_def in &plugin.command.options {
            if let Some(value) = params.get(&opt_def.name) {
                if let Some(ref validation) = opt_def.validation {
                    // Check pattern
                    if let Some(ref pattern) = validation.pattern {
                        let re = regex::Regex::new(pattern)?;
                        if !re.is_match(value) {
                            command
                                .create_interaction_response(&ctx.http, |response| {
                                    response
                                        .kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content(format!(
                                                "Invalid value for `{}`: doesn't match expected format.",
                                                opt_def.name
                                            ))
                                            .ephemeral(true)
                                        })
                                })
                                .await?;
                            return Ok(());
                        }
                    }
                    // Check length constraints
                    if let Some(min_len) = validation.min_length {
                        if value.len() < min_len {
                            command
                                .create_interaction_response(&ctx.http, |response| {
                                    response
                                        .kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content(format!(
                                                "Value for `{}` is too short (minimum {} characters).",
                                                opt_def.name, min_len
                                            ))
                                            .ephemeral(true)
                                        })
                                })
                                .await?;
                            return Ok(());
                        }
                    }
                    if let Some(max_len) = validation.max_length {
                        if value.len() > max_len {
                            command
                                .create_interaction_response(&ctx.http, |response| {
                                    response
                                        .kind(InteractionResponseType::ChannelMessageWithSource)
                                        .interaction_response_data(|message| {
                                            message.content(format!(
                                                "Value for `{}` is too long (maximum {} characters).",
                                                opt_def.name, max_len
                                            ))
                                            .ephemeral(true)
                                        })
                                })
                                .await?;
                            return Ok(());
                        }
                    }
                }
            } else if opt_def.required {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content(format!(
                                        "Missing required parameter: `{}`.",
                                        opt_def.name
                                    ))
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
        }

        // Check if plugins feature is enabled for this guild
        if let Some(ref gid) = guild_id {
            let enabled = self
                .database
                .is_feature_enabled("plugins", None, Some(gid))
                .await?;
            if !enabled {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content("Plugin commands are disabled in this server.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
        }

        // Handle virtual plugins (no CLI execution, handled internally)
        if plugin.is_virtual() {
            info!(
                "[{}] 🔧 Handling virtual plugin: {}",
                request_id, plugin.command.name
            );
            return self
                .handle_virtual_plugin(
                    ctx,
                    command,
                    &plugin,
                    &plugin_manager,
                    &params,
                    &user_id,
                    request_id,
                )
                .await;
        }

        // Defer the response (command will take a while)
        // Use ephemeral response so only the thread appears in the channel
        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::DeferredChannelMessageWithSource)
                    .interaction_response_data(|data| data.ephemeral(true))
            })
            .await?;

        info!(
            "[{}] ⏳ Deferred response for plugin command: {}",
            request_id, plugin.command.name
        );

        // Clone values needed for the background task
        let http = ctx.http.clone();
        let plugin_manager = plugin_manager.clone();
        let plugin = plugin.clone();
        let discord_channel_id = command.channel_id;
        let interaction_token = command.token.clone();
        let application_id = command.application_id.0;
        let user_id_owned = user_id.clone();
        let guild_id_owned = guild_id.clone();

        // Check if we're already in a thread
        let is_thread = match discord_channel_id.to_channel(&ctx.http).await {
            Ok(channel) => {
                use serenity::model::channel::Channel;
                matches!(channel, Channel::Guild(gc) if gc.kind.name() == "public_thread" || gc.kind.name() == "private_thread")
            }
            Err(_) => false,
        };

        // Spawn background task to execute the plugin
        // Pass interaction info so the thread can be created from the interaction response
        let interaction_info = Some((application_id, interaction_token.clone()));

        tokio::spawn(async move {
            // Check if this should use chunked transcription
            let use_chunking = plugin_manager.should_use_chunking(&plugin);
            let is_youtube = params
                .get("url")
                .map(|u| u.contains("youtube.com") || u.contains("youtu.be"))
                .unwrap_or(false);
            let is_playlist = params
                .get("url")
                .map(|u| u.contains("playlist?list=") || u.contains("&list="))
                .unwrap_or(false);

            let result = if use_chunking && is_youtube && !is_playlist {
                // Use chunked transcription for YouTube videos (not playlists)
                let url = params.get("url").cloned().unwrap_or_default();
                let video_title = crate::features::plugins::fetch_youtube_title(&url)
                    .await
                    .unwrap_or_else(|| "Video".to_string());

                info!(
                    "[{request_id}] 📦 Using chunked transcription for: {video_title}"
                );

                plugin_manager
                    .execute_chunked_transcription(
                        http,
                        plugin.clone(),
                        url,
                        video_title,
                        params,
                        user_id_owned,
                        guild_id_owned,
                        discord_channel_id,
                        interaction_info,
                        is_thread,
                    )
                    .await
            } else {
                // Use regular execution
                plugin_manager
                    .execute_plugin(
                        http,
                        plugin.clone(),
                        params,
                        user_id_owned,
                        guild_id_owned,
                        discord_channel_id,
                        interaction_info,
                        is_thread,
                    )
                    .await
            };

            match result {
                Ok(job_id) => {
                    info!(
                        "[{}] ✅ Plugin job started: {} (job_id: {})",
                        request_id, plugin.name, job_id
                    );
                    // Note: execute_plugin/execute_chunked_transcription handles editing the interaction response
                }
                Err(e) => {
                    error!(
                        "[{}] ❌ Plugin execution failed: {} - {}",
                        request_id, plugin.name, e
                    );

                    // Edit the deferred response with error
                    let edit_url = format!(
                        "https://discord.com/api/v10/webhooks/{application_id}/{interaction_token}/messages/@original"
                    );

                    let client = reqwest::Client::new();
                    let _ = client
                        .patch(&edit_url)
                        .header("Content-Type", "application/json")
                        .json(&serde_json::json!({
                            "content": format!("❌ Command failed: {}", e)
                        }))
                        .send()
                        .await;
                }
            }
        });

        Ok(())
    }

    /// Handle virtual plugins (commands handled internally without CLI execution)
    async fn handle_virtual_plugin(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        plugin: &crate::features::plugins::Plugin,
        plugin_manager: &Arc<PluginManager>,
        params: &HashMap<String, String>,
        user_id: &str,
        request_id: Uuid,
    ) -> Result<()> {
        use serenity::model::application::interaction::InteractionResponseType;

        match plugin.command.name.as_str() {
            "transcribe_cancel" => {
                self.handle_transcribe_cancel(
                    ctx,
                    command,
                    plugin_manager,
                    params,
                    user_id,
                    request_id,
                )
                .await
            }
            "transcribe_status" => {
                self.handle_transcribe_status(ctx, command, plugin_manager, user_id, request_id)
                    .await
            }
            _ => {
                // Unknown virtual plugin
                warn!(
                    "[{}] ❓ Unknown virtual plugin: {}",
                    request_id, plugin.command.name
                );
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content("This command is not yet implemented.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                Ok(())
            }
        }
    }

    /// Handle /transcribe_cancel command - cancel an active transcription job
    async fn handle_transcribe_cancel(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        plugin_manager: &Arc<PluginManager>,
        params: &HashMap<String, String>,
        user_id: &str,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::features::plugins::short_job_id;
        use serenity::model::application::interaction::InteractionResponseType;

        info!(
            "[{request_id}] 🛑 Processing transcribe_cancel for user {user_id}"
        );

        // Get optional job_id parameter
        let job_id_param = params.get("job_id").cloned();

        // Find the job to cancel
        let job_to_cancel = if let Some(job_id) = job_id_param {
            // User specified a job ID - look for it
            // Try to find by full ID or short ID prefix
            let active_jobs = plugin_manager
                .job_manager
                .get_user_active_playlist_jobs(user_id);
            active_jobs
                .into_iter()
                .find(|j| j.id == job_id || j.id.starts_with(&job_id))
        } else {
            // No job ID specified - get user's most recent active job
            let active_jobs = plugin_manager
                .job_manager
                .get_user_active_playlist_jobs(user_id);
            active_jobs.into_iter().next()
        };

        match job_to_cancel {
            Some(job) => {
                let job_id = job.id.clone();
                let job_title = job
                    .playlist_title
                    .clone()
                    .unwrap_or_else(|| "Untitled".to_string());

                // Cancel the job
                match plugin_manager
                    .job_manager
                    .cancel_playlist_job(&job_id, user_id)
                    .await
                {
                    Ok(true) => {
                        info!(
                            "[{request_id}] ✅ Cancelled playlist job {job_id} for user {user_id}"
                        );
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message
                                            .content(format!(
                                                "✅ Cancelled transcription job `{}` ({})\n\
                                             Progress: {}/{} videos completed",
                                                short_job_id(&job_id),
                                                job_title,
                                                job.completed_videos,
                                                job.total_videos
                                            ))
                                            .ephemeral(true)
                                    })
                            })
                            .await?;
                    }
                    Ok(false) => {
                        // Job wasn't active (already completed or cancelled)
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message
                                            .content(format!(
                                                "⚠️ Job `{}` is no longer active (status: {})",
                                                short_job_id(&job_id),
                                                job.status
                                            ))
                                            .ephemeral(true)
                                    })
                            })
                            .await?;
                    }
                    Err(e) => {
                        error!("[{request_id}] ❌ Failed to cancel job {job_id}: {e}");
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message
                                            .content(format!("❌ Failed to cancel job: {e}"))
                                            .ephemeral(true)
                                    })
                            })
                            .await?;
                    }
                }
            }
            None => {
                // No active job found
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content(
                                        "❌ No active transcription job found to cancel.\n\
                                                 Use `/transcribe_status` to view your jobs.",
                                    )
                                    .ephemeral(true)
                            })
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle /transcribe_status command - show user's transcription jobs
    async fn handle_transcribe_status(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        plugin_manager: &Arc<PluginManager>,
        user_id: &str,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::features::plugins::short_job_id;
        use serenity::model::application::interaction::InteractionResponseType;

        info!(
            "[{request_id}] 📊 Processing transcribe_status for user {user_id}"
        );

        // Get user's active playlist jobs
        let active_jobs = plugin_manager
            .job_manager
            .get_user_active_playlist_jobs(user_id);

        // Get user's regular (single video) jobs
        let all_jobs = plugin_manager.job_manager.get_user_jobs(user_id);
        let active_video_jobs: Vec<_> = all_jobs
            .into_iter()
            .filter(|j| {
                matches!(
                    j.status,
                    crate::features::plugins::JobStatus::Running
                        | crate::features::plugins::JobStatus::Pending
                )
            })
            .collect();

        if active_jobs.is_empty() && active_video_jobs.is_empty() {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .content(
                                    "📭 You have no active transcription jobs.\n\
                                            Use `/transcribe <url>` to start a transcription.",
                                )
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        // Build status message
        let mut status_lines = vec!["**Your Transcription Jobs:**".to_string()];

        if !active_jobs.is_empty() {
            status_lines.push("\n**Playlist Jobs:**".to_string());
            for job in &active_jobs {
                let progress_pct = job.progress_percent();
                let title = job.playlist_title.as_deref().unwrap_or("Untitled playlist");
                let truncated_title = if title.len() > 40 {
                    format!("{}...", &title[..37])
                } else {
                    title.to_string()
                };
                status_lines.push(format!(
                    "• `{}` \"{}\" - {}/{} videos ({:.0}%)",
                    short_job_id(&job.id),
                    truncated_title,
                    job.completed_videos + job.failed_videos,
                    job.total_videos,
                    progress_pct
                ));
            }
        }

        if !active_video_jobs.is_empty() {
            status_lines.push("\n**Video Jobs:**".to_string());
            for job in &active_video_jobs {
                let url = job
                    .params
                    .get("url")
                    .map(|u| {
                        if u.len() > 50 {
                            format!("{}...", &u[..47])
                        } else {
                            u.clone()
                        }
                    })
                    .unwrap_or_else(|| "Unknown".to_string());
                let status = match job.status {
                    crate::features::plugins::JobStatus::Running => "🔄 Running",
                    crate::features::plugins::JobStatus::Pending => "⏳ Pending",
                    _ => "❓ Unknown",
                };
                status_lines.push(format!(
                    "• `{}` {} - {}",
                    short_job_id(&job.id),
                    url,
                    status
                ));
            }
        }

        status_lines.push("\n*To cancel a job, use `/transcribe_cancel [job_id]`*".to_string());

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(status_lines.join("\n")).ephemeral(true)
                    })
            })
            .await?;

        Ok(())
    }

    async fn handle_text_command_with_id(
        &self,
        ctx: &Context,
        msg: &Message,
        request_id: Uuid,
    ) -> Result<()> {
        let content = msg.content.trim();
        debug!("[{request_id}] Text command received: {content} - directing to slash commands");
        msg.channel_id
            .say(
                &ctx.http,
                "Text commands have been replaced by slash commands. Type `/` to see available commands.",
            )
            .await?;
        info!("[{request_id}] Slash command redirect message sent");

        Ok(())
    }

    pub async fn get_ai_response(&self, system_prompt: &str, user_message: &str) -> Result<String> {
        self.get_ai_response_with_context(
            system_prompt,
            user_message,
            Vec::new(),
            Uuid::new_v4(),
            None,
            None,
            None,
        )
        .await
    }

    /// Get AI response with full context for usage tracking
    #[allow(clippy::too_many_arguments)]
    pub async fn get_ai_response_with_context(
        &self,
        system_prompt: &str,
        user_message: &str,
        conversation_history: Vec<(String, String)>,
        request_id: Uuid,
        user_id: Option<&str>,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
    ) -> Result<String> {
        let start_time = Instant::now();

        info!(
            "[{}] 🤖 Starting OpenAI API request | Model: {} | History messages: {}",
            request_id,
            self.openai_model,
            conversation_history.len()
        );
        debug!(
            "[{}] 📝 System prompt length: {} chars | User message length: {} chars",
            request_id,
            system_prompt.len(),
            user_message.len()
        );
        debug!(
            "[{}] 📝 User message preview: '{}'",
            request_id,
            user_message.chars().take(100).collect::<String>()
        );

        debug!("[{request_id}] 🔨 Building OpenAI message objects");
        let mut messages = vec![ChatCompletionMessage {
            role: ChatCompletionMessageRole::System,
            content: Some(system_prompt.to_string()),
            name: None,
            function_call: None,
            tool_call_id: None,
            tool_calls: None,
        }];

        // Add conversation history
        for (role, content) in conversation_history {
            let message_role = match role.as_str() {
                "user" => ChatCompletionMessageRole::User,
                "assistant" => ChatCompletionMessageRole::Assistant,
                _ => continue, // Skip invalid roles
            };
            messages.push(ChatCompletionMessage {
                role: message_role,
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

        debug!(
            "[{}] ✅ OpenAI message objects built successfully | Message count: {}",
            request_id,
            messages.len()
        );

        // Add timeout to the OpenAI API call (45 seconds)
        debug!("[{request_id}] 🚀 Initiating OpenAI API call with 45-second timeout");
        let chat_completion_future = ChatCompletion::builder(&self.openai_model, messages).create();

        info!("[{request_id}] ⏰ Waiting for OpenAI API response (timeout: 45s)");
        let chat_completion = timeout(TokioDuration::from_secs(45), chat_completion_future)
            .await
            .map_err(|_| {
                let elapsed = start_time.elapsed();
                error!("[{request_id}] ⏱️ OpenAI API request timed out after {elapsed:?}");
                anyhow::anyhow!("OpenAI API request timed out after 45 seconds")
            })?
            .map_err(|e| {
                let elapsed = start_time.elapsed();
                error!("[{request_id}] ❌ OpenAI API error after {elapsed:?}: {e}");
                anyhow::anyhow!("OpenAI API error: {}", e)
            })?;

        let elapsed = start_time.elapsed();
        info!("[{request_id}] ✅ OpenAI API response received after {elapsed:?}");

        // Log usage if we have context
        if let (Some(uid), Some(usage)) = (user_id, &chat_completion.usage) {
            debug!(
                "[{request_id}] 📊 Token usage - Prompt: {}, Completion: {}, Total: {}",
                usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
            );
            self.usage_tracker.log_chat(
                &self.openai_model,
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens,
                uid,
                guild_id,
                channel_id,
                Some(&request_id.to_string()),
                CostBucket::Ask,
            );
        }

        debug!("[{request_id}] 🔍 Parsing OpenAI API response");
        debug!(
            "[{}] 📊 Response choices count: {}",
            request_id,
            chat_completion.choices.len()
        );

        let response = chat_completion
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .ok_or_else(|| {
                error!("[{request_id}] ❌ No content in OpenAI response");
                anyhow::anyhow!("No response from OpenAI")
            })?;

        let trimmed_response = response.trim().to_string();
        info!(
            "[{}] ✅ OpenAI response processed | Length: {} chars | First 100 chars: '{}'",
            request_id,
            trimmed_response.len(),
            trimmed_response.chars().take(100).collect::<String>()
        );

        Ok(trimmed_response)
    }

    /// Handle audio attachments, returns true if any audio was processed
    async fn handle_audio_attachments(
        &self,
        ctx: &Context,
        msg: &Message,
        guild_id_opt: Option<&str>,
    ) -> Result<bool> {
        let user_id = msg.author.id.to_string();
        let mut audio_processed = false;

        // Get output mode setting (transcription_only or with_commentary)
        let output_mode = if let Some(gid) = guild_id_opt {
            self.database
                .get_guild_setting(gid, "audio_transcription_output")
                .await?
                .unwrap_or_else(|| "transcription_only".to_string())
        } else {
            "transcription_only".to_string() // Default for DMs
        };

        for attachment in &msg.attachments {
            if self.is_audio_attachment(&attachment.filename) {
                info!("Processing audio attachment: {}", attachment.filename);
                audio_processed = true;

                msg.channel_id
                    .say(&ctx.http, "🎵 Transcribing your audio... please wait!")
                    .await?;

                match self
                    .audio_transcriber
                    .download_and_transcribe_with_duration(&attachment.url, &attachment.filename)
                    .await
                {
                    Ok(result) => {
                        let transcription = &result.text;

                        // Log Whisper usage
                        self.usage_tracker.log_whisper(
                            result.duration_seconds,
                            &user_id,
                            guild_id_opt,
                            Some(&msg.channel_id.to_string()),
                            CostBucket::Transcription,
                        );

                        if transcription.trim().is_empty() {
                            msg.channel_id
                                .say(&ctx.http, "I couldn't hear anything in that audio file.")
                                .await?;
                        } else {
                            let response = format!("📝 **Transcription:**\n{transcription}");

                            let chunks = chunk_for_message(&response);
                            for chunk in chunks {
                                if !chunk.trim().is_empty() {
                                    msg.channel_id.say(&ctx.http, &chunk).await?;
                                }
                            }

                            // Only generate AI commentary if output mode is "with_commentary"
                            if output_mode == "with_commentary" && !msg.content.trim().is_empty() {
                                let user_persona = self.database.get_user_persona(&user_id).await?;
                                let system_prompt =
                                    self.persona_manager.get_system_prompt(&user_persona, None);
                                let combined_message = format!(
                                    "Based on this transcription: '{}', {}",
                                    transcription, msg.content
                                );

                                match self
                                    .get_ai_response(&system_prompt, &combined_message)
                                    .await
                                {
                                    Ok(ai_response) => {
                                        msg.channel_id.say(&ctx.http, &ai_response).await?;
                                    }
                                    Err(e) => {
                                        error!("AI response error: {e}");
                                    }
                                }
                            }
                        }

                        self.database
                            .log_usage(&user_id, "audio_transcription", None)
                            .await?;
                    }
                    Err(e) => {
                        error!("Transcription error: {e}");
                        msg.channel_id
                            .say(&ctx.http, "Sorry, I couldn't transcribe that audio file. Please make sure it's a valid audio format.")
                            .await?;
                    }
                }
            }
        }

        Ok(audio_processed)
    }

    fn is_audio_attachment(&self, filename: &str) -> bool {
        let audio_extensions = [
            // Whisper native formats
            ".mp3", ".mp4", ".m4a", ".wav", ".webm", ".mpeg", ".mpga",
            // Converted via ffmpeg
            ".flac", ".ogg", ".aac", ".wma", ".mov", ".avi", ".mkv", ".opus", ".m4v",
        ];

        let filename_lower = filename.to_lowercase();
        audio_extensions
            .iter()
            .any(|ext| filename_lower.ends_with(ext))
    }

    /// Check if an attachment is a text-based file that can be read
    fn is_text_attachment(&self, filename: &str) -> bool {
        let text_extensions = [
            // Plain text
            ".txt",
            ".md",
            ".markdown",
            // Data formats
            ".json",
            ".xml",
            ".yaml",
            ".yml",
            ".toml",
            ".csv",
            // Config files
            ".ini",
            ".cfg",
            ".conf",
            ".env",
            // Code files
            ".rs",
            ".py",
            ".js",
            ".ts",
            ".jsx",
            ".tsx",
            ".html",
            ".css",
            ".sh",
            ".bat",
            ".ps1",
            ".sql",
            ".rb",
            ".go",
            ".java",
            ".c",
            ".cpp",
            ".h",
            ".hpp",
            ".cs",
            ".php",
            ".swift",
            ".kt",
            // Log files
            ".log",
        ];

        let filename_lower = filename.to_lowercase();
        text_extensions
            .iter()
            .any(|ext| filename_lower.ends_with(ext))
    }

    /// Read a text attachment and return (filename, content) if successful
    /// Returns None if the file couldn't be read, or a truncation message if too large
    async fn read_text_attachment(
        &self,
        attachment: &serenity::model::channel::Attachment,
    ) -> Result<Option<(String, String)>> {
        const MAX_TEXT_SIZE: u64 = 100_000; // ~100KB limit

        // Check file size first
        if attachment.size > MAX_TEXT_SIZE {
            return Ok(Some((
                attachment.filename.clone(),
                format!(
                    "[File too large: {} bytes, max {} bytes]",
                    attachment.size, MAX_TEXT_SIZE
                ),
            )));
        }

        // Download file content
        let client = reqwest::Client::new();
        let response = match client.get(&attachment.url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!(
                    "Failed to download text attachment {}: {}",
                    attachment.filename, e
                );
                return Ok(None);
            }
        };

        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                warn!(
                    "Failed to read text attachment bytes {}: {}",
                    attachment.filename, e
                );
                return Ok(None);
            }
        };

        // Convert to string (handle encoding gracefully)
        let content = String::from_utf8_lossy(&bytes).to_string();

        // Truncate if content is extremely long (for context window limits)
        const MAX_CONTENT_CHARS: usize = 50_000;
        let final_content = if content.len() > MAX_CONTENT_CHARS {
            format!(
                "{}\n\n[... truncated, showing first {} of {} characters ...]",
                &content[..MAX_CONTENT_CHARS],
                MAX_CONTENT_CHARS,
                content.len()
            )
        } else {
            content
        };

        Ok(Some((attachment.filename.clone(), final_content)))
    }

    /// Fetch text attachments from recent thread messages
    /// Returns list of (filename, content) pairs
    async fn fetch_thread_attachments(
        &self,
        ctx: &Context,
        channel_id: serenity::model::id::ChannelId,
        limit: u8,
        request_id: Uuid,
    ) -> Result<Vec<(String, String)>> {
        use serenity::builder::GetMessages;

        debug!("[{request_id}] 📎 Fetching text attachments from thread (limit: {limit})");

        // Fetch messages from the thread
        let messages = channel_id
            .messages(&ctx.http, |builder: &mut GetMessages| {
                builder.limit(limit as u64)
            })
            .await?;

        let mut attachments = Vec::new();

        for message in messages.iter() {
            for attachment in &message.attachments {
                if self.is_text_attachment(&attachment.filename) {
                    debug!(
                        "[{request_id}] 📄 Found text attachment: {}",
                        attachment.filename
                    );
                    if let Ok(Some((filename, content))) =
                        self.read_text_attachment(attachment).await
                    {
                        attachments.push((filename, content));
                    }
                }
            }
        }

        debug!(
            "[{request_id}] 📎 Found {} text attachments in thread",
            attachments.len()
        );
        Ok(attachments)
    }

    /// Check if a message seems to be asking about uploaded files
    fn seems_like_file_question(&self, message: &str) -> bool {
        let lower = message.to_lowercase();
        let file_keywords = [
            "file",
            "transcript",
            "attachment",
            "uploaded",
            "document",
            "what's in",
            "what is in",
            "read the",
            "show me the",
            "contents of",
            "the .txt",
            "the .md",
            "the .json",
            "the .log",
            "summary of",
            "summarize the",
            "analyze the",
            "in the file",
        ];
        file_keywords.iter().any(|kw| lower.contains(kw))
    }

    /// Read text attachments from current message and format them for AI context
    async fn get_text_attachments_context(
        &self,
        msg: &Message,
        request_id: Uuid,
    ) -> Vec<(String, String)> {
        let mut attachments = Vec::new();

        for attachment in &msg.attachments {
            if self.is_text_attachment(&attachment.filename) {
                debug!(
                    "[{request_id}] 📄 Reading text attachment from message: {}",
                    attachment.filename
                );
                match self.read_text_attachment(attachment).await {
                    Ok(Some((filename, content))) => {
                        attachments.push((filename, content));
                    }
                    Ok(None) => {
                        debug!(
                            "[{request_id}] ⚠️ Could not read attachment: {}",
                            attachment.filename
                        );
                    }
                    Err(e) => {
                        warn!(
                            "[{request_id}] ❌ Error reading attachment {}: {}",
                            attachment.filename, e
                        );
                    }
                }
            }
        }

        attachments
    }

    /// Format text attachments into a context string to prepend to user message
    fn format_attachments_for_context(&self, attachments: &[(String, String)]) -> String {
        if attachments.is_empty() {
            return String::new();
        }

        let mut context = String::new();
        for (filename, content) in attachments {
            context.push_str(&format!(
                "[Attached file: {filename}]\n```\n{content}\n```\n\n"
            ));
        }
        context
    }

    async fn check_and_mediate_conflicts(
        &self,
        ctx: &Context,
        msg: &Message,
        channel_id: &str,
        guild_id: Option<&str>,
    ) -> Result<()> {
        // Get guild-specific conflict sensitivity
        let sensitivity_threshold = if let Some(gid) = guild_id {
            let sensitivity = self
                .database
                .get_guild_setting(gid, "conflict_sensitivity")
                .await?
                .unwrap_or_else(|| "medium".to_string());
            match sensitivity.as_str() {
                "low" => 0.7,
                "high" => 0.35,
                "ultra" => 0.3,
                _ => self.conflict_sensitivity_threshold, // Use env var default
            }
        } else {
            self.conflict_sensitivity_threshold
        };

        // Get guild-specific mediation cooldown
        let cooldown_minutes = if let Some(gid) = guild_id {
            self.database
                .get_guild_setting(gid, "mediation_cooldown")
                .await?
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(5) // Default 5 minutes
        } else {
            5
        };

        // Get the timestamp of the last mediation to avoid re-analyzing same messages
        let last_mediation_ts = self
            .database
            .get_last_mediation_timestamp(channel_id)
            .await?;

        // Get recent messages, optionally filtering to only new messages since last mediation
        let recent_messages = if let Some(last_ts) = last_mediation_ts {
            info!("🔍 Getting messages since last mediation at timestamp {last_ts}");
            self.database
                .get_recent_channel_messages_since(channel_id, last_ts, 10)
                .await?
        } else {
            info!("🔍 No previous mediation found, getting all recent messages");
            self.database
                .get_recent_channel_messages(channel_id, 10)
                .await?
        };

        info!(
            "🔍 Conflict check: Found {} recent messages in channel {} (after last mediation)",
            recent_messages.len(),
            channel_id
        );

        if recent_messages.is_empty() {
            info!("⏭️ Skipping conflict detection: No messages found");
            return Ok(());
        }

        // Log message samples for debugging
        let unique_users: std::collections::HashSet<_> = recent_messages
            .iter()
            .map(|(user_id, _, _)| user_id.clone())
            .collect();
        info!("👥 Messages from {} unique users", unique_users.len());

        for (i, (user_id, content, timestamp)) in recent_messages.iter().take(3).enumerate() {
            debug!("  Message {i}: User={user_id} | Content='{content}' | Time={timestamp}");
        }

        // Detect conflicts in recent messages
        let (is_conflict, confidence, conflict_type) = self
            .conflict_detector
            .detect_heated_argument(&recent_messages, 120);

        info!("📊 Detection result: conflict={is_conflict} | confidence={confidence:.2} | threshold={sensitivity_threshold:.2} | type='{conflict_type}' | cooldown={cooldown_minutes}min");

        if is_conflict && confidence >= sensitivity_threshold {
            info!("🔥 Conflict detected in channel {channel_id} | Confidence: {confidence:.2} | Type: {conflict_type}");

            // Check cooldown using last mediation timestamp and guild-specific cooldown
            if let Some(last_ts) = last_mediation_ts {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let cooldown_secs = (cooldown_minutes * 60) as i64;
                if now - last_ts < cooldown_secs {
                    info!(
                        "⏸️ Mediation on cooldown for channel {} ({}s remaining)",
                        channel_id,
                        cooldown_secs - (now - last_ts)
                    );
                    return Ok(());
                }
            }

            // Also check the in-memory rate limiter
            if !self.conflict_mediator.can_intervene(channel_id) {
                info!("⏸️ Mediation on cooldown for channel {channel_id} (in-memory limiter)");
                return Ok(());
            }

            // Extract participant user IDs
            let participants: Vec<String> = recent_messages
                .iter()
                .map(|(user_id, _, _)| user_id.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            info!("👥 Conflict participants: {} users", participants.len());

            if participants.is_empty() {
                info!("⏭️ Skipping mediation: No participants found");
                return Ok(());
            }

            // Record the conflict in database
            let participants_json = serde_json::to_string(&participants)?;
            let conflict_id = self
                .database
                .record_conflict_detection(
                    channel_id,
                    guild_id,
                    &participants_json,
                    &conflict_type,
                    confidence,
                    &msg.id.to_string(),
                )
                .await?;

            // Generate context-aware mediation response using OpenAI
            info!("🤖 Generating context-aware mediation response with OpenAI...");
            let mediation_text = match self
                .generate_mediation_response(
                    &recent_messages,
                    &conflict_type,
                    confidence,
                    guild_id,
                    channel_id,
                )
                .await
            {
                Ok(response) => {
                    info!("✅ OpenAI mediation response generated successfully");
                    response
                }
                Err(e) => {
                    warn!("⚠️ Failed to generate AI mediation response: {e}. Using fallback.");
                    self.conflict_mediator
                        .get_mediation_response(&conflict_type, confidence)
                }
            };

            // Send mediation message as Obi-Wan with proper error handling
            match msg.channel_id.say(&ctx.http, &mediation_text).await {
                Ok(mediation_msg) => {
                    info!("☮️ Mediation sent successfully in channel {channel_id} | Message: {mediation_text}");

                    // Record the intervention
                    self.conflict_mediator.record_intervention(channel_id);

                    // Record in database
                    self.database
                        .mark_mediation_triggered(conflict_id, &mediation_msg.id.to_string())
                        .await?;
                    self.database
                        .record_mediation(conflict_id, channel_id, &mediation_text)
                        .await?;
                }
                Err(e) => {
                    warn!("⚠️ Failed to send mediation message to Discord: {e}. Recording intervention to prevent spam.");

                    // Still record the intervention to prevent repeated mediation attempts
                    self.conflict_mediator.record_intervention(channel_id);

                    // Try to record in database with no message ID
                    if let Err(db_err) = self
                        .database
                        .record_mediation(conflict_id, channel_id, &mediation_text)
                        .await
                    {
                        warn!("⚠️ Failed to record mediation in database: {db_err}");
                    }
                }
            }

            // Update user interaction patterns
            if participants.len() == 2 {
                let user_a = &participants[0];
                let user_b = &participants[1];
                self.database
                    .update_user_interaction_pattern(user_a, user_b, channel_id, true)
                    .await?;
            }
        }

        Ok(())
    }

    /// Generate a context-aware mediation response using OpenAI
    async fn generate_mediation_response(
        &self,
        messages: &[(String, String, String)], // (user_id, content, timestamp)
        conflict_type: &str,
        confidence: f32,
        guild_id: Option<&str>,
        channel_id: &str,
    ) -> Result<String> {
        // Build conversation context from recent messages
        let mut conversation_context = String::new();
        for (user_id, content, _timestamp) in messages.iter().rev().take(5) {
            conversation_context.push_str(&format!("User {user_id}: {content}\n"));
        }

        // Create system prompt for Obi-Wan as mediator
        let mediation_prompt = format!(
            "You are Obi-Wan Kenobi observing a conversation that has become heated. \
            Your role is to gently mediate and bring calm wisdom to the situation.\n\n\
            Conflict type detected: {}\n\
            Confidence: {:.0}%\n\n\
            Recent conversation:\n{}\n\n\
            Respond with a brief, characteristic Obi-Wan comment that:\n\
            1. Acknowledges what's being discussed specifically\n\
            2. Offers a calming philosophical perspective\n\
            3. Encourages understanding or reflection\n\
            4. Stays in character with Obi-Wan's wise, measured tone\n\n\
            Keep it to 1-2 sentences maximum. Be natural and conversational, not preachy.",
            conflict_type,
            confidence * 100.0,
            conversation_context
        );

        // Call OpenAI (API key set at startup)
        let chat_completion = ChatCompletion::builder(
            &self.openai_model,
            vec![ChatCompletionMessage {
                role: ChatCompletionMessageRole::System,
                content: Some(mediation_prompt),
                name: None,
                function_call: None,
                tool_call_id: None,
                tool_calls: None,
            }],
        )
        .create()
        .await
        .map_err(|e| {
            error!("Conflict mediation OpenAI API error: {e}");
            anyhow::anyhow!("OpenAI API error: {e}")
        })?;

        // Log usage for mediation (system-initiated, no specific user)
        if let Some(usage) = &chat_completion.usage {
            self.usage_tracker.log_chat(
                &self.openai_model,
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens,
                "system_mediation", // Special user_id for system-initiated requests
                guild_id,
                Some(channel_id),
                None,
                CostBucket::Mediation,
            );
        }

        let response = chat_completion
            .choices
            .first()
            .and_then(|choice| choice.message.content.clone())
            .unwrap_or_else(|| {
                "I sense tension here. Perhaps a moment of calm reflection would serve us all well."
                    .to_string()
            });

        Ok(response)
    }

    /// Handle a follow-up question in an active council thread
    ///
    /// All council personas respond to the follow-up with context from the discussion.
    async fn handle_council_followup(
        &self,
        ctx: &Context,
        msg: &Message,
        request_id: Uuid,
    ) -> Result<()> {
        let channel_id = msg.channel_id;
        let user_id = msg.author.id.to_string();

        // Get the council state
        let council_state = match get_active_councils().get(&channel_id.0) {
            Some(state) => state.clone(),
            None => {
                debug!(
                    "[{request_id}] No active council found for channel {channel_id}"
                );
                return Ok(());
            }
        };

        // Extract the question (remove bot mention)
        let question = self.strip_bot_mention(ctx, &msg.content).await?;
        if question.trim().is_empty() {
            return Ok(());
        }

        info!(
            "[{request_id}] Council follow-up question: '{}' from user {}",
            question.chars().take(50).collect::<String>(),
            user_id
        );

        // Add the user's question to history
        if let Some(mut state) = get_active_councils().get_mut(&channel_id.0) {
            state.add_user_message(question.clone());
        }

        // Send typing indicator
        let _ = channel_id.broadcast_typing(&ctx.http).await;

        // Get context summary for the personas
        let context_summary = council_state.get_context_summary();

        // Clone values for async task
        let openai_model = self.openai_model.clone();
        let usage_tracker = self.usage_tracker.clone();
        let persona_manager = self.persona_manager.clone();
        let persona_ids = council_state.persona_ids.clone();
        let guild_id = council_state.guild_id.clone();
        let ctx_clone = ctx.clone();
        let channel_id_str = channel_id.to_string();

        // Post a "reconvening" message
        let reconvene_embed = serenity::builder::CreateEmbed::default()
            .title("Council Reconvening")
            .description(format!("**Follow-up Question:** {question}"))
            .color(0x9B59B6)
            .to_owned();

        let _ = channel_id
            .send_message(&ctx.http, |m| m.set_embed(reconvene_embed))
            .await;

        // Spawn task to get responses from each persona
        tokio::spawn(async move {
            for (i, persona_id) in persona_ids.iter().enumerate() {
                let persona = match persona_manager.get_persona_with_portrait(persona_id) {
                    Some(p) => p,
                    None => continue,
                };

                // Get system prompt for this persona
                let system_prompt = persona_manager.get_system_prompt(persona_id, None);

                // Build context-aware prompt
                let council_context = format!(
                    "{}\n\n\
                    You are participating in a council discussion with other personas.\n\n\
                    {}\n\n\
                    A user has asked a follow-up question. Respond thoughtfully, considering \
                    what was discussed before. Be concise but insightful. Stay in character as {}.",
                    system_prompt, context_summary, persona.name
                );

                // Build messages for OpenAI
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
                        content: Some(question.clone()),
                        name: None,
                        function_call: None,
                        tool_call_id: None,
                        tool_calls: None,
                    },
                ];

                // Call OpenAI
                let response = match openai::chat::ChatCompletion::builder(&openai_model, messages)
                    .create()
                    .await
                {
                    Ok(completion) => {
                        // Log usage
                        if let Some(usage) = &completion.usage {
                            usage_tracker.log_chat(
                                &openai_model,
                                usage.prompt_tokens,
                                usage.completion_tokens,
                                usage.total_tokens,
                                &user_id,
                                guild_id.as_deref(),
                                Some(&channel_id_str),
                                Some(&request_id.to_string()),
                                CostBucket::Council,
                            );
                        }

                        completion
                            .choices
                            .first()
                            .and_then(|c| c.message.content.clone())
                            .unwrap_or_else(|| "I have no further words at this time.".to_string())
                    }
                    Err(e) => {
                        error!(
                            "[{request_id}] Council follow-up: Failed to get response from {}: {}",
                            persona.name, e
                        );
                        format!("*{} contemplates in silence...*", persona.name)
                    }
                };

                // Add response to council history
                if let Some(mut state) = get_active_councils().get_mut(&channel_id.0) {
                    state.add_persona_response(persona_id, response.clone());
                }

                // Build embed for this persona's response
                let mut embed = serenity::builder::CreateEmbed::default();
                embed.author(|a| {
                    a.name(&persona.name);
                    if let Some(url) = &persona.portrait_url {
                        a.icon_url(url);
                    }
                    a
                });
                embed.color(persona.color);

                // Handle long responses
                embed.description(truncate_for_embed(&response));

                // Send the response
                if let Err(e) = channel_id
                    .send_message(&ctx_clone.http, |m| m.set_embed(embed.clone()))
                    .await
                {
                    error!(
                        "[{request_id}] Council follow-up: Failed to send message from {}: {}",
                        persona.name, e
                    );
                }

                // Small delay between responses
                if i < persona_ids.len() - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }

            info!("[{request_id}] Council follow-up completed");
        });

        Ok(())
    }

    /// Strip bot mention from message content
    async fn strip_bot_mention(&self, ctx: &Context, content: &str) -> Result<String> {
        let current_user = ctx.http.get_current_user().await?;
        let bot_mention = format!("<@{}>", current_user.id);
        let bot_mention_nick = format!("<@!{}>", current_user.id);

        let stripped = content
            .replace(&bot_mention, "")
            .replace(&bot_mention_nick, "")
            .trim()
            .to_string();

        Ok(stripped)
    }
}
