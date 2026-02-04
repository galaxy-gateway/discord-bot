use crate::features::audio::transcriber::AudioTranscriber;
use crate::features::conflict::{ConflictDetector, ConflictMediator};
use crate::features::debate::{DebateOrchestrator, orchestrator::DebateConfig};
use crate::features::image_gen::generator::{ImageGenerator, ImageSize, ImageStyle};
use crate::features::analytics::InteractionTracker;
use crate::features::introspection::get_component_snippet;
use crate::features::personas::{PersonaManager, Persona};
use crate::features::plugins::PluginManager;
use crate::features::rate_limiting::RateLimiter;
use crate::features::analytics::UsageTracker;
use crate::database::Database;
use crate::message_components::MessageComponentHandler;
use crate::commands::slash::{get_string_option, get_channel_option, get_role_option, get_integer_option, get_bool_option};
use anyhow::Result;
use log::{debug, error, info, warn};
use tokio::time::{timeout, Duration as TokioDuration, Instant};
use uuid::Uuid;
use std::sync::Arc;
use std::collections::HashMap;
use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};
use serenity::builder::CreateEmbed;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::channel::Message;
use serenity::prelude::Context;
use std::time::Duration;

#[derive(Clone)]
pub struct CommandHandler {
    persona_manager: PersonaManager,
    database: Database,
    rate_limiter: RateLimiter,
    audio_transcriber: AudioTranscriber,
    image_generator: ImageGenerator,
    openai_model: String,
    conflict_detector: ConflictDetector,
    conflict_mediator: ConflictMediator,
    conflict_enabled: bool,
    conflict_sensitivity_threshold: f32,
    start_time: std::time::Instant,
    usage_tracker: UsageTracker,
    interaction_tracker: InteractionTracker,
    plugin_manager: Option<Arc<PluginManager>>,
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
            "low" => 0.7,      // Only very high confidence conflicts
            "high" => 0.35,    // More sensitive - catches single keywords + context
            "ultra" => 0.3,    // Maximum sensitivity - triggers on single hostile keyword
            _ => 0.5,          // Medium (default)
        };

        CommandHandler {
            persona_manager: PersonaManager::new(),
            database,
            rate_limiter: RateLimiter::new(10, Duration::from_secs(60)),
            audio_transcriber: AudioTranscriber::new(openai_api_key.clone()),
            image_generator: ImageGenerator::new(openai_api_key),
            openai_model,
            conflict_detector: ConflictDetector::new(),
            conflict_mediator: ConflictMediator::new(999, mediation_cooldown_minutes), // High limit for testing
            conflict_enabled,
            conflict_sensitivity_threshold: sensitivity_threshold,
            start_time: std::time::Instant::now(),
            usage_tracker,
            interaction_tracker,
            plugin_manager: None,
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
        let text = if response_text.len() > 4096 {
            &response_text[..4096]
        } else {
            response_text
        };
        embed.description(text);

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
        let guild_id = msg.guild_id.map(|id| id.to_string()).unwrap_or_else(|| "DM".to_string());
        let guild_id_opt = if guild_id != "DM" { Some(guild_id.as_str()) } else { None };

        info!("[{}] 📥 Message received | User: {} | Channel: {} | Guild: {} | Content: '{}'",
              request_id, user_id, channel_id, guild_id,
              msg.content.chars().take(100).collect::<String>());

        debug!("[{request_id}] 🔍 Checking rate limit for user: {user_id}");
        if !self.rate_limiter.wait_for_rate_limit(&user_id).await {
            warn!("[{request_id}] 🚫 Rate limit exceeded for user: {user_id}");
            debug!("[{request_id}] 📤 Sending rate limit message to Discord");
            msg.channel_id
                .say(&ctx.http, "You're sending messages too quickly! Please slow down.")
                .await?;
            info!("[{request_id}] ✅ Rate limit message sent successfully");
            return Ok(());
        }
        debug!("[{request_id}] ✅ Rate limit check passed");

        // Get audio transcription mode for this guild
        let is_dm = msg.guild_id.is_none();
        let audio_mode = if let Some(gid) = guild_id_opt {
            let feature_enabled = self.database.is_feature_enabled("audio_transcription", None, Some(gid)).await?;
            if !feature_enabled {
                "disabled".to_string()
            } else {
                self.database.get_guild_setting(gid, "audio_transcription_mode").await?
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
            debug!("[{}] 🎵 Processing {} audio attachments (mode: {})", request_id, msg.attachments.len(), audio_mode);
            self.handle_audio_attachments(ctx, msg, guild_id_opt).await?
        } else {
            false
        };

        let content = msg.content.trim();
        debug!("[{}] 🔍 Analyzing message content | Length: {} | Is DM: {} | Starts with command: {}",
               request_id, content.len(), is_dm, content.starts_with('/'));

        // Store guild messages FIRST (needed for conflict detection to have data)
        if !is_dm && !content.is_empty() && !content.starts_with('/') {
            debug!("[{request_id}] 💾 Storing guild message for analysis");
            self.database.store_message(&user_id, &channel_id, "user", content, None).await?;
        }

        // Conflict detection - check both env var AND feature flag
        let guild_conflict_enabled = if let Some(gid) = guild_id_opt {
            self.database.is_feature_enabled("conflict_mediation", None, Some(gid)).await?
        } else {
            false // No conflict detection in DMs
        };

        if !is_dm && self.conflict_enabled && guild_conflict_enabled && !content.is_empty() && !content.starts_with('/') {
            debug!("[{request_id}] 🔍 Running conflict detection analysis");
            if let Err(e) = self.check_and_mediate_conflicts(ctx, msg, &channel_id, guild_id_opt).await {
                warn!("[{request_id}] ⚠️ Conflict detection error: {e}");
                // Don't fail the whole message processing if conflict detection fails
            }
        }

        if content.starts_with('/') {
            info!("[{}] 🎯 Processing text command: {}", request_id, content.split_whitespace().next().unwrap_or(""));
            self.handle_text_command_with_id(ctx, msg, request_id).await?;
        } else if is_dm && !content.is_empty() && !audio_handled {
            info!("[{request_id}] 💬 Processing DM message (auto-response mode)");
            self.handle_dm_message_with_id(ctx, msg, request_id).await?;
        } else if !is_dm && !audio_handled && !content.is_empty() && self.is_in_active_debate_thread(msg.channel_id).await {
            // Check if debate_auto_response is enabled for this guild
            let auto_response_enabled = if let Some(gid) = guild_id_opt {
                self.database.get_guild_setting(gid, "debate_auto_response").await?
                    .map(|v| v == "enabled")
                    .unwrap_or(false) // Default disabled
            } else {
                false
            };

            if auto_response_enabled {
                info!("[{request_id}] 🎭 Auto-responding in active debate thread");
                self.handle_mention_message_with_id(ctx, msg, request_id).await?;
            } else {
                debug!("[{request_id}] ℹ️ Message in debate thread but auto-response disabled");
            }
        } else if !is_dm && !audio_handled && self.is_bot_mentioned(ctx, msg).await? && !content.is_empty() {
            // Check mention_responses guild setting
            let mention_enabled = if let Some(gid) = guild_id_opt {
                self.database.get_guild_setting(gid, "mention_responses").await?
                    .map(|v| v == "enabled")
                    .unwrap_or(true) // Default enabled
            } else {
                true
            };

            if mention_enabled {
                info!("[{request_id}] 🏷️ Bot mentioned in channel - responding");
                self.handle_mention_message_with_id(ctx, msg, request_id).await?;
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

    async fn is_in_thread(&self, ctx: &Context, msg: &Message) -> Result<bool> {
        use serenity::model::channel::{Channel, ChannelType};

        // Fetch the channel to check its type
        match ctx.http.get_channel(msg.channel_id.0).await {
            Ok(Channel::Guild(guild_channel)) => {
                Ok(matches!(guild_channel.kind,
                    ChannelType::PublicThread | ChannelType::PrivateThread))
            }
            _ => Ok(false),
        }
    }

    async fn fetch_thread_messages(&self, ctx: &Context, msg: &Message, limit: u8, request_id: Uuid) -> Result<Vec<(String, String)>> {
        use serenity::builder::GetMessages;

        debug!("[{request_id}] 🧵 Fetching up to {limit} messages from thread");

        // Fetch messages from the thread (Discord API limit is 100)
        let messages = msg.channel_id.messages(&ctx.http, |builder: &mut GetMessages| {
            builder.limit(limit as u64)
        }).await?;

        debug!("[{}] 🧵 Retrieved {} messages from thread", request_id, messages.len());

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

        debug!("[{}] 🧵 Processed {} non-empty messages from thread", request_id, conversation.len());

        Ok(conversation)
    }

    /// Fetch all messages from a debate thread for tag-team context
    /// This includes both debate embeds (persona responses) and regular messages (user Q&A)
    async fn fetch_thread_history_for_debate(
        &self,
        ctx: &Context,
        channel_id: serenity::model::id::ChannelId,
        request_id: Uuid,
    ) -> Result<Vec<(String, String)>> {
        use serenity::builder::GetMessages;

        debug!("[{request_id}] 🎭 Fetching thread history for tag-team debate");

        // Fetch up to 100 messages (Discord API limit)
        let messages = channel_id.messages(&ctx.http, |builder: &mut GetMessages| {
            builder.limit(100)
        }).await?;

        debug!("[{}] 🎭 Retrieved {} messages from debate thread", request_id, messages.len());

        // Get bot's user ID to identify bot messages
        let current_user = ctx.http.get_current_user().await?;
        let bot_id = current_user.id;

        // Process messages in chronological order (oldest first)
        let mut history: Vec<(String, String)> = Vec::new();

        for msg in messages.iter().rev() {
            // Check for embeds first (debate responses have persona name in author)
            if !msg.embeds.is_empty() {
                for embed in &msg.embeds {
                    // Debate embeds have author.name = persona name and description = response
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
                // Regular text message (user question or bot response outside debate)
                let speaker = if msg.author.id == bot_id {
                    "Assistant".to_string()
                } else {
                    format!("User ({})", msg.author.name)
                };
                history.push((speaker, msg.content.clone()));
            }
        }

        debug!(
            "[{}] 🎭 Processed {} entries from debate thread for tag-team context",
            request_id, history.len()
        );

        Ok(history)
    }

    async fn handle_dm_message_with_id(&self, ctx: &Context, msg: &Message, request_id: Uuid) -> Result<()> {
        let start_time = Instant::now();
        let user_id = msg.author.id.to_string();
        let channel_id = msg.channel_id.to_string();
        let user_message = msg.content.trim();

        debug!("[{}] 💬 Processing DM auto-response | User: {} | Message: '{}'",
               request_id, user_id, user_message.chars().take(100).collect::<String>());

        // Get or create DM session
        let session_id = self.interaction_tracker.get_or_create_session(&user_id, &channel_id);
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
            info!("[{request_id}] 📎 Including {} text attachment(s) in context", text_attachments.len());
            format!("{}{}", attachment_context, user_message)
        };

        // Get user's persona
        debug!("[{request_id}] 🎭 Fetching user persona from database");
        let user_persona = self.database.get_user_persona(&user_id).await?;
        debug!("[{request_id}] 🎭 User persona: {user_persona}");

        // Store user message in conversation history (store original message, not enhanced)
        debug!("[{request_id}] 💾 Storing user message to conversation history");
        self.database.store_message(&user_id, &channel_id, "user", user_message, Some(&user_persona)).await?;
        debug!("[{request_id}] ✅ User message stored successfully");

        // Retrieve conversation history (last 40 messages = ~20 exchanges)
        debug!("[{request_id}] 📚 Retrieving conversation history");
        let conversation_history = self.database.get_conversation_history(&user_id, &channel_id, 40).await?;
        info!("[{}] 📚 Retrieved {} historical messages", request_id, conversation_history.len());

        // Show typing indicator while processing
        debug!("[{request_id}] ⌨️ Starting typing indicator");
        let typing = msg.channel_id.start_typing(&ctx.http)?;

        // Build system prompt without modifier (conversational mode)
        debug!("[{request_id}] 📝 Building system prompt | Persona: {user_persona}");
        let system_prompt = self.persona_manager.get_system_prompt(&user_persona, None);
        debug!("[{}] ✅ System prompt generated | Length: {} chars", request_id, system_prompt.len());

        // Log usage
        debug!("[{request_id}] 📊 Logging usage to database");
        self.database.log_usage(&user_id, "dm_chat", Some(&user_persona)).await?;
        debug!("[{request_id}] ✅ Usage logged successfully");

        // Get AI response with conversation history (use enhanced message with attachments)
        info!("[{request_id}] 🚀 Calling OpenAI API for DM response");
        let api_call_result = self.get_ai_response_with_context(&system_prompt, &enhanced_message, conversation_history, request_id, Some(&user_id), None, Some(&channel_id)).await;

        // Track API call (estimate cost from usage tracker's pricing)
        // This will be more accurate if we can access the actual usage data, but for now we'll track it after response

        match api_call_result {
            Ok(ai_response) => {
                info!("[{}] ✅ OpenAI response received | Response length: {}",
                      request_id, ai_response.len());

                // Stop typing
                typing.stop();
                debug!("[{request_id}] ⌨️ Stopped typing indicator");

                // Get persona for embed styling
                let persona = self.persona_manager.get_persona(&user_persona);

                // Send response as embed (handle long messages - embed description limit is 4096)
                if ai_response.len() > 4096 {
                    debug!("[{request_id}] 📄 Response too long for single embed, splitting into chunks");
                    let chunks: Vec<&str> = ai_response.as_bytes()
                        .chunks(4096)
                        .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                        .collect();

                    debug!("[{}] 📄 Split response into {} embed chunks", request_id, chunks.len());

                    for (i, chunk) in chunks.iter().enumerate() {
                        if !chunk.trim().is_empty() {
                            debug!("[{}] 📤 Sending embed chunk {} of {} ({} chars)",
                                   request_id, i + 1, chunks.len(), chunk.len());

                            if let Some(p) = persona {
                                // First chunk gets full embed with author, rest are continuation
                                let embed = if i == 0 {
                                    Self::build_persona_embed(p, chunk)
                                } else {
                                    Self::build_continuation_embed(p, chunk)
                                };
                                msg.channel_id.send_message(&ctx.http, |m| m.set_embed(embed)).await?;
                            } else {
                                // Fallback to plain text if persona not found
                                msg.channel_id.say(&ctx.http, chunk).await?;
                            }
                            debug!("[{}] ✅ Embed chunk {} sent successfully", request_id, i + 1);
                        }
                    }
                    info!("[{request_id}] ✅ All DM embed response chunks sent successfully");
                } else {
                    debug!("[{}] 📤 Sending DM embed response ({} chars)", request_id, ai_response.len());
                    if let Some(p) = persona {
                        let embed = Self::build_persona_embed(p, &ai_response);
                        msg.channel_id.send_message(&ctx.http, |m| m.set_embed(embed)).await?;
                    } else {
                        // Fallback to plain text if persona not found
                        msg.channel_id.say(&ctx.http, &ai_response).await?;
                    }
                    info!("[{request_id}] ✅ DM embed response sent successfully");
                }

                // Store assistant response in conversation history
                debug!("[{request_id}] 💾 Storing assistant response to conversation history");
                self.database.store_message(&user_id, &channel_id, "assistant", &ai_response, Some(&user_persona)).await?;
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
                debug!("[{request_id}] 📊 Tracked message sent (response time: {}ms)", response_time_ms);
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

    async fn handle_mention_message_with_id(&self, ctx: &Context, msg: &Message, request_id: Uuid) -> Result<()> {
        let user_id = msg.author.id.to_string();
        let channel_id = msg.channel_id.to_string();
        let guild_id = msg.guild_id.map(|id| id.to_string());
        let guild_id_opt = guild_id.as_deref();
        let user_message = msg.content.trim();

        debug!("[{}] 🏷️ Processing mention in channel | User: {} | Message: '{}'",
               request_id, user_id, user_message.chars().take(100).collect::<String>());

        // Get user's persona with channel override -> user -> guild default cascade
        debug!("[{request_id}] 🎭 Fetching user persona from database");
        let user_persona = if let Some(gid) = guild_id_opt {
            self.database.get_persona_with_channel(&user_id, gid, &channel_id).await?
        } else {
            self.database.get_user_persona_with_guild(&user_id, None).await?
        };
        debug!("[{request_id}] 🎭 User persona: {user_persona}");

        // Get max_context_messages from guild settings
        let max_context = if let Some(gid) = guild_id_opt {
            self.database.get_guild_setting(gid, "max_context_messages").await?
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
            info!("[{request_id}] 📎 User asking about files in thread, fetching thread attachments");
            let thread_attachments = self.fetch_thread_attachments(ctx, msg.channel_id, 20, request_id).await?;
            all_attachments.extend(thread_attachments);
        }

        // Format attachment context
        let attachment_context = self.format_attachments_for_context(&all_attachments);

        // Enhance user message with attachment content if present
        let enhanced_message = if attachment_context.is_empty() {
            user_message.to_string()
        } else {
            info!("[{request_id}] 📎 Including {} text attachment(s) in context", all_attachments.len());
            format!("{}{}", attachment_context, user_message)
        };

        // Retrieve conversation history based on context type
        let conversation_history = if is_thread {
            // Thread context: Fetch messages from Discord
            info!("[{request_id}] 🧵 Fetching thread context from Discord");
            self.fetch_thread_messages(ctx, msg, max_context as u8, request_id).await?
        } else {
            // Channel context: Use database history
            info!("[{request_id}] 📚 Fetching channel context from database");

            // Store user message in conversation history for channels (store original, not enhanced)
            debug!("[{request_id}] 💾 Storing user message to conversation history");
            self.database.store_message(&user_id, &channel_id, "user", user_message, Some(&user_persona)).await?;
            debug!("[{request_id}] ✅ User message stored successfully");

            self.database.get_conversation_history(&user_id, &channel_id, max_context).await?
        };

        info!("[{}] 📚 Retrieved {} historical messages for context", request_id, conversation_history.len());

        // Show typing indicator while processing
        debug!("[{request_id}] ⌨️ Starting typing indicator");
        let typing = msg.channel_id.start_typing(&ctx.http)?;

        // Get channel verbosity for guild channels
        let verbosity = if let Some(guild_id) = msg.guild_id {
            self.database.get_channel_verbosity(&guild_id.to_string(), &channel_id).await?
        } else {
            "concise".to_string()
        };

        // Build system prompt without modifier (conversational mode), with verbosity
        debug!("[{request_id}] 📝 Building system prompt | Persona: {user_persona} | Verbosity: {verbosity}");
        let system_prompt = self.persona_manager.get_system_prompt_with_verbosity(&user_persona, None, &verbosity);
        debug!("[{}] ✅ System prompt generated | Length: {} chars", request_id, system_prompt.len());

        // Log usage
        debug!("[{request_id}] 📊 Logging usage to database");
        self.database.log_usage(&user_id, "mention_chat", Some(&user_persona)).await?;
        debug!("[{request_id}] ✅ Usage logged successfully");

        // Get AI response with conversation history (use enhanced message with attachments)
        info!("[{request_id}] 🚀 Calling OpenAI API for mention response");
        match self.get_ai_response_with_context(&system_prompt, &enhanced_message, conversation_history, request_id, Some(&user_id), guild_id_opt, Some(&channel_id)).await {
            Ok(ai_response) => {
                info!("[{}] ✅ OpenAI response received | Response length: {}",
                      request_id, ai_response.len());

                // Stop typing
                typing.stop();
                debug!("[{request_id}] ⌨️ Stopped typing indicator");

                // Check if embeds are enabled for this guild
                let use_embeds = if let Some(gid) = guild_id_opt {
                    self.database.get_guild_setting(gid, "response_embeds").await
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
                    if ai_response.len() > 4096 {
                        debug!("[{request_id}] 📄 Response too long for single embed, splitting into chunks");
                        let chunks: Vec<&str> = ai_response.as_bytes()
                            .chunks(4096)
                            .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                            .collect();

                        debug!("[{}] 📄 Split response into {} embed chunks", request_id, chunks.len());

                        for (i, chunk) in chunks.iter().enumerate() {
                            if !chunk.trim().is_empty() {
                                debug!("[{}] 📤 Sending embed chunk {} of {} ({} chars)",
                                       request_id, i + 1, chunks.len(), chunk.len());

                                // First chunk gets full embed with author, rest are continuation
                                let embed = if i == 0 {
                                    Self::build_persona_embed(p, chunk)
                                } else {
                                    Self::build_continuation_embed(p, chunk)
                                };
                                msg.channel_id.send_message(&ctx.http, |m| m.set_embed(embed)).await?;
                                debug!("[{}] ✅ Embed chunk {} sent successfully", request_id, i + 1);
                            }
                        }
                        info!("[{request_id}] ✅ All mention embed response chunks sent successfully");
                    } else {
                        debug!("[{}] 📤 Sending mention embed response ({} chars)", request_id, ai_response.len());
                        let embed = Self::build_persona_embed(p, &ai_response);
                        msg.channel_id.send_message(&ctx.http, |m| m.set_embed(embed)).await?;
                        info!("[{request_id}] ✅ Mention embed response sent successfully");
                    }
                } else {
                    // Plain text fallback (legacy behavior or embeds disabled)
                    if ai_response.len() > 2000 {
                        debug!("[{request_id}] 📄 Response too long, splitting into chunks");
                        let chunks: Vec<&str> = ai_response.as_bytes()
                            .chunks(2000)
                            .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                            .collect();

                        debug!("[{}] 📄 Split response into {} chunks", request_id, chunks.len());

                        // First chunk as threaded reply
                        if let Some(first_chunk) = chunks.first() {
                            if !first_chunk.trim().is_empty() {
                                debug!("[{}] 📤 Sending first chunk as reply ({} chars)", request_id, first_chunk.len());
                                msg.reply(&ctx.http, first_chunk).await?;
                                debug!("[{request_id}] ✅ First chunk sent as reply");
                            }
                        }

                        // Remaining chunks as regular messages in the thread
                        for (i, chunk) in chunks.iter().skip(1).enumerate() {
                            if !chunk.trim().is_empty() {
                                debug!("[{}] 📤 Sending chunk {} of {} ({} chars)",
                                       request_id, i + 2, chunks.len(), chunk.len());
                                msg.channel_id.say(&ctx.http, chunk).await?;
                                debug!("[{}] ✅ Chunk {} sent successfully", request_id, i + 2);
                            }
                        }
                        info!("[{request_id}] ✅ All mention response chunks sent successfully");
                    } else {
                        debug!("[{}] 📤 Sending mention response as reply ({} chars)", request_id, ai_response.len());
                        msg.reply(&ctx.http, &ai_response).await?;
                        info!("[{request_id}] ✅ Mention response sent successfully");
                    }
                }

                // Store assistant response in conversation history (only for channels, not threads)
                if !is_thread {
                    debug!("[{request_id}] 💾 Storing assistant response to conversation history");
                    self.database.store_message(&user_id, &channel_id, "assistant", &ai_response, Some(&user_persona)).await?;
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

    pub async fn handle_slash_command(&self, ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
        let request_id = Uuid::new_v4();
        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string()).unwrap_or_else(|| "DM".to_string());
        
        info!("[{}] 📥 Slash command received | Command: {} | User: {} | Channel: {} | Guild: {}", 
              request_id, command.data.name, user_id, channel_id, guild_id);
        
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

        info!("[{}] 🎯 Processing slash command: {} from user: {}", request_id, command.data.name, user_id);

        match command.data.name.as_str() {
            "ping" => {
                debug!("[{request_id}] 🏓 Handling ping command");
                self.handle_slash_ping_with_id(ctx, command, request_id).await?;
            }
            "help" => {
                debug!("[{request_id}] 📚 Handling help command");
                self.handle_slash_help_with_id(ctx, command, request_id).await?;
            }
            "personas" => {
                debug!("[{request_id}] 🎭 Handling personas command");
                self.handle_slash_personas_with_id(ctx, command, request_id).await?;
            }
            "set_user" => {
                debug!("[{request_id}] ⚙️ Handling set_user command");
                self.handle_set_user(ctx, command, request_id).await?;
            }
            "forget" => {
                debug!("[{request_id}] 🧹 Handling forget command");
                self.handle_slash_forget_with_id(ctx, command, request_id).await?;
            }
            "hey" | "explain" | "simple" | "steps" | "recipe" => {
                debug!("[{}] 🤖 Handling AI command: {}", request_id, command.data.name);
                self.handle_slash_ai_command_with_id(ctx, command, request_id).await?;
            }
            "imagine" => {
                debug!("[{request_id}] 🎨 Handling imagine command");
                self.handle_slash_imagine_with_id(ctx, command, request_id).await?;
            }
            "Analyze Message" | "Explain Message" => {
                debug!("[{}] 🔍 Handling context menu message command: {}", request_id, command.data.name);
                self.handle_context_menu_message_with_id(ctx, command, request_id).await?;
            }
            "Analyze User" => {
                debug!("[{request_id}] 👤 Handling context menu user command");
                self.handle_context_menu_user_with_id(ctx, command, request_id).await?;
            }
            // Admin commands
            "set_channel" => {
                debug!("[{request_id}] ⚙️ Handling set_channel command");
                self.handle_set_channel(ctx, command, request_id).await?;
            }
            "set_guild" => {
                debug!("[{request_id}] ⚙️ Handling set_guild command");
                self.handle_set_guild(ctx, command, request_id).await?;
            }
            "settings" => {
                debug!("[{request_id}] ⚙️ Handling settings command");
                self.handle_settings(ctx, command, request_id).await?;
            }
            "admin_role" => {
                debug!("[{request_id}] ⚙️ Handling admin_role command");
                self.handle_admin_role(ctx, command, request_id).await?;
            }
            // Reminder commands
            "remind" => {
                debug!("[{request_id}] ⏰ Handling remind command");
                self.handle_remind(ctx, command, request_id).await?;
            }
            "reminders" => {
                debug!("[{request_id}] 📋 Handling reminders command");
                self.handle_reminders(ctx, command, request_id).await?;
            }
            "introspect" => {
                debug!("[{request_id}] 🔍 Handling introspect command");
                self.handle_introspect(ctx, command, request_id).await?;
            }
            // Utility commands
            "status" => {
                debug!("[{request_id}] 📊 Handling status command");
                self.handle_slash_status(ctx, command, request_id).await?;
            }
            "version" => {
                debug!("[{request_id}] 📦 Handling version command");
                self.handle_slash_version(ctx, command, request_id).await?;
            }
            "uptime" => {
                debug!("[{request_id}] ⏱️ Handling uptime command");
                self.handle_slash_uptime(ctx, command, request_id).await?;
            }
            "commits" => {
                debug!("[{request_id}] 📝 Handling commits command");
                self.handle_slash_commits(ctx, command, request_id).await?;
            }
            // Feature management commands
            "features" => {
                debug!("[{request_id}] 📋 Handling features command");
                self.handle_slash_features(ctx, command, request_id).await?;
            }
            "toggle" => {
                debug!("[{request_id}] 🔀 Handling toggle command");
                self.handle_slash_toggle(ctx, command, request_id).await?;
            }
            "sysinfo" => {
                debug!("[{request_id}] 📊 Handling sysinfo command");
                self.handle_slash_sysinfo(ctx, command, request_id).await?;
            }
            "usage" => {
                debug!("[{request_id}] 💰 Handling usage command");
                self.handle_slash_usage(ctx, command, request_id).await?;
            }
            "dm_stats" => {
                debug!("[{request_id}] 📊 Handling dm_stats command");
                self.handle_slash_dm_stats(ctx, command, request_id).await?;
            }
            "session_history" => {
                debug!("[{request_id}] 📜 Handling session_history command");
                self.handle_slash_session_history(ctx, command, request_id).await?;
            }
            "debate" => {
                debug!("[{request_id}] 🎭 Handling debate command");
                self.handle_slash_debate(ctx, command, request_id).await?;
            }
            "ask" => {
                debug!("[{request_id}] 🎤 Handling ask command");
                self.handle_slash_ask(ctx, command, request_id).await?;
            }
            cmd_name => {
                // Check if this is a plugin command
                if let Some(ref pm) = self.plugin_manager {
                    if let Some(plugin) = pm.config.plugins.iter().find(|p| p.enabled && p.command.name == cmd_name) {
                        debug!("[{}] 🔌 Handling plugin command: {}", request_id, cmd_name);
                        self.handle_plugin_command(ctx, command, plugin.clone(), pm.clone(), request_id).await?;
                    } else {
                        warn!("[{}] ❓ Unknown slash command: {}", request_id, cmd_name);
                        debug!("[{request_id}] 📤 Sending unknown command response to Discord");
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message.content("Unknown command. Use `/help` to see available commands.")
                                    })
                            })
                            .await?;
                        info!("[{request_id}] ✅ Unknown command response sent successfully");
                    }
                } else {
                    warn!("[{}] ❓ Unknown slash command: {}", request_id, cmd_name);
                    debug!("[{request_id}] 📤 Sending unknown command response to Discord");
                    command
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|message| {
                                    message.content("Unknown command. Use `/help` to see available commands.")
                                })
                        })
                        .await?;
                    info!("[{request_id}] ✅ Unknown command response sent successfully");
                }
            }
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

        info!("[{}] 🔌 Processing plugin command: {} | User: {} | Plugin: {}",
              request_id, plugin.command.name, user_id, plugin.name);

        // Check guild_only restriction
        if plugin.security.guild_only && guild_id.is_none() {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content("This command can only be used in a server, not in DMs.")
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        // Check cooldown
        if plugin.security.cooldown_seconds > 0 {
            if !plugin_manager.job_manager.check_cooldown(&user_id, &plugin.name, plugin.security.cooldown_seconds) {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content(format!(
                                    "Please wait before using `{}` again. Cooldown: {} seconds.",
                                    plugin.command.name, plugin.security.cooldown_seconds
                                ))
                                .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
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
                                message.content(format!("Missing required parameter: `{}`.", opt_def.name))
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
        }

        // Check if plugins feature is enabled for this guild
        if let Some(ref gid) = guild_id {
            let enabled = self.database.is_feature_enabled("plugins", None, Some(gid)).await?;
            if !enabled {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("Plugin commands are disabled in this server.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
                return Ok(());
            }
        }

        // Handle virtual plugins (no CLI execution, handled internally)
        if plugin.is_virtual() {
            info!("[{}] 🔧 Handling virtual plugin: {}", request_id, plugin.command.name);
            return self.handle_virtual_plugin(ctx, command, &plugin, &plugin_manager, &params, &user_id, request_id).await;
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

        info!("[{}] ⏳ Deferred response for plugin command: {}", request_id, plugin.command.name);

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
            let is_youtube = params.get("url")
                .map(|u| u.contains("youtube.com") || u.contains("youtu.be"))
                .unwrap_or(false);
            let is_playlist = params.get("url")
                .map(|u| u.contains("playlist?list=") || u.contains("&list="))
                .unwrap_or(false);

            let result = if use_chunking && is_youtube && !is_playlist {
                // Use chunked transcription for YouTube videos (not playlists)
                let url = params.get("url").cloned().unwrap_or_default();
                let video_title = crate::features::plugins::fetch_youtube_title(&url)
                    .await
                    .unwrap_or_else(|| "Video".to_string());

                info!("[{}] 📦 Using chunked transcription for: {}", request_id, video_title);

                plugin_manager.execute_chunked_transcription(
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
                ).await
            } else {
                // Use regular execution
                plugin_manager.execute_plugin(
                    http,
                    plugin.clone(),
                    params,
                    user_id_owned,
                    guild_id_owned,
                    discord_channel_id,
                    interaction_info,
                    is_thread,
                ).await
            };

            match result {
                Ok(job_id) => {
                    info!("[{}] ✅ Plugin job started: {} (job_id: {})", request_id, plugin.name, job_id);
                    // Note: execute_plugin/execute_chunked_transcription handles editing the interaction response
                }
                Err(e) => {
                    error!("[{}] ❌ Plugin execution failed: {} - {}", request_id, plugin.name, e);

                    // Edit the deferred response with error
                    let edit_url = format!(
                        "https://discord.com/api/v10/webhooks/{}/{}/messages/@original",
                        application_id, interaction_token
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
                self.handle_transcribe_cancel(ctx, command, plugin_manager, params, user_id, request_id).await
            }
            "transcribe_status" => {
                self.handle_transcribe_status(ctx, command, plugin_manager, user_id, request_id).await
            }
            _ => {
                // Unknown virtual plugin
                warn!("[{}] ❓ Unknown virtual plugin: {}", request_id, plugin.command.name);
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("This command is not yet implemented.")
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
        use serenity::model::application::interaction::InteractionResponseType;
        use crate::features::plugins::short_job_id;

        info!("[{}] 🛑 Processing transcribe_cancel for user {}", request_id, user_id);

        // Get optional job_id parameter
        let job_id_param = params.get("job_id").cloned();

        // Find the job to cancel
        let job_to_cancel = if let Some(job_id) = job_id_param {
            // User specified a job ID - look for it
            // Try to find by full ID or short ID prefix
            let active_jobs = plugin_manager.job_manager.get_user_active_playlist_jobs(user_id);
            active_jobs.into_iter().find(|j| j.id == job_id || j.id.starts_with(&job_id))
        } else {
            // No job ID specified - get user's most recent active job
            let active_jobs = plugin_manager.job_manager.get_user_active_playlist_jobs(user_id);
            active_jobs.into_iter().next()
        };

        match job_to_cancel {
            Some(job) => {
                let job_id = job.id.clone();
                let job_title = job.playlist_title.clone().unwrap_or_else(|| "Untitled".to_string());

                // Cancel the job
                match plugin_manager.job_manager.cancel_playlist_job(&job_id, user_id).await {
                    Ok(true) => {
                        info!("[{}] ✅ Cancelled playlist job {} for user {}", request_id, job_id, user_id);
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message.content(format!(
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
                                        message.content(format!(
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
                        error!("[{}] ❌ Failed to cancel job {}: {}", request_id, job_id, e);
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|message| {
                                        message.content(format!("❌ Failed to cancel job: {}", e))
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
                                message.content("❌ No active transcription job found to cancel.\n\
                                                 Use `/transcribe_status` to view your jobs.")
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
        use serenity::model::application::interaction::InteractionResponseType;
        use crate::features::plugins::short_job_id;

        info!("[{}] 📊 Processing transcribe_status for user {}", request_id, user_id);

        // Get user's active playlist jobs
        let active_jobs = plugin_manager.job_manager.get_user_active_playlist_jobs(user_id);

        // Get user's regular (single video) jobs
        let all_jobs = plugin_manager.job_manager.get_user_jobs(user_id);
        let active_video_jobs: Vec<_> = all_jobs.into_iter()
            .filter(|j| matches!(j.status, crate::features::plugins::JobStatus::Running | crate::features::plugins::JobStatus::Pending))
            .collect();

        if active_jobs.is_empty() && active_video_jobs.is_empty() {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content("📭 You have no active transcription jobs.\n\
                                            Use `/transcribe <url>` to start a transcription.")
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
                let url = job.params.get("url").map(|u| {
                    if u.len() > 50 { format!("{}...", &u[..47]) } else { u.clone() }
                }).unwrap_or_else(|| "Unknown".to_string());
                let status = match job.status {
                    crate::features::plugins::JobStatus::Running => "🔄 Running",
                    crate::features::plugins::JobStatus::Pending => "⏳ Pending",
                    _ => "❓ Unknown",
                };
                status_lines.push(format!("• `{}` {} - {}", short_job_id(&job.id), url, status));
            }
        }

        status_lines.push("\n*To cancel a job, use `/transcribe_cancel [job_id]`*".to_string());

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(status_lines.join("\n"))
                            .ephemeral(true)
                    })
            })
            .await?;

        Ok(())
    }

    async fn handle_text_command_with_id(&self, ctx: &Context, msg: &Message, request_id: Uuid) -> Result<()> {
        let user_id = msg.author.id.to_string();
        let parts: Vec<&str> = msg.content.split_whitespace().collect();

        if parts.is_empty() {
            debug!("[{request_id}] 🔍 Empty command parts array");
            return Ok(());
        }

        let command = parts[0];
        let args = &parts[1..];

        info!("[{}] 🎯 Processing text command: {} | Args: {} | User: {}",
              request_id, command, args.len(), user_id);

        match command {
            "/help" => {
                debug!("[{request_id}] 📚 Processing help command");
                self.handle_help_command_with_id(ctx, msg, request_id).await?;
            }
            "/personas" => {
                debug!("[{request_id}] 🎭 Processing personas command");
                self.handle_personas_command_with_id(ctx, msg, request_id).await?;
            }
            "/set_persona" => {
                debug!("[{request_id}] ⚙️ Processing set_persona command");
                self.handle_set_persona_command_with_id(ctx, msg, args, request_id).await?;
            }
            "/hey" | "/explain" | "/simple" | "/steps" | "/recipe" => {
                debug!("[{request_id}] 🤖 Processing AI command: {command}");
                self.handle_ai_command_with_id(ctx, msg, command, args, request_id).await?;
            }
            _ => {
                debug!("[{request_id}] ❓ Unknown command: {command}");
                debug!("[{request_id}] 📤 Sending unknown command response to Discord");
                msg.channel_id
                    .say(&ctx.http, "Unknown command. Use `/help` to see available commands.")
                    .await?;
                info!("[{request_id}] ✅ Unknown command response sent successfully");
            }
        }

        Ok(())
    }

    async fn handle_slash_ping(&self, ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
        let user_id = command.user.id.to_string();
        self.database.log_usage(&user_id, "ping", None).await?;
        
        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content("Pong!")
                    })
            })
            .await?;
        Ok(())
    }

    async fn handle_slash_help(&self, ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
        let help_text = r#"**Available Slash Commands:**
`/ping` - Test bot responsiveness
`/help` - Show this help message
`/personas` - List available personas
`/set_user` - Set your personal preferences
`/hey <message>` - Chat with your current persona
`/explain <topic>` - Get an explanation
`/simple <topic>` - Get a simple explanation with analogies
`/steps <task>` - Break something into steps
`/recipe <food>` - Get a recipe for the specified food

**Available Personas:**
- `muppet` - Muppet expert (default)
- `chef` - Cooking expert
- `teacher` - Patient teacher
- `analyst` - Step-by-step analyst

**Interactive Features:**
Use the buttons below for more help or to try custom prompts!"#;

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message
                            .content(help_text)
                            .set_components(MessageComponentHandler::create_help_buttons())
                    })
            })
            .await?;
        Ok(())
    }

    async fn handle_slash_personas(&self, ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
        let personas = self.persona_manager.list_personas();
        let mut response = "**Available Personas:**\n".to_string();
        
        for (name, persona) in personas {
            response.push_str(&format!("• `{}` - {}\n", name, persona.description));
        }
        
        let user_id = command.user.id.to_string();
        let current_persona = self.database.get_user_persona(&user_id).await?;
        response.push_str(&format!("\nYour current persona: `{current_persona}`"));
        response.push_str("\n\n**Quick Switch:**\nUse the dropdown below to change your persona!");
        
        command
            .create_interaction_response(&ctx.http, |response_builder| {
                response_builder
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message
                            .content(response)
                            .set_components(MessageComponentHandler::create_persona_select_menu())
                    })
            })
            .await?;
        Ok(())
    }

    /// Handle /set_user command (unified user settings)
    async fn handle_set_user(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::commands::slash::admin::validate_user_setting;

        let setting = get_string_option(&command.data.options, "setting")
            .ok_or_else(|| anyhow::anyhow!("Missing setting parameter"))?;
        let value = get_string_option(&command.data.options, "value")
            .ok_or_else(|| anyhow::anyhow!("Missing value parameter"))?;

        // Validate setting and value
        let (is_valid, error_msg) = validate_user_setting(&setting, &value);
        if !is_valid {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content(format!("❌ {error_msg}"))
                        })
                })
                .await?;
            return Ok(());
        }

        let user_id = command.user.id.to_string();

        // Apply the setting
        let response_message = match setting.as_str() {
            "persona" => {
                self.database.set_user_persona(&user_id, &value).await?;
                info!("[{request_id}] Set persona for user {user_id} to {value}");
                format!("✅ Your persona has been set to **{value}**")
            }
            _ => {
                format!("❌ Unknown setting: {setting}")
            }
        };

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(response_message)
                    })
            })
            .await?;
        Ok(())
    }

    async fn handle_slash_ai_command_with_id(&self, ctx: &Context, command: &ApplicationCommandInteraction, request_id: Uuid) -> Result<()> {
        let start_time = Instant::now();
        
        debug!("[{}] 🤖 Starting AI slash command processing | Command: {}", request_id, command.data.name);
        
        let option_name = match command.data.name.as_str() {
            "hey" => "message",
            "explain" => "topic",
            "simple" => "topic",
            "steps" => "task",
            "recipe" => "food",
            _ => "message",
        };

        debug!("[{request_id}] 🔍 Extracting option '{option_name}' from command parameters");
        let user_message = get_string_option(&command.data.options, option_name)
            .ok_or_else(|| anyhow::anyhow!("Missing message parameter"))?;

        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();
        debug!("[{}] 👤 Processing for user: {} | Message: '{}'",
               request_id, user_id, user_message.chars().take(100).collect::<String>());

        // Get user's persona with channel override -> user -> guild default cascade
        debug!("[{request_id}] 🔍 Getting user persona from database");
        let user_persona = if let Some(guild_id) = command.guild_id {
            self.database.get_persona_with_channel(&user_id, &guild_id.to_string(), &channel_id).await?
        } else {
            self.database.get_user_persona(&user_id).await?
        };
        debug!("[{request_id}] 🎭 User persona: {user_persona}");

        let modifier = match command.data.name.as_str() {
            "explain" => Some("explain"),
            "simple" => Some("simple"),
            "steps" => Some("steps"),
            "recipe" => Some("recipe"),
            _ => None,
        };

        // Get channel verbosity (only for guild channels)
        let verbosity = if let Some(guild_id) = command.guild_id {
            self.database.get_channel_verbosity(&guild_id.to_string(), &channel_id).await?
        } else {
            "concise".to_string() // Default to concise for DMs
        };

        debug!("[{request_id}] 📝 Building system prompt | Persona: {user_persona} | Modifier: {modifier:?} | Verbosity: {verbosity}");
        let system_prompt = self.persona_manager.get_system_prompt_with_verbosity(&user_persona, modifier, &verbosity);
        debug!("[{}] ✅ System prompt generated | Length: {} chars", request_id, system_prompt.len());

        debug!("[{request_id}] 📊 Logging usage to database");
        self.database.log_usage(&user_id, &command.data.name, Some(&user_persona)).await?;
        debug!("[{request_id}] ✅ Usage logged successfully");

        // Immediately defer the interaction to prevent timeout (required within 3 seconds)
        info!("[{request_id}] ⏰ Deferring Discord interaction response (3s rule)");
        debug!("[{request_id}] 📤 Sending DeferredChannelMessageWithSource to Discord");
        command
            .create_interaction_response(&ctx.http, |response| {
                response.kind(serenity::model::application::interaction::InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .map_err(|e| {
                error!("[{request_id}] ❌ Failed to defer interaction response: {e}");
                anyhow::anyhow!("Failed to defer interaction: {}", e)
            })?;
        info!("[{request_id}] ✅ Interaction deferred successfully");

        // Get AI response and edit the message
        let guild_id_str = command.guild_id.map(|id| id.to_string());
        let channel_id_str = command.channel_id.to_string();
        info!("[{request_id}] 🚀 Calling OpenAI API");
        match self.get_ai_response_with_context(&system_prompt, &user_message, Vec::new(), request_id, Some(&user_id), guild_id_str.as_deref(), Some(&channel_id_str)).await {
            Ok(ai_response) => {
                let processing_time = start_time.elapsed();
                info!("[{}] ✅ OpenAI response received | Processing time: {:?} | Response length: {}",
                      request_id, processing_time, ai_response.len());

                // Check if embeds are enabled for this guild
                let use_embeds = if let Some(gid) = guild_id_str.as_deref() {
                    self.database.get_guild_setting(gid, "response_embeds").await
                        .unwrap_or(None)
                        .map(|v| v != "disabled")
                        .unwrap_or(true) // Default to enabled
                } else {
                    true // DMs always use embeds
                };

                // Get persona for embed styling
                let persona = self.persona_manager.get_persona(&user_persona);

                if use_embeds && persona.is_some() {
                    let p = persona.unwrap();

                    // Embed description limit is 4096
                    if ai_response.len() > 4096 {
                        debug!("[{request_id}] 📄 Response too long for single embed, splitting into chunks");
                        let chunks: Vec<&str> = ai_response.as_bytes()
                            .chunks(4096)
                            .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                            .collect();

                        debug!("[{}] 📄 Split response into {} embed chunks", request_id, chunks.len());

                        if let Some(first_chunk) = chunks.first() {
                            debug!("[{}] 📤 Editing original interaction response with first embed chunk ({} chars)",
                                   request_id, first_chunk.len());
                            let embed = Self::build_persona_embed(p, first_chunk);
                            command
                                .edit_original_interaction_response(&ctx.http, |response| {
                                    response.set_embed(embed)
                                })
                                .await
                                .map_err(|e| {
                                    error!("[{request_id}] ❌ Failed to edit original interaction response: {e}");
                                    anyhow::anyhow!("Failed to edit original response: {}", e)
                                })?;
                            info!("[{request_id}] ✅ Original embed response edited successfully");
                        }

                        // Send remaining chunks as follow-up embeds
                        for (i, chunk) in chunks.iter().skip(1).enumerate() {
                            if !chunk.trim().is_empty() {
                                debug!("[{}] 📤 Sending follow-up embed {} of {} ({} chars)",
                                       request_id, i + 2, chunks.len(), chunk.len());
                                let embed = Self::build_continuation_embed(p, chunk);
                                command
                                    .create_followup_message(&ctx.http, |message| {
                                        message.set_embed(embed)
                                    })
                                    .await
                                    .map_err(|e| {
                                        error!("[{}] ❌ Failed to send follow-up embed {}: {}", request_id, i + 2, e);
                                        anyhow::anyhow!("Failed to send follow-up message: {}", e)
                                    })?;
                                debug!("[{}] ✅ Follow-up embed {} sent successfully", request_id, i + 2);
                            }
                        }
                        info!("[{request_id}] ✅ All embed response chunks sent successfully");
                    } else {
                        debug!("[{}] 📤 Editing original interaction response with embed ({} chars)",
                               request_id, ai_response.len());
                        let embed = Self::build_persona_embed(p, &ai_response);
                        command
                            .edit_original_interaction_response(&ctx.http, |response| {
                                response.set_embed(embed)
                            })
                            .await
                            .map_err(|e| {
                                error!("[{request_id}] ❌ Failed to edit original interaction response: {e}");
                                anyhow::anyhow!("Failed to edit original response: {}", e)
                            })?;
                        info!("[{request_id}] ✅ Embed response edited successfully");
                    }
                } else {
                    // Plain text fallback (legacy behavior or embeds disabled)
                    if ai_response.len() > 2000 {
                        debug!("[{request_id}] 📄 Response too long, splitting into chunks");
                        let chunks: Vec<&str> = ai_response.as_bytes()
                            .chunks(2000)
                            .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                            .collect();

                        debug!("[{}] 📄 Split response into {} chunks", request_id, chunks.len());

                        if let Some(first_chunk) = chunks.first() {
                            debug!("[{}] 📤 Editing original interaction response with first chunk ({} chars)",
                                   request_id, first_chunk.len());
                            command
                                .edit_original_interaction_response(&ctx.http, |response| {
                                    response.content(first_chunk)
                                })
                                .await
                                .map_err(|e| {
                                    error!("[{request_id}] ❌ Failed to edit original interaction response: {e}");
                                    anyhow::anyhow!("Failed to edit original response: {}", e)
                                })?;
                            info!("[{request_id}] ✅ Original interaction response edited successfully");
                        }

                        // Send remaining chunks as follow-up messages
                        for (i, chunk) in chunks.iter().skip(1).enumerate() {
                            if !chunk.trim().is_empty() {
                                debug!("[{}] 📤 Sending follow-up message {} of {} ({} chars)",
                                       request_id, i + 2, chunks.len(), chunk.len());
                                command
                                    .create_followup_message(&ctx.http, |message| {
                                        message.content(chunk)
                                    })
                                    .await
                                    .map_err(|e| {
                                        error!("[{}] ❌ Failed to send follow-up message {}: {}", request_id, i + 2, e);
                                        anyhow::anyhow!("Failed to send follow-up message: {}", e)
                                    })?;
                                debug!("[{}] ✅ Follow-up message {} sent successfully", request_id, i + 2);
                            }
                        }
                        info!("[{request_id}] ✅ All response chunks sent successfully");
                    } else {
                        debug!("[{}] 📤 Editing original interaction response with complete response ({} chars)",
                               request_id, ai_response.len());
                        command
                            .edit_original_interaction_response(&ctx.http, |response| {
                                response.content(&ai_response)
                            })
                            .await
                            .map_err(|e| {
                                error!("[{request_id}] ❌ Failed to edit original interaction response: {e}");
                                anyhow::anyhow!("Failed to edit original response: {}", e)
                            })?;
                        info!("[{request_id}] ✅ Original interaction response edited successfully");
                    }
                }

                let total_time = start_time.elapsed();
                info!("[{request_id}] 🎉 AI command completed successfully | Total time: {total_time:?}");
            }
            Err(e) => {
                let processing_time = start_time.elapsed();
                error!("[{request_id}] ❌ OpenAI API error after {processing_time:?}: {e}");
                
                let error_message = if e.to_string().contains("timed out") {
                    debug!("[{request_id}] ⏱️ Error type: timeout");
                    "⏱️ **Request timed out** - The AI service is taking too long to respond. Please try again with a shorter message or try again later."
                } else if e.to_string().contains("OpenAI API error") {
                    debug!("[{request_id}] 🔧 Error type: OpenAI API error");
                    "🔧 **AI service error** - There's an issue with the AI service. Please try again in a moment."
                } else {
                    debug!("[{request_id}] ❓ Error type: unknown - {e}");
                    "❌ **Error processing request** - Something went wrong. Please try again later."
                };
                
                debug!("[{request_id}] 📤 Sending error message to Discord: '{error_message}'");
                command
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content(error_message)
                    })
                    .await
                    .map_err(|discord_err| {
                        error!("[{request_id}] ❌ Failed to send error message to Discord: {discord_err}");
                        anyhow::anyhow!("Failed to send error response: {}", discord_err)
                    })?;
                info!("[{request_id}] ✅ Error message sent to Discord successfully");
                
                let total_time = start_time.elapsed();
                error!("[{request_id}] 💥 AI command failed | Total time: {total_time:?}");
            }
        }

        Ok(())
    }

    async fn handle_slash_imagine_with_id(&self, ctx: &Context, command: &ApplicationCommandInteraction, request_id: Uuid) -> Result<()> {
        let start_time = Instant::now();
        let user_id = command.user.id.to_string();

        // Check if image_generation feature is enabled for this guild
        let guild_id = command.guild_id.map(|id| id.to_string());
        let guild_id_opt = guild_id.as_deref();
        let image_gen_enabled = if let Some(gid) = guild_id_opt {
            self.database.is_feature_enabled("image_generation", None, Some(gid)).await?
        } else {
            true // Always enabled in DMs
        };

        if !image_gen_enabled {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| {
                            msg.content("❌ Image generation is disabled on this server.")
                        })
                })
                .await?;
            return Ok(());
        }

        debug!("[{request_id}] 🎨 Starting image generation | Command: imagine");

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

        info!("[{}] 🎨 Generating image | User: {} | Size: {} | Style: {} | Prompt: '{}'",
              request_id, user_id, size.as_str(), style.as_str(),
              prompt.chars().take(100).collect::<String>());

        // Log usage
        self.database.log_usage(&user_id, "imagine", None).await?;

        // Defer the response immediately (DALL-E can take 10-30 seconds)
        info!("[{request_id}] ⏰ Deferring Discord interaction response (DALL-E generation)");
        command
            .create_interaction_response(&ctx.http, |response| {
                response.kind(serenity::model::application::interaction::InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .map_err(|e| {
                error!("[{request_id}] ❌ Failed to defer interaction response: {e}");
                anyhow::anyhow!("Failed to defer interaction: {}", e)
            })?;

        // Generate the image
        let channel_id_str = command.channel_id.to_string();
        match self.image_generator.generate_image(&prompt, size.clone(), style).await {
            Ok(generated_image) => {
                let generation_time = start_time.elapsed();
                info!("[{request_id}] ✅ Image generated | Time: {generation_time:?}");

                // Log DALL-E usage
                self.usage_tracker.log_dalle(
                    size.as_str(),
                    "standard", // DALL-E 3 via this bot uses standard quality
                    1,          // One image per request
                    &user_id,
                    guild_id_opt,
                    Some(&channel_id_str),
                );

                // Download the image
                match self.image_generator.download_image(&generated_image.url).await {
                    Ok(image_bytes) => {
                        debug!("[{}] 📥 Image downloaded | Size: {} bytes", request_id, image_bytes.len());

                        // Build the response message
                        let mut response_text = format!("🎨 **Generated Image**\n> {prompt}");
                        if let Some(revised) = &generated_image.revised_prompt {
                            if revised != &prompt {
                                response_text.push_str(&format!("\n\n*DALL-E revised prompt:* _{revised}_"));
                            }
                        }

                        // Edit the deferred response to show we're sending the image
                        command
                            .edit_original_interaction_response(&ctx.http, |response| {
                                response.content(&response_text)
                            })
                            .await
                            .map_err(|e| {
                                error!("[{request_id}] ❌ Failed to edit interaction response: {e}");
                                anyhow::anyhow!("Failed to edit response: {}", e)
                            })?;

                        // Send the image as a followup message with attachment
                        command
                            .create_followup_message(&ctx.http, |message| {
                                message.add_file(serenity::model::channel::AttachmentType::Bytes {
                                    data: std::borrow::Cow::Owned(image_bytes),
                                    filename: "generated_image.png".to_string(),
                                })
                            })
                            .await
                            .map_err(|e| {
                                error!("[{request_id}] ❌ Failed to send image attachment: {e}");
                                anyhow::anyhow!("Failed to send image: {}", e)
                            })?;

                        let total_time = start_time.elapsed();
                        info!("[{request_id}] ✅ Image sent successfully | Total time: {total_time:?}");
                    }
                    Err(e) => {
                        error!("[{request_id}] ❌ Failed to download image: {e}");
                        command
                            .edit_original_interaction_response(&ctx.http, |response| {
                                response.content("❌ **Error** - Failed to download the generated image. Please try again.")
                            })
                            .await?;
                    }
                }
            }
            Err(e) => {
                let processing_time = start_time.elapsed();
                error!("[{request_id}] ❌ DALL-E error after {processing_time:?}: {e}");

                let error_message = if e.to_string().contains("content_policy") || e.to_string().contains("safety") {
                    "🚫 **Content Policy Violation** - Your prompt was rejected by DALL-E's safety system. Please try a different prompt."
                } else if e.to_string().contains("rate") || e.to_string().contains("limit") {
                    "⏱️ **Rate Limited** - Too many image requests. Please wait a moment and try again."
                } else if e.to_string().contains("billing") || e.to_string().contains("quota") {
                    "💳 **Quota Exceeded** - The image generation quota has been reached. Please try again later."
                } else {
                    "❌ **Error** - Failed to generate image. Please try again with a different prompt."
                };

                command
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content(error_message)
                    })
                    .await?;
            }
        }

        Ok(())
    }

    // Placeholder methods with basic logging - can be enhanced later
    async fn handle_slash_ping_with_id(&self, ctx: &Context, command: &ApplicationCommandInteraction, request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] 🏓 Processing ping slash command");
        self.handle_slash_ping(ctx, command).await
    }

    async fn handle_slash_help_with_id(&self, ctx: &Context, command: &ApplicationCommandInteraction, request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] 📚 Processing help slash command");
        self.handle_slash_help(ctx, command).await
    }

    async fn handle_slash_personas_with_id(&self, ctx: &Context, command: &ApplicationCommandInteraction, request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] 🎭 Processing personas slash command");
        self.handle_slash_personas(ctx, command).await
    }

    async fn handle_slash_forget_with_id(&self, ctx: &Context, command: &ApplicationCommandInteraction, request_id: Uuid) -> Result<()> {
        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();

        debug!("[{request_id}] 🧹 Processing forget command for user: {user_id} in channel: {channel_id}");

        // Clear conversation history
        info!("[{request_id}] 🗑️ Clearing conversation history");
        self.database.clear_conversation_history(&user_id, &channel_id).await?;
        info!("[{request_id}] ✅ Conversation history cleared successfully");

        // Send confirmation response
        debug!("[{request_id}] 📤 Sending confirmation to Discord");
        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content("🧹 Your conversation history has been cleared! I'll start fresh from now on.")
                    })
            })
            .await?;

        info!("[{request_id}] ✅ Forget command completed successfully");
        Ok(())
    }

    async fn handle_context_menu_message_with_id(&self, ctx: &Context, command: &ApplicationCommandInteraction, request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] 🔍 Processing context menu message command");
        self.handle_context_menu_message(ctx, command).await
    }

    async fn handle_context_menu_user_with_id(&self, ctx: &Context, command: &ApplicationCommandInteraction, request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] 👤 Processing context menu user command");
        self.handle_context_menu_user(ctx, command).await
    }

    async fn handle_help_command_with_id(&self, ctx: &Context, msg: &Message, request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] 📚 Processing help text command");
        self.handle_help_command(ctx, msg).await
    }

    async fn handle_personas_command_with_id(&self, ctx: &Context, msg: &Message, request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] 🎭 Processing personas text command");
        self.handle_personas_command(ctx, msg).await
    }

    async fn handle_set_persona_command_with_id(&self, ctx: &Context, msg: &Message, args: &[&str], request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] ⚙️ Processing set_persona text command");
        self.handle_set_persona_command(ctx, msg, args).await
    }

    async fn handle_ai_command_with_id(&self, ctx: &Context, msg: &Message, command: &str, args: &[&str], request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] 🤖 Processing AI text command: {command}");
        self.handle_ai_command(ctx, msg, command, args).await
    }

    async fn handle_context_menu_message(&self, ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
        let user_id = command.user.id.to_string();
        
        // Get the message data from the interaction
        // For now, we'll use a placeholder since resolved data structure varies by version
        let message_content = "Message content will be analyzed".to_string();

        let user_persona = self.database.get_user_persona(&user_id).await?;
        
        let system_prompt = match command.data.name.as_str() {
            "Analyze Message" => {
                self.persona_manager.get_system_prompt(&user_persona, Some("steps"))
            }
            "Explain Message" => {
                self.persona_manager.get_system_prompt(&user_persona, Some("explain"))
            }
            _ => self.persona_manager.get_system_prompt(&user_persona, None)
        };

        let prompt = format!("Please analyze this message: \"{message_content}\"");
        
        self.database.log_usage(&user_id, &command.data.name, Some(&user_persona)).await?;

        // Immediately defer the interaction to prevent timeout
        command
            .create_interaction_response(&ctx.http, |response| {
                response.kind(serenity::model::application::interaction::InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        // Get AI response and edit the message
        match self.get_ai_response(&system_prompt, &prompt).await {
            Ok(ai_response) => {
                let response_text = format!("📝 **{}:**\n{}", command.data.name, ai_response);
                command
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content(&response_text)
                    })
                    .await?;
            }
            Err(e) => {
                error!("AI response error in context menu: {e}");
                let error_message = if e.to_string().contains("timed out") {
                    "⏱️ **Analysis timed out** - The AI service is taking too long. Please try again."
                } else {
                    "❌ **Error analyzing message** - Something went wrong. Please try again later."
                };
                
                command
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content(error_message)
                    })
                    .await?;
            }
        }

        Ok(())
    }

    async fn handle_context_menu_user(&self, ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
        let user_id = command.user.id.to_string();
        
        // Get the user data from the interaction
        // For now, we'll use a placeholder since resolved data structure varies by version
        let target_user = "Discord User".to_string();

        let user_persona = self.database.get_user_persona(&user_id).await?;
        let system_prompt = self.persona_manager.get_system_prompt(&user_persona, Some("explain"));
        
        let prompt = format!("Please provide general information about Discord users and their roles in communities. The user being analyzed is: {target_user}");
        
        self.database.log_usage(&user_id, "analyze_user", Some(&user_persona)).await?;

        // Immediately defer the interaction to prevent timeout
        command
            .create_interaction_response(&ctx.http, |response| {
                response.kind(serenity::model::application::interaction::InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        // Get AI response and edit the message
        match self.get_ai_response(&system_prompt, &prompt).await {
            Ok(ai_response) => {
                let response_text = format!("👤 **User Analysis:**\n{ai_response}");
                command
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content(&response_text)
                    })
                    .await?;
            }
            Err(e) => {
                error!("AI response error in user context menu: {e}");
                let error_message = if e.to_string().contains("timed out") {
                    "⏱️ **Analysis timed out** - The AI service is taking too long. Please try again."
                } else {
                    "❌ **Error analyzing user** - Something went wrong. Please try again later."
                };
                
                command
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content(error_message)
                    })
                    .await?;
            }
        }

        Ok(())
    }

    async fn handle_help_command(&self, ctx: &Context, msg: &Message) -> Result<()> {
        let help_text = r#"**Available Commands:**
`!ping` - Test bot responsiveness
`/help` - Show this help message
`/personas` - List available personas
`/set_user setting:persona value:<name>` - Set your default persona
`/hey <message>` - Chat with your current persona
`/explain <message>` - Get an explanation
`/simple <message>` - Get a simple explanation with analogies
`/steps <message>` - Break something into steps
`/recipe <food>` - Get a recipe for the specified food

**Available Personas:**
- `muppet` - Muppet expert (default)
- `chef` - Cooking expert
- `teacher` - Patient teacher
- `analyst` - Step-by-step analyst"#;

        msg.channel_id.say(&ctx.http, help_text).await?;
        Ok(())
    }

    async fn handle_personas_command(&self, ctx: &Context, msg: &Message) -> Result<()> {
        let personas = self.persona_manager.list_personas();
        let mut response = "**Available Personas:**\n".to_string();
        
        for (name, persona) in personas {
            response.push_str(&format!("• `{}` - {}\n", name, persona.description));
        }
        
        let user_id = msg.author.id.to_string();
        let current_persona = self.database.get_user_persona(&user_id).await?;
        response.push_str(&format!("\nYour current persona: `{current_persona}`"));
        
        msg.channel_id.say(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_set_persona_command(&self, ctx: &Context, msg: &Message, args: &[&str]) -> Result<()> {
        if args.is_empty() {
            msg.channel_id
                .say(&ctx.http, "Please specify a persona. Use `/personas` to see available options.")
                .await?;
            return Ok(());
        }

        let persona_name = args[0];
        if self.persona_manager.get_persona(persona_name).is_none() {
            msg.channel_id
                .say(&ctx.http, "Invalid persona. Use `/personas` to see available options.")
                .await?;
            return Ok(());
        }

        let user_id = msg.author.id.to_string();
        self.database.set_user_persona(&user_id, persona_name).await?;
        
        msg.channel_id
            .say(&ctx.http, &format!("Your persona has been set to: `{persona_name}`"))
            .await?;
        Ok(())
    }

    async fn handle_ai_command(&self, ctx: &Context, msg: &Message, command: &str, args: &[&str]) -> Result<()> {
        if args.is_empty() {
            msg.channel_id
                .say(&ctx.http, "Please provide a message to process.")
                .await?;
            return Ok(());
        }

        let user_id = msg.author.id.to_string();
        let user_persona = self.database.get_user_persona(&user_id).await?;
        
        let modifier = match command {
            "/explain" => Some("explain"),
            "/simple" => Some("simple"),
            "/steps" => Some("steps"),
            "/recipe" => Some("recipe"),
            _ => None,
        };

        let system_prompt = self.persona_manager.get_system_prompt(&user_persona, modifier);
        let user_message = args.join(" ");

        self.database.log_usage(&user_id, command, Some(&user_persona)).await?;

        match self.get_ai_response(&system_prompt, &user_message).await {
            Ok(response) => {
                if response.len() > 2000 {
                    let chunks: Vec<&str> = response.as_bytes()
                        .chunks(2000)
                        .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                        .collect();
                    
                    for chunk in chunks {
                        if !chunk.trim().is_empty() {
                            msg.channel_id.say(&ctx.http, chunk).await?;
                        }
                    }
                } else {
                    msg.channel_id.say(&ctx.http, &response).await?;
                }
            }
            Err(e) => {
                error!("OpenAI API error: {e}");
                let error_message = if e.to_string().contains("timed out") {
                    "⏱️ **Request timed out** - The AI service is taking too long to respond. Please try again with a shorter message or try again later."
                } else if e.to_string().contains("OpenAI API error") {
                    "🔧 **AI service error** - There's an issue with the AI service. Please try again in a moment."
                } else {
                    "❌ **Error processing request** - Something went wrong. Please try again later."
                };
                
                msg.channel_id.say(&ctx.http, error_message).await?;
            }
        }

        Ok(())
    }

    pub async fn get_ai_response(&self, system_prompt: &str, user_message: &str) -> Result<String> {
        self.get_ai_response_with_context(system_prompt, user_message, Vec::new(), Uuid::new_v4(), None, None, None).await
    }

    pub async fn get_ai_response_with_id(&self, system_prompt: &str, user_message: &str, conversation_history: Vec<(String, String)>, request_id: Uuid) -> Result<String> {
        self.get_ai_response_with_context(system_prompt, user_message, conversation_history, request_id, None, None, None).await
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

        info!("[{}] 🤖 Starting OpenAI API request | Model: {} | History messages: {}", request_id, self.openai_model, conversation_history.len());
        debug!("[{}] 📝 System prompt length: {} chars | User message length: {} chars",
               request_id, system_prompt.len(), user_message.len());
        debug!("[{}] 📝 User message preview: '{}'",
               request_id, user_message.chars().take(100).collect::<String>());

        debug!("[{request_id}] 🔨 Building OpenAI message objects");
        let mut messages = vec![
            ChatCompletionMessage {
                role: ChatCompletionMessageRole::System,
                content: Some(system_prompt.to_string()),
                name: None,
                function_call: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];

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

        debug!("[{}] ✅ OpenAI message objects built successfully | Message count: {}", request_id, messages.len());

        // Add timeout to the OpenAI API call (45 seconds)
        debug!("[{request_id}] 🚀 Initiating OpenAI API call with 45-second timeout");
        let chat_completion_future = ChatCompletion::builder(&self.openai_model, messages)
            .create();
        
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
            debug!("[{request_id}] 📊 Token usage - Prompt: {}, Completion: {}, Total: {}",
                   usage.prompt_tokens, usage.completion_tokens, usage.total_tokens);
            self.usage_tracker.log_chat(
                &self.openai_model,
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens,
                uid,
                guild_id,
                channel_id,
                Some(&request_id.to_string()),
            );
        }

        debug!("[{request_id}] 🔍 Parsing OpenAI API response");
        debug!("[{}] 📊 Response choices count: {}", request_id, chat_completion.choices.len());

        let response = chat_completion
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .ok_or_else(|| {
                error!("[{request_id}] ❌ No content in OpenAI response");
                anyhow::anyhow!("No response from OpenAI")
            })?;

        let trimmed_response = response.trim().to_string();
        info!("[{}] ✅ OpenAI response processed | Length: {} chars | First 100 chars: '{}'",
              request_id, trimmed_response.len(),
              trimmed_response.chars().take(100).collect::<String>());

        Ok(trimmed_response)
    }

    /// Handle audio attachments, returns true if any audio was processed
    async fn handle_audio_attachments(&self, ctx: &Context, msg: &Message, guild_id_opt: Option<&str>) -> Result<bool> {
        let user_id = msg.author.id.to_string();
        let mut audio_processed = false;

        // Get output mode setting (transcription_only or with_commentary)
        let output_mode = if let Some(gid) = guild_id_opt {
            self.database.get_guild_setting(gid, "audio_transcription_output").await?
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
                        );

                        if transcription.trim().is_empty() {
                            msg.channel_id
                                .say(&ctx.http, "I couldn't hear anything in that audio file.")
                                .await?;
                        } else {
                            let response = format!("📝 **Transcription:**\n{transcription}");

                            if response.len() > 2000 {
                                let chunks: Vec<&str> = response.as_bytes()
                                    .chunks(2000)
                                    .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                                    .collect();

                                for chunk in chunks {
                                    if !chunk.trim().is_empty() {
                                        msg.channel_id.say(&ctx.http, chunk).await?;
                                    }
                                }
                            } else {
                                msg.channel_id.say(&ctx.http, &response).await?;
                            }

                            // Only generate AI commentary if output mode is "with_commentary"
                            if output_mode == "with_commentary" && !msg.content.trim().is_empty() {
                                let user_persona = self.database.get_user_persona(&user_id).await?;
                                let system_prompt = self.persona_manager.get_system_prompt(&user_persona, None);
                                let combined_message = format!("Based on this transcription: '{}', {}", transcription, msg.content);

                                match self.get_ai_response(&system_prompt, &combined_message).await {
                                    Ok(ai_response) => {
                                        msg.channel_id.say(&ctx.http, &ai_response).await?;
                                    }
                                    Err(e) => {
                                        error!("AI response error: {e}");
                                    }
                                }
                            }
                        }

                        self.database.log_usage(&user_id, "audio_transcription", None).await?;
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
        audio_extensions.iter().any(|ext| filename_lower.ends_with(ext))
    }

    /// Check if an attachment is a text-based file that can be read
    fn is_text_attachment(&self, filename: &str) -> bool {
        let text_extensions = [
            // Plain text
            ".txt", ".md", ".markdown",
            // Data formats
            ".json", ".xml", ".yaml", ".yml", ".toml", ".csv",
            // Config files
            ".ini", ".cfg", ".conf", ".env",
            // Code files
            ".rs", ".py", ".js", ".ts", ".jsx", ".tsx", ".html", ".css",
            ".sh", ".bat", ".ps1", ".sql", ".rb", ".go", ".java", ".c",
            ".cpp", ".h", ".hpp", ".cs", ".php", ".swift", ".kt",
            // Log files
            ".log",
        ];

        let filename_lower = filename.to_lowercase();
        text_extensions.iter().any(|ext| filename_lower.ends_with(ext))
    }

    /// Read a text attachment and return (filename, content) if successful
    /// Returns None if the file couldn't be read, or a truncation message if too large
    async fn read_text_attachment(&self, attachment: &serenity::model::channel::Attachment) -> Result<Option<(String, String)>> {
        const MAX_TEXT_SIZE: u64 = 100_000; // ~100KB limit

        // Check file size first
        if attachment.size > MAX_TEXT_SIZE {
            return Ok(Some((
                attachment.filename.clone(),
                format!("[File too large: {} bytes, max {} bytes]", attachment.size, MAX_TEXT_SIZE)
            )));
        }

        // Download file content
        let client = reqwest::Client::new();
        let response = match client.get(&attachment.url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("Failed to download text attachment {}: {}", attachment.filename, e);
                return Ok(None);
            }
        };

        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to read text attachment bytes {}: {}", attachment.filename, e);
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
        let messages = channel_id.messages(&ctx.http, |builder: &mut GetMessages| {
            builder.limit(limit as u64)
        }).await?;

        let mut attachments = Vec::new();

        for message in messages.iter() {
            for attachment in &message.attachments {
                if self.is_text_attachment(&attachment.filename) {
                    debug!("[{request_id}] 📄 Found text attachment: {}", attachment.filename);
                    if let Ok(Some((filename, content))) = self.read_text_attachment(attachment).await {
                        attachments.push((filename, content));
                    }
                }
            }
        }

        debug!("[{request_id}] 📎 Found {} text attachments in thread", attachments.len());
        Ok(attachments)
    }

    /// Check if a message seems to be asking about uploaded files
    fn seems_like_file_question(&self, message: &str) -> bool {
        let lower = message.to_lowercase();
        let file_keywords = [
            "file", "transcript", "attachment", "uploaded", "document",
            "what's in", "what is in", "read the", "show me the",
            "contents of", "the .txt", "the .md", "the .json", "the .log",
            "summary of", "summarize the", "analyze the", "in the file",
        ];
        file_keywords.iter().any(|kw| lower.contains(kw))
    }

    /// Read text attachments from current message and format them for AI context
    async fn get_text_attachments_context(&self, msg: &Message, request_id: Uuid) -> Vec<(String, String)> {
        let mut attachments = Vec::new();

        for attachment in &msg.attachments {
            if self.is_text_attachment(&attachment.filename) {
                debug!("[{request_id}] 📄 Reading text attachment from message: {}", attachment.filename);
                match self.read_text_attachment(attachment).await {
                    Ok(Some((filename, content))) => {
                        attachments.push((filename, content));
                    }
                    Ok(None) => {
                        debug!("[{request_id}] ⚠️ Could not read attachment: {}", attachment.filename);
                    }
                    Err(e) => {
                        warn!("[{request_id}] ❌ Error reading attachment {}: {}", attachment.filename, e);
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
                "[Attached file: {}]\n```\n{}\n```\n\n",
                filename, content
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
            let sensitivity = self.database.get_guild_setting(gid, "conflict_sensitivity").await?
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
            self.database.get_guild_setting(gid, "mediation_cooldown").await?
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(5) // Default 5 minutes
        } else {
            5
        };

        // Get the timestamp of the last mediation to avoid re-analyzing same messages
        let last_mediation_ts = self.database.get_last_mediation_timestamp(channel_id).await?;

        // Get recent messages, optionally filtering to only new messages since last mediation
        let recent_messages = if let Some(last_ts) = last_mediation_ts {
            info!("🔍 Getting messages since last mediation at timestamp {last_ts}");
            self.database.get_recent_channel_messages_since(channel_id, last_ts, 10).await?
        } else {
            info!("🔍 No previous mediation found, getting all recent messages");
            self.database.get_recent_channel_messages(channel_id, 10).await?
        };

        info!("🔍 Conflict check: Found {} recent messages in channel {} (after last mediation)",
              recent_messages.len(), channel_id);

        if recent_messages.is_empty() {
            info!("⏭️ Skipping conflict detection: No messages found");
            return Ok(());
        }

        // Log message samples for debugging
        let unique_users: std::collections::HashSet<_> = recent_messages.iter()
            .map(|(user_id, _, _)| user_id.clone())
            .collect();
        info!("👥 Messages from {} unique users", unique_users.len());

        for (i, (user_id, content, timestamp)) in recent_messages.iter().take(3).enumerate() {
            debug!("  Message {i}: User={user_id} | Content='{content}' | Time={timestamp}");
        }

        // Detect conflicts in recent messages
        let (is_conflict, confidence, conflict_type) =
            self.conflict_detector.detect_heated_argument(&recent_messages, 120);

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
                    info!("⏸️ Mediation on cooldown for channel {} ({}s remaining)",
                          channel_id, cooldown_secs - (now - last_ts));
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
            let conflict_id = self.database.record_conflict_detection(
                channel_id,
                guild_id,
                &participants_json,
                &conflict_type,
                confidence,
                &msg.id.to_string(),
            ).await?;

            // Generate context-aware mediation response using OpenAI
            info!("🤖 Generating context-aware mediation response with OpenAI...");
            let mediation_text = match self.generate_mediation_response(&recent_messages, &conflict_type, confidence, guild_id, channel_id).await {
                Ok(response) => {
                    info!("✅ OpenAI mediation response generated successfully");
                    response
                },
                Err(e) => {
                    warn!("⚠️ Failed to generate AI mediation response: {e}. Using fallback.");
                    self.conflict_mediator.get_mediation_response(&conflict_type, confidence)
                }
            };

            // Send mediation message as Obi-Wan with proper error handling
            match msg.channel_id.say(&ctx.http, &mediation_text).await {
                Ok(mediation_msg) => {
                    info!("☮️ Mediation sent successfully in channel {channel_id} | Message: {mediation_text}");

                    // Record the intervention
                    self.conflict_mediator.record_intervention(channel_id);

                    // Record in database
                    self.database.mark_mediation_triggered(conflict_id, &mediation_msg.id.to_string()).await?;
                    self.database.record_mediation(conflict_id, channel_id, &mediation_text).await?;
                },
                Err(e) => {
                    warn!("⚠️ Failed to send mediation message to Discord: {e}. Recording intervention to prevent spam.");

                    // Still record the intervention to prevent repeated mediation attempts
                    self.conflict_mediator.record_intervention(channel_id);

                    // Try to record in database with no message ID
                    if let Err(db_err) = self.database.record_mediation(conflict_id, channel_id, &mediation_text).await {
                        warn!("⚠️ Failed to record mediation in database: {db_err}");
                    }
                }
            }

            // Update user interaction patterns
            if participants.len() == 2 {
                let user_a = &participants[0];
                let user_b = &participants[1];
                self.database.update_user_interaction_pattern(user_a, user_b, channel_id, true).await?;
            }
        }

        Ok(())
    }

    // ==================== Admin Command Handlers ====================

    /// Handle /set_channel command (unified channel settings)
    async fn handle_set_channel(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::commands::slash::admin::validate_channel_setting;

        let guild_id = match command.guild_id {
            Some(id) => id.to_string(),
            None => {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("❌ This command can only be used in a server.")
                            })
                    })
                    .await?;
                return Ok(());
            }
        };

        let setting = get_string_option(&command.data.options, "setting")
            .ok_or_else(|| anyhow::anyhow!("Missing setting parameter"))?;
        let value = get_string_option(&command.data.options, "value")
            .ok_or_else(|| anyhow::anyhow!("Missing value parameter"))?;

        // Validate setting and value
        let (is_valid, error_msg) = validate_channel_setting(&setting, &value);
        if !is_valid {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content(format!("❌ {error_msg}"))
                        })
                })
                .await?;
            return Ok(());
        }

        // Get target channel (default to current channel)
        let target_channel_id = get_channel_option(&command.data.options, "channel")
            .map(|id| id.to_string())
            .unwrap_or_else(|| command.channel_id.to_string());

        // Apply the setting
        let response_message = match setting.as_str() {
            "verbosity" => {
                self.database.set_channel_verbosity(&guild_id, &target_channel_id, &value).await?;
                info!("[{request_id}] Set verbosity for channel {target_channel_id} to {value}");
                format!("✅ Verbosity for <#{target_channel_id}> set to **{value}**")
            }
            "persona" => {
                if value == "clear" {
                    self.database.set_channel_persona(&guild_id, &target_channel_id, None).await?;
                    info!("[{request_id}] Cleared persona override for channel {target_channel_id}");
                    format!("✅ Persona override cleared for <#{target_channel_id}>. Users will use their own personas.")
                } else {
                    self.database.set_channel_persona(&guild_id, &target_channel_id, Some(&value)).await?;
                    info!("[{request_id}] Set persona for channel {target_channel_id} to {value}");
                    format!("✅ Persona for <#{target_channel_id}> set to **{value}**. All users in this channel will use this persona.")
                }
            }
            "conflict_mediation" => {
                let enabled = value == "enabled";
                self.database.set_channel_conflict_enabled(&guild_id, &target_channel_id, enabled).await?;
                info!("[{request_id}] Set conflict_mediation for channel {target_channel_id} to {value}");
                let status = if enabled { "Enabled ✅" } else { "Disabled ❌" };
                format!("✅ Conflict mediation for <#{target_channel_id}> is now **{status}**")
            }
            _ => {
                format!("❌ Unknown setting: {setting}")
            }
        };

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(response_message)
                    })
            })
            .await?;

        Ok(())
    }

    /// Handle /set_guild command
    async fn handle_set_guild(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::commands::slash::admin::validate_guild_setting;

        let guild_id = match command.guild_id {
            Some(id) => id.to_string(),
            None => {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("❌ This command can only be used in a server.")
                            })
                    })
                    .await?;
                return Ok(());
            }
        };

        let setting = get_string_option(&command.data.options, "setting")
            .ok_or_else(|| anyhow::anyhow!("Missing setting parameter"))?;

        let value = get_string_option(&command.data.options, "value")
            .ok_or_else(|| anyhow::anyhow!("Missing value parameter"))?;

        // Validate setting and value using shared validation
        let (is_valid, error_msg) = validate_guild_setting(&setting, &value);

        if !is_valid {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content(format!("❌ {error_msg}"))
                        })
                })
                .await?;
            return Ok(());
        }

        // Check if this is a global bot setting or a guild setting
        let is_global_setting = matches!(
            setting.as_str(),
            "startup_notification" | "startup_notify_owner_id" | "startup_notify_channel_id"
            | "startup_dm_commit_count" | "startup_channel_commit_count"
        );

        if is_global_setting {
            info!("[{request_id}] Setting global bot setting '{setting}' to '{value}'");
            self.database.set_bot_setting(&setting, &value).await?;
        } else {
            info!("[{request_id}] Setting guild {guild_id} setting '{setting}' to '{value}'");
            self.database.set_guild_setting(&guild_id, &setting, &value).await?;
        }

        let scope = if is_global_setting { "Global" } else { "Guild" };
        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(format!(
                            "✅ {scope} setting `{setting}` set to **{value}**"
                        ))
                    })
            })
            .await?;

        Ok(())
    }

    /// Handle /settings command
    async fn handle_settings(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let guild_id = match command.guild_id {
            Some(id) => id.to_string(),
            None => {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("❌ This command can only be used in a server.")
                            })
                    })
                    .await?;
                return Ok(());
            }
        };

        let channel_id = command.channel_id.to_string();

        // Get channel settings (verbosity, conflict_enabled, persona)
        let (channel_verbosity, conflict_enabled, channel_persona) = self.database.get_channel_settings(&guild_id, &channel_id).await?;
        let channel_persona_display = channel_persona
            .map(|p| format!("`{p}` (override)"))
            .unwrap_or_else(|| "Not set (uses user/guild default)".to_string());

        // Get guild settings with defaults
        let guild_default_verbosity = self.database.get_guild_setting(&guild_id, "default_verbosity").await?
            .unwrap_or_else(|| "concise".to_string());
        let guild_default_persona = self.database.get_guild_setting(&guild_id, "default_persona").await?
            .unwrap_or_else(|| "obi".to_string());
        let guild_conflict_mediation = self.database.get_guild_setting(&guild_id, "conflict_mediation").await?
            .unwrap_or_else(|| "enabled".to_string());
        let guild_conflict_sensitivity = self.database.get_guild_setting(&guild_id, "conflict_sensitivity").await?
            .unwrap_or_else(|| "medium".to_string());
        let guild_mediation_cooldown = self.database.get_guild_setting(&guild_id, "mediation_cooldown").await?
            .unwrap_or_else(|| "5".to_string());
        let guild_max_context = self.database.get_guild_setting(&guild_id, "max_context_messages").await?
            .unwrap_or_else(|| "40".to_string());
        let guild_audio_transcription = self.database.get_guild_setting(&guild_id, "audio_transcription").await?
            .unwrap_or_else(|| "enabled".to_string());
        let guild_audio_mode = self.database.get_guild_setting(&guild_id, "audio_transcription_mode").await?
            .unwrap_or_else(|| "mention_only".to_string());
        let guild_audio_output = self.database.get_guild_setting(&guild_id, "audio_transcription_output").await?
            .unwrap_or_else(|| "transcription_only".to_string());
        let guild_mention_responses = self.database.get_guild_setting(&guild_id, "mention_responses").await?
            .unwrap_or_else(|| "enabled".to_string());
        let guild_debate_auto_response = self.database.get_guild_setting(&guild_id, "debate_auto_response").await?
            .unwrap_or_else(|| "disabled".to_string());

        // Get bot admin role
        let admin_role = self.database.get_guild_setting(&guild_id, "bot_admin_role").await?;
        let admin_role_display = match admin_role {
            Some(role_id) => format!("<@&{role_id}>"),
            None => "Not set (Discord admins only)".to_string(),
        };

        let settings_text = format!(
            "**Bot Settings**\n\n\
            **Channel Settings** (<#{}>):\n\
            • Verbosity: `{}`\n\
            • Persona: {}\n\
            • Conflict Mediation: {}\n\n\
            **Guild Settings**:\n\
            • Default Verbosity: `{}`\n\
            • Default Persona: `{}`\n\
            • Conflict Mediation: `{}`\n\
            • Conflict Sensitivity: `{}`\n\
            • Mediation Cooldown: `{}` minutes\n\
            • Max Context Messages: `{}`\n\
            • Audio Transcription: `{}`\n\
            • Audio Transcription Mode: `{}`\n\
            • Audio Transcription Output: `{}`\n\
            • Mention Responses: `{}`\n\
            • Debate Auto-Response: `{}`\n\
            • Bot Admin Role: {}\n",
            channel_id,
            channel_verbosity,
            channel_persona_display,
            if conflict_enabled { "Enabled ✅" } else { "Disabled ❌" },
            guild_default_verbosity,
            guild_default_persona,
            guild_conflict_mediation,
            guild_conflict_sensitivity,
            guild_mediation_cooldown,
            guild_max_context,
            guild_audio_transcription,
            guild_audio_mode,
            guild_audio_output,
            guild_mention_responses,
            guild_debate_auto_response,
            admin_role_display
        );

        info!("[{request_id}] Displaying settings for guild {guild_id} channel {channel_id}");

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(&settings_text)
                    })
            })
            .await?;

        Ok(())
    }

    /// Handle /admin_role command
    async fn handle_admin_role(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let guild_id = match command.guild_id {
            Some(id) => id.to_string(),
            None => {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("❌ This command can only be used in a server.")
                            })
                    })
                    .await?;
                return Ok(());
            }
        };

        let role_id = get_role_option(&command.data.options, "role")
            .ok_or_else(|| anyhow::anyhow!("Missing role parameter"))?;

        info!("[{request_id}] Setting bot admin role for guild {guild_id} to {role_id}");

        // Set the bot admin role
        self.database.set_guild_setting(&guild_id, "bot_admin_role", &role_id.to_string()).await?;

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(format!(
                            "✅ Bot Admin role set to <@&{role_id}>. Users with this role can now manage bot settings."
                        ))
                    })
            })
            .await?;

        Ok(())
    }

    /// Parse a time duration string like "30m", "2h", "1d", "1h30m" into seconds
    fn parse_duration(&self, time_str: &str) -> Option<i64> {
        let time_str = time_str.trim().to_lowercase();
        let mut total_seconds: i64 = 0;
        let mut current_number = String::new();

        for c in time_str.chars() {
            if c.is_ascii_digit() {
                current_number.push(c);
            } else if !current_number.is_empty() {
                let value: i64 = current_number.parse().ok()?;
                current_number.clear();

                let seconds = match c {
                    's' => value,
                    'm' => value * 60,
                    'h' => value * 60 * 60,
                    'd' => value * 60 * 60 * 24,
                    'w' => value * 60 * 60 * 24 * 7,
                    _ => return None,
                };
                total_seconds += seconds;
            }
        }

        if total_seconds > 0 {
            Some(total_seconds)
        } else {
            None
        }
    }

    /// Format a duration in seconds into a human-readable string
    fn format_duration(&self, seconds: i64) -> String {
        if seconds < 60 {
            format!("{} second{}", seconds, if seconds == 1 { "" } else { "s" })
        } else if seconds < 3600 {
            let mins = seconds / 60;
            format!("{} minute{}", mins, if mins == 1 { "" } else { "s" })
        } else if seconds < 86400 {
            let hours = seconds / 3600;
            let mins = (seconds % 3600) / 60;
            if mins > 0 {
                format!("{} hour{} {} minute{}", hours, if hours == 1 { "" } else { "s" }, mins, if mins == 1 { "" } else { "s" })
            } else {
                format!("{} hour{}", hours, if hours == 1 { "" } else { "s" })
            }
        } else {
            let days = seconds / 86400;
            let hours = (seconds % 86400) / 3600;
            if hours > 0 {
                format!("{} day{} {} hour{}", days, if days == 1 { "" } else { "s" }, hours, if hours == 1 { "" } else { "s" })
            } else {
                format!("{} day{}", days, if days == 1 { "" } else { "s" })
            }
        }
    }

    /// Handle the /remind command
    async fn handle_remind(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();

        // Check if reminders feature is enabled for this guild
        let guild_id = command.guild_id.map(|id| id.to_string());
        let guild_id_opt = guild_id.as_deref();
        let reminders_enabled = if let Some(gid) = guild_id_opt {
            self.database.is_feature_enabled("reminders", None, Some(gid)).await?
        } else {
            true // Always enabled in DMs
        };

        if !reminders_enabled {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| {
                            msg.content("❌ Reminders are disabled on this server.")
                        })
                })
                .await?;
            return Ok(());
        }

        let time_str = get_string_option(&command.data.options, "time")
            .ok_or_else(|| anyhow::anyhow!("Missing time parameter"))?;
        let message = get_string_option(&command.data.options, "message")
            .ok_or_else(|| anyhow::anyhow!("Missing message parameter"))?;

        // Parse the duration
        let duration_seconds = match self.parse_duration(&time_str) {
            Some(secs) => secs,
            None => {
                command
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|msg| {
                                msg.content("❌ Invalid time format. Use formats like `30m`, `2h`, `1d`, or `1h30m`.")
                            })
                    })
                    .await?;
                return Ok(());
            }
        };

        // Calculate remind_at timestamp
        let remind_at = chrono::Utc::now() + chrono::Duration::seconds(duration_seconds);
        let remind_at_str = remind_at.format("%Y-%m-%d %H:%M:%S").to_string();

        // Store the reminder
        let reminder_id = self.database.add_reminder(&user_id, &channel_id, &message, &remind_at_str).await?;

        info!("[{}] ⏰ Created reminder {} for user {} in {} ({})",
              request_id, reminder_id, user_id, self.format_duration(duration_seconds), remind_at_str);

        // Log usage
        self.database.log_usage(&user_id, "remind", None).await?;

        let duration_display = self.format_duration(duration_seconds);
        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|msg| {
                        msg.content(format!(
                            "⏰ Got it! I'll remind you in **{duration_display}** about:\n> {message}\n\n*Reminder ID: #{reminder_id}*"
                        ))
                    })
            })
            .await?;

        Ok(())
    }

    /// Handle the /reminders command
    async fn handle_reminders(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        // Check if reminders feature is enabled for this guild
        let guild_id = command.guild_id.map(|id| id.to_string());
        let guild_id_opt = guild_id.as_deref();
        let reminders_enabled = if let Some(gid) = guild_id_opt {
            self.database.is_feature_enabled("reminders", None, Some(gid)).await?
        } else {
            true // Always enabled in DMs
        };

        if !reminders_enabled {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| {
                            msg.content("❌ Reminders are disabled on this server.")
                        })
                })
                .await?;
            return Ok(());
        }

        let action = get_string_option(&command.data.options, "action")
            .unwrap_or_else(|| "list".to_string());

        match action.as_str() {
            "cancel" => {
                let reminder_id = get_integer_option(&command.data.options, "id");

                if let Some(id) = reminder_id {
                    let deleted = self.database.delete_reminder(id, &user_id).await?;

                    if deleted {
                        info!("[{request_id}] 🗑️ Deleted reminder {id} for user {user_id}");
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|msg| {
                                        msg.content(format!("✅ Cancelled reminder #{id}."))
                                    })
                            })
                            .await?;
                    } else {
                        command
                            .create_interaction_response(&ctx.http, |response| {
                                response
                                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                    .interaction_response_data(|msg| {
                                        msg.content(format!("❌ Reminder #{id} not found or doesn't belong to you."))
                                    })
                            })
                            .await?;
                    }
                } else {
                    command
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|msg| {
                                    msg.content("❌ Please provide a reminder ID to cancel. Use `/reminders` to see your reminder IDs.")
                                })
                        })
                        .await?;
                }
            }
            _ => {
                // List reminders (default action)
                let reminders = self.database.get_user_reminders(&user_id).await?;

                if reminders.is_empty() {
                    command
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|msg| {
                                    msg.content("📋 You don't have any pending reminders.\n\nUse `/remind <time> <message>` to create one!")
                                })
                        })
                        .await?;
                } else {
                    let mut reminder_list = String::from("📋 **Your Pending Reminders:**\n\n");

                    for (id, _channel_id, text, remind_at) in &reminders {
                        // Parse remind_at to show relative time
                        let remind_time = chrono::NaiveDateTime::parse_from_str(remind_at, "%Y-%m-%d %H:%M:%S")
                            .map(|dt| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc))
                            .ok();

                        let time_display = if let Some(dt) = remind_time {
                            let now = chrono::Utc::now();
                            let diff = dt.signed_duration_since(now);
                            if diff.num_seconds() > 0 {
                                format!("in {}", self.format_duration(diff.num_seconds()))
                            } else {
                                "any moment now".to_string()
                            }
                        } else {
                            remind_at.clone()
                        };

                        reminder_list.push_str(&format!("**#{id}** - {time_display} ({remind_at})\n> {text}\n\n"));
                    }

                    reminder_list.push_str("*Use `/reminders cancel <id>` to cancel a reminder.*");

                    command
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|msg| {
                                    msg.content(&reminder_list)
                                })
                        })
                        .await?;
                }
            }
        }

        self.database.log_usage(&user_id, "reminders", None).await?;
        Ok(())
    }

    /// Handle the /introspect command - let personas explain their own code
    async fn handle_introspect(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        let component = get_string_option(&command.data.options, "component")
            .ok_or_else(|| anyhow::anyhow!("Missing component parameter"))?;

        info!("[{request_id}] 🔍 Introspect requested for component: {component} by user: {user_id}");

        // Defer response - AI generation takes time
        command
            .create_interaction_response(&ctx.http, |response| {
                response.kind(serenity::model::application::interaction::InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        // Get user's persona with channel override -> user -> guild default cascade
        let persona_name = if let Some(gid) = &guild_id {
            self.database.get_persona_with_channel(&user_id, gid, &channel_id).await?
        } else {
            self.database.get_user_persona(&user_id).await?
        };

        // Get the code snippet for this component
        let (component_title, code_snippet) = get_component_snippet(&component);

        // Get persona's system prompt
        let persona = self.persona_manager.get_persona(&persona_name);
        let persona_prompt = persona.map(|p| p.system_prompt.as_str()).unwrap_or("");

        // Build the introspection prompt
        let introspection_prompt = format!(
            "{persona_prompt}\n\n\
            You are now being asked to explain your own implementation. \
            The user wants to understand how you work internally.\n\n\
            Here is actual code from your implementation - {component_title}:\n\n\
            ```rust\n{code_snippet}\n```\n\n\
            Explain this code in your characteristic style and personality. \
            Use metaphors and analogies that fit your character. \
            Make it entertaining and educational. \
            Keep it conversational, not too technical. \
            Aim for 2-3 paragraphs."
        );

        // Call OpenAI
        let chat_completion = ChatCompletion::builder(&self.openai_model, vec![
            ChatCompletionMessage {
                role: ChatCompletionMessageRole::System,
                content: Some(introspection_prompt),
                name: None,
                function_call: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatCompletionMessage {
                role: ChatCompletionMessageRole::User,
                content: Some(format!("Explain how your {component_title} system works, in your own words.")),
                name: None,
                function_call: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ])
        .create()
        .await;

        let channel_id_str = command.channel_id.to_string();
        let response = match chat_completion {
            Ok(completion) => {
                // Log usage if available
                if let Some(usage) = &completion.usage {
                    self.usage_tracker.log_chat(
                        &self.openai_model,
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        usage.total_tokens,
                        &user_id,
                        guild_id.as_deref(),
                        Some(&channel_id_str),
                        Some(&request_id.to_string()),
                    );
                }
                completion
                    .choices
                    .first()
                    .and_then(|choice| choice.message.content.clone())
                    .unwrap_or_else(|| "I seem to be having trouble reflecting on myself right now.".to_string())
            }
            Err(e) => {
                warn!("[{request_id}] ⚠️ OpenAI error during introspection: {e}");
                format!("I encountered an error while attempting to explain my {component} system: {e}")
            }
        };

        // Edit the deferred response
        command
            .edit_original_interaction_response(&ctx.http, |msg| {
                msg.content(format!("## 🔍 Introspection: {component_title}\n\n{response}"))
            })
            .await?;

        self.database.log_usage(&user_id, "introspect", Some(&persona_name)).await?;

        info!("[{request_id}] ✅ Introspection complete for component: {component}");
        Ok(())
    }

    /// Handle the /status slash command
    async fn handle_slash_status(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        let uptime = self.start_time.elapsed();
        let hours = uptime.as_secs() / 3600;
        let minutes = (uptime.as_secs() % 3600) / 60;
        let seconds = uptime.as_secs() % 60;

        let response = format!(
            "**Bot Status**\n\
            ✅ Online and operational\n\
            ⏱️ Uptime: {}h {}m {}s\n\
            📦 Version: {}",
            hours,
            minutes,
            seconds,
            crate::features::get_bot_version()
        );

        command
            .create_interaction_response(&ctx.http, |r| {
                r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(response))
            })
            .await?;

        self.database.log_usage(&user_id, "status", None).await?;
        info!("[{request_id}] ✅ Status command completed");
        Ok(())
    }

    /// Handle the /version slash command
    async fn handle_slash_version(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        let mut output = format!("**Persona Bot v{}**\n\n", crate::features::get_bot_version());
        output.push_str("**Feature Versions:**\n");

        for feature in crate::features::get_features() {
            output.push_str(&format!("• {} v{}\n", feature.name, feature.version));
        }

        command
            .create_interaction_response(&ctx.http, |r| {
                r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(output))
            })
            .await?;

        self.database.log_usage(&user_id, "version", None).await?;
        info!("[{request_id}] ✅ Version command completed");
        Ok(())
    }

    /// Handle the /uptime slash command
    async fn handle_slash_uptime(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        let uptime = self.start_time.elapsed();
        let days = uptime.as_secs() / 86400;
        let hours = (uptime.as_secs() % 86400) / 3600;
        let minutes = (uptime.as_secs() % 3600) / 60;
        let seconds = uptime.as_secs() % 60;

        let response = if days > 0 {
            format!("⏱️ Uptime: {days}d {hours}h {minutes}m {seconds}s")
        } else if hours > 0 {
            format!("⏱️ Uptime: {hours}h {minutes}m {seconds}s")
        } else if minutes > 0 {
            format!("⏱️ Uptime: {minutes}m {seconds}s")
        } else {
            format!("⏱️ Uptime: {seconds}s")
        };

        command
            .create_interaction_response(&ctx.http, |r| {
                r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(response))
            })
            .await?;

        self.database.log_usage(&user_id, "uptime", None).await?;
        info!("[{request_id}] ✅ Uptime command completed");
        Ok(())
    }

    /// Handle the /commits slash command - shows recent git commits
    ///
    /// In guild channels: sends a minimal embed summary and creates a thread with detailed commit info.
    /// In DMs: only sends the minimal embed (threads not supported in DMs).
    async fn handle_slash_commits(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::features::startup::notification::{format_commit_for_thread, get_detailed_commits, get_github_repo_url};
        use serenity::model::application::interaction::InteractionResponseType;
        use serenity::model::channel::ChannelType;

        let user_id = command.user.id.to_string();
        let is_dm = command.guild_id.is_none();

        let count = get_integer_option(&command.data.options, "count")
            .unwrap_or(1) as usize;
        let count = count.clamp(1, 10);

        let commits = get_detailed_commits(count).await;
        let repo_url = get_github_repo_url().await;

        if commits.is_empty() {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|msg| {
                            msg.content("No commit history available.")
                        })
                })
                .await?;
            return Ok(());
        }

        // Build minimal summary for main embed (subjects with linked hashes)
        let mut summary = String::new();
        for commit in &commits {
            let hash_display = if let Some(ref url) = repo_url {
                format!("[`{}`]({}/commit/{})", commit.hash, url, commit.hash)
            } else {
                format!("`{}`", commit.hash)
            };
            summary.push_str(&format!("• **{}** ({})\n", commit.subject, hash_display));
        }

        // Send minimal embed
        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|msg| {
                        msg.embed(|e| {
                            e.title(format!("Recent Commits ({})", commits.len()))
                                .description(&summary)
                                .color(0x57F287)
                        })
                    })
            })
            .await?;

        // Only create thread in guild channels (DMs can't have threads)
        if !is_dm {
            // Get the message we just sent
            if let Ok(msg) = command.get_interaction_response(&ctx.http).await {
                // Create thread from the message
                match command
                    .channel_id
                    .create_public_thread(&ctx.http, msg.id, |t| {
                        t.name("Commit Details")
                            .kind(ChannelType::PublicThread)
                            .auto_archive_duration(60) // 1 hour
                    })
                    .await
                {
                    Ok(thread) => {
                        info!(
                            "[{request_id}] Created thread '{}' for commit details",
                            thread.name()
                        );

                        // Post detailed commits to thread
                        for commit in &commits {
                            let formatted = format_commit_for_thread(commit, repo_url.as_deref());
                            if let Err(e) = thread.say(&ctx.http, &formatted).await {
                                warn!(
                                    "[{request_id}] Failed to post commit {} to thread: {}",
                                    commit.hash, e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        warn!("[{request_id}] Failed to create thread for commit details: {}", e);
                    }
                }
            }
        }

        self.database.log_usage(&user_id, "commits", None).await?;
        info!("[{request_id}] ✅ Commits command completed");
        Ok(())
    }

    /// Handle the /features slash command - shows all features with toggle status
    async fn handle_slash_features(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        // Get feature flags for this guild
        let flags = if let Some(ref gid) = guild_id {
            self.database.get_guild_feature_flags(gid).await.unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };

        let mut output = format!("📦 **Bot Features** (v{})\n\n", crate::features::get_bot_version());
        output.push_str("```\n");
        output.push_str("Feature              Version  Status  Toggleable\n");
        output.push_str("─────────────────────────────────────────────────\n");

        for feature in crate::features::get_features() {
            // Check if feature is enabled (default true if no record)
            let enabled = flags.get(feature.id).copied().unwrap_or(true);
            let status_str = if enabled { "✅ ON " } else { "❌ OFF" };
            let toggle_str = if feature.toggleable { "Yes" } else { "No " };

            output.push_str(&format!(
                "{:<20} {:<8} {}  {}\n",
                feature.name, feature.version, status_str, toggle_str
            ));
        }

        output.push_str("```\n");
        output.push_str("Use `/toggle <feature>` to enable/disable toggleable features.");

        command
            .create_interaction_response(&ctx.http, |r| {
                r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(output))
            })
            .await?;

        self.database.log_usage(&user_id, "features", None).await?;
        info!("[{request_id}] ✅ Features command completed");
        Ok(())
    }

    /// Handle the /toggle slash command - enables/disables toggleable features
    async fn handle_slash_toggle(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        let feature_id = get_string_option(&command.data.options, "feature")
            .ok_or_else(|| anyhow::anyhow!("Missing feature parameter"))?;

        // Verify this is a valid toggleable feature
        let feature = crate::features::get_feature(&feature_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown feature: {}", feature_id))?;

        if !feature.toggleable {
            command
                .create_interaction_response(&ctx.http, |r| {
                    r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.content(format!("❌ **{}** cannot be toggled. It's a core feature.", feature.name))
                        })
                })
                .await?;
            return Ok(());
        }

        // Get current status
        let guild_id_str = guild_id.as_deref().unwrap_or("");
        let current_enabled = self.database.is_feature_enabled(&feature_id, None, Some(guild_id_str)).await?;

        // Toggle it
        let new_enabled = !current_enabled;
        self.database.set_feature_flag(&feature_id, new_enabled, None, Some(guild_id_str)).await?;

        // Record in audit trail
        self.database.record_feature_toggle(
            &feature_id,
            feature.version,
            Some(guild_id_str),
            &user_id,
            new_enabled,
        ).await?;

        let status = if new_enabled { "✅ enabled" } else { "❌ disabled" };
        let response = format!(
            "**{}** has been {}.\n\nFeature: {} v{}",
            feature.name, status, feature.id, feature.version
        );

        command
            .create_interaction_response(&ctx.http, |r| {
                r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|m| m.content(response))
            })
            .await?;

        self.database.log_usage(&user_id, "toggle", None).await?;
        info!("[{request_id}] ✅ Toggle command completed: {feature_id} -> {new_enabled}");
        Ok(())
    }

    /// Handle the /sysinfo slash command - displays system diagnostics and metrics history
    async fn handle_slash_sysinfo(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::features::analytics::system_info::{CurrentMetrics, HistoricalSummary, format_history};

        let user_id = command.user.id.to_string();

        // Get the view option (defaults to "current")
        let view = get_string_option(&command.data.options, "view")
            .unwrap_or_else(|| "current".to_string());

        info!("[{request_id}] 📊 Sysinfo requested: view={view}");

        // Defer response since gathering metrics can take a moment
        command
            .create_interaction_response(&ctx.http, |response| {
                response.kind(serenity::model::application::interaction::InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        let response = match view.as_str() {
            "history_24h" | "history_7d" => {
                let hours = if view == "history_24h" { 24 } else { 168 };
                let period_label = if view == "history_24h" { "24h" } else { "7d" };

                // Fetch historical data
                let db_size_data = self.database.get_metrics_history("db_size_bytes", hours).await?;
                let bot_memory_data = self.database.get_metrics_history("bot_memory_bytes", hours).await?;
                let system_memory_data = self.database.get_metrics_history("system_memory_percent", hours).await?;
                let system_cpu_data = self.database.get_metrics_history("system_cpu_percent", hours).await?;

                // Build summaries
                let db_size = HistoricalSummary::from_data(&db_size_data);
                let bot_memory = HistoricalSummary::from_data(&bot_memory_data);
                let system_memory = HistoricalSummary::from_data(&system_memory_data);
                let system_cpu = HistoricalSummary::from_data(&system_cpu_data);

                format_history(db_size, bot_memory, system_memory, system_cpu, period_label)
            }
            _ => {
                // Default: current system info
                // Create a new System instance and do two CPU refreshes for accuracy
                let mut sys = sysinfo::System::new();
                sys.refresh_cpu_usage();
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                sys.refresh_cpu_usage();
                sys.refresh_memory();

                // Refresh process info for bot memory
                if let Ok(pid) = sysinfo::get_current_pid() {
                    sys.refresh_processes_specifics(
                        sysinfo::ProcessesToUpdate::Some(&[pid]),
                        true,
                        sysinfo::ProcessRefreshKind::new().with_memory()
                    );
                }

                let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "persona.db".to_string());
                let metrics = CurrentMetrics::gather(&sys, &db_path);
                let bot_uptime_secs = self.start_time.elapsed().as_secs();

                metrics.format(bot_uptime_secs)
            }
        };

        // Edit the deferred response
        command
            .edit_original_interaction_response(&ctx.http, |msg| {
                msg.content(response)
            })
            .await?;

        self.database.log_usage(&user_id, "sysinfo", None).await?;
        info!("[{request_id}] ✅ Sysinfo command completed");
        Ok(())
    }

    /// Handle the /usage slash command - displays OpenAI API usage and cost metrics
    async fn handle_slash_usage(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        // Get the scope option (defaults to "personal_today")
        let scope = get_string_option(&command.data.options, "scope")
            .unwrap_or_else(|| "personal_today".to_string());

        info!("[{request_id}] 💰 Usage requested: scope={scope}");

        // Defer response since querying can take a moment
        command
            .create_interaction_response(&ctx.http, |response| {
                response.kind(serenity::model::application::interaction::InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        let response = match scope.as_str() {
            "personal_today" => {
                let stats = self.database.get_user_usage_stats(&user_id, 1).await?;
                Self::format_usage_stats("Your Usage Today", &stats, None)
            }
            "personal_7d" => {
                let stats = self.database.get_user_usage_stats(&user_id, 7).await?;
                Self::format_usage_stats("Your Usage (7 days)", &stats, None)
            }
            "server_today" => {
                if let Some(gid) = &guild_id {
                    let stats = self.database.get_guild_usage_stats(gid, 1).await?;
                    Self::format_usage_stats("Server Usage Today", &stats, None)
                } else {
                    "Server usage is only available in guild channels.".to_string()
                }
            }
            "server_7d" => {
                if let Some(gid) = &guild_id {
                    let stats = self.database.get_guild_usage_stats(gid, 7).await?;
                    Self::format_usage_stats("Server Usage (7 days)", &stats, None)
                } else {
                    "Server usage is only available in guild channels.".to_string()
                }
            }
            "top_users" => {
                if let Some(gid) = &guild_id {
                    let top_users = self.database.get_guild_top_users_by_cost(gid, 7, 10).await?;
                    Self::format_top_users("Top Users by Cost (7 days)", &top_users)
                } else {
                    "Top users is only available in guild channels.".to_string()
                }
            }
            _ => "Invalid scope. Please select a valid option.".to_string(),
        };

        // Edit the deferred response
        command
            .edit_original_interaction_response(&ctx.http, |msg| {
                msg.content(response)
            })
            .await?;

        self.database.log_usage(&user_id, "usage", None).await?;
        info!("[{request_id}] ✅ Usage command completed");
        Ok(())
    }

    /// Format usage statistics into a Discord message
    fn format_usage_stats(
        title: &str,
        stats: &[(String, i64, i64, f64, i64, f64)],
        _extra_info: Option<&str>,
    ) -> String {
        if stats.is_empty() {
            return format!("**{title}**\n\nNo usage recorded for this period.");
        }

        let mut total_requests: i64 = 0;
        let mut total_tokens: i64 = 0;
        let mut total_audio_secs: f64 = 0.0;
        let mut total_images: i64 = 0;
        let mut total_cost: f64 = 0.0;

        let mut lines = vec![format!("**{title}**\n")];

        for (service_type, requests, tokens, audio_secs, images, cost) in stats {
            total_requests += requests;
            total_cost += cost;

            let details = match service_type.as_str() {
                "chat" => {
                    total_tokens += tokens;
                    format!("**Chat (GPT)**: {} requests, {} tokens, ${:.4}", requests, tokens, cost)
                }
                "whisper" => {
                    total_audio_secs += audio_secs;
                    let mins = audio_secs / 60.0;
                    format!("**Audio (Whisper)**: {} requests, {:.1} minutes, ${:.4}", requests, mins, cost)
                }
                "dalle" => {
                    total_images += images;
                    format!("**Images (DALL-E)**: {} requests, {} images, ${:.4}", requests, images, cost)
                }
                _ => format!("**{}**: {} requests, ${:.4}", service_type, requests, cost),
            };
            lines.push(details);
        }

        lines.push(String::new());
        lines.push(format!("**Total**: {} requests, ${:.4} estimated cost", total_requests, total_cost));

        if total_tokens > 0 {
            lines.push(format!("📝 {} total tokens", total_tokens));
        }
        if total_audio_secs > 0.0 {
            lines.push(format!("🎤 {:.1} minutes transcribed", total_audio_secs / 60.0));
        }
        if total_images > 0 {
            lines.push(format!("🎨 {} images generated", total_images));
        }

        lines.join("\n")
    }

    /// Format top users list into a Discord message
    fn format_top_users(title: &str, top_users: &[(String, i64, f64)]) -> String {
        if top_users.is_empty() {
            return format!("**{title}**\n\nNo usage recorded for this period.");
        }

        let mut lines = vec![format!("**{title}**\n")];

        for (i, (user_id, requests, cost)) in top_users.iter().enumerate() {
            let medal = match i {
                0 => "🥇",
                1 => "🥈",
                2 => "🥉",
                _ => "  ",
            };
            lines.push(format!("{} <@{}>: {} requests, ${:.4}", medal, user_id, requests, cost));
        }

        lines.join("\n")
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
        let chat_completion = ChatCompletion::builder(&self.openai_model, vec![
            ChatCompletionMessage {
                role: ChatCompletionMessageRole::System,
                content: Some(mediation_prompt),
                name: None,
                function_call: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ])
        .create()
        .await?;

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
            );
        }

        let response = chat_completion
            .choices
            .first()
            .and_then(|choice| choice.message.content.clone())
            .unwrap_or_else(|| "I sense tension here. Perhaps a moment of calm reflection would serve us all well.".to_string());

        Ok(response)
    }

    /// Handle /dm_stats command
    async fn handle_slash_dm_stats(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();

        // Get period option (default to week)
        let period = get_string_option(&command.data.options, "period")
            .unwrap_or_else(|| "week".to_string());

        let days = match period.as_str() {
            "today" => 1,
            "week" => 7,
            "month" => 30,
            "all" => 36500, // ~100 years
            _ => 7,
        };

        let period_display = match period.as_str() {
            "today" => "Today",
            "week" => "This Week",
            "month" => "This Month",
            "all" => "All Time",
            _ => "This Week",
        };

        debug!("[{request_id}] Fetching DM stats for user {} (period: {}, days: {})", user_id, period, days);

        match self.database.get_user_dm_stats(&user_id, days).await {
            Ok(stats) => {
                let response = if stats.session_count == 0 {
                    format!("You don't have any DM sessions recorded for {}.", period_display.to_lowercase())
                } else {
                    // Format duration
                    let duration_str = if stats.avg_session_duration_min < 1.0 {
                        format!("{:.0}s", stats.avg_session_duration_min * 60.0)
                    } else {
                        format!("{:.1}m", stats.avg_session_duration_min)
                    };

                    // Format response time
                    let response_time_str = if stats.avg_response_time_ms < 1000 {
                        format!("{}ms", stats.avg_response_time_ms)
                    } else {
                        format!("{:.1}s", stats.avg_response_time_ms as f64 / 1000.0)
                    };

                    format!(
                        "**Your DM Statistics ({})**\n\n\
                        Sessions: {} conversations\n\
                        Messages: {} sent, {} received\n\
                        Avg Session: {}\n\
                        Avg Response Time: {}\n\n\
                        **API Usage**\n\
                        Chat: {} calls, {}K tokens\n\
                        Audio: {} transcriptions\n\
                        Total Cost: ${:.4}\n\n\
                        **Feature Usage**\n\
                        Slash Commands: {}",
                        period_display,
                        stats.session_count,
                        stats.user_messages,
                        stats.bot_messages,
                        duration_str,
                        response_time_str,
                        stats.chat_calls,
                        stats.total_tokens / 1000,
                        stats.whisper_calls,
                        stats.total_cost_usd,
                        stats.slash_commands_used
                    )
                };

                command
                    .create_interaction_response(&ctx.http, |r| {
                        r
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content(&response).ephemeral(true)
                            })
                    })
                    .await?;
            }
            Err(e) => {
                error!("[{request_id}] Error fetching DM stats: {e}");
                command
                    .create_interaction_response(&ctx.http, |r| {
                        r
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content("Failed to fetch DM statistics. Please try again later.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle /session_history command
    async fn handle_slash_session_history(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let user_id = command.user.id.to_string();
        let limit = get_integer_option(&command.data.options, "limit").unwrap_or(5);

        debug!("[{request_id}] Fetching session history for user {} (limit: {})", user_id, limit);

        match self.database.get_user_recent_sessions(&user_id, limit).await {
            Ok(sessions) => {
                let resp_text = if sessions.is_empty() {
                    "You don't have any DM sessions recorded yet.".to_string()
                } else {
                    let mut output = format!("**Your Recent DM Sessions ({} most recent)**\n\n", sessions.len());

                    for (idx, session) in sessions.iter().enumerate() {
                        let status = if session.ended_at.is_some() {
                            "Ended"
                        } else {
                            "Active"
                        };

                        let started = session.started_at.split('T').next().unwrap_or(&session.started_at);
                        let response_time = if session.avg_response_time_ms < 1000 {
                            format!("{}ms", session.avg_response_time_ms)
                        } else {
                            format!("{:.1}s", session.avg_response_time_ms as f64 / 1000.0)
                        };

                        output.push_str(&format!(
                            "{}. {} | {} messages | Avg response: {} | {}\n",
                            idx + 1,
                            started,
                            session.message_count,
                            response_time,
                            status
                        ));
                    }

                    output
                };

                command
                    .create_interaction_response(&ctx.http, |r| {
                        r
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content(&resp_text).ephemeral(true)
                            })
                    })
                    .await?;
            }
            Err(e) => {
                error!("[{request_id}] Error fetching session history: {e}");
                command
                    .create_interaction_response(&ctx.http, |r| {
                        r
                            .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message
                                    .content("Failed to fetch session history. Please try again later.")
                                    .ephemeral(true)
                            })
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle /debate command - creates a threaded debate between two personas
    async fn handle_slash_debate(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        use crate::commands::slash::debate::{DEFAULT_RESPONSES, MAX_RESPONSES, MIN_RESPONSES};
        use crate::features::debate::get_active_debates;

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

        // Validate personas are different
        if persona1_id == persona2_id {
            command
                .create_interaction_response(&ctx.http, |r| {
                    r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
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
        let persona1 = self.persona_manager.get_persona(&persona1_id);
        let persona2 = self.persona_manager.get_persona(&persona2_id);

        if persona1.is_none() || persona2.is_none() {
            command
                .create_interaction_response(&ctx.http, |r| {
                    r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
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
        let (thread_id, initial_history, previous_debaters) = if let Some(prev_state) = existing_debate {
            // We're in a thread with an existing debate - this is a tag-team!
            let prev_p1 = self.persona_manager.get_persona(&prev_state.config.persona1_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| prev_state.config.persona1_id.clone());
            let prev_p2 = self.persona_manager.get_persona(&prev_state.config.persona2_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| prev_state.config.persona2_id.clone());

            info!(
                "[{request_id}] Tag-team debate: {} & {} taking over from {} & {} on '{}'",
                persona1_name, persona2_name, prev_p1, prev_p2, topic
            );

            // Fetch ALL messages from the thread (includes debate exchanges AND user Q&A)
            let thread_history = self.fetch_thread_history_for_debate(ctx, channel_id, request_id).await
                .unwrap_or_else(|e| {
                    warn!("[{request_id}] Failed to fetch thread history, falling back to debate state: {}", e);
                    prev_state.history.clone()
                });

            // Send response in the thread
            command
                .create_interaction_response(&ctx.http, |r| {
                    r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.embed(|e| {
                                e.title("New Debaters Entering!")
                                    .description(format!(
                                        "**{} and {}** are stepping aside.\n\n\
                                        **{} vs {}** will now continue the debate on:\n\
                                        *{}*\n\n\
                                        The new debaters have reviewed everything said so far.",
                                        prev_p1, prev_p2,
                                        persona1_name, persona2_name,
                                        topic
                                    ))
                                    .color(0xF39C12) // Gold for transition
                            })
                        })
                })
                .await?;

            (channel_id, Some(thread_history), Some((prev_p1, prev_p2)))
        } else {
            // Check if the current channel is a thread (by trying to get channel info)
            // For now, we'll always create a new thread if there's no existing debate
            info!(
                "[{request_id}] Starting debate: {} vs {} on '{}' ({} rounds)",
                persona1_name, persona2_name, topic, rounds
            );

            // Send initial response message (we'll create a thread from this)
            command
                .create_interaction_response(&ctx.http, |r| {
                    r.kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.content(format!(
                                "**Debate Starting!**\n\n\
                                **Topic:** {}\n\
                                **Debaters:** {} vs {}\n\
                                **Rounds:** {}",
                                topic, persona1_name, persona2_name, rounds
                            ))
                        })
                })
                .await?;

            // Get the message we just sent to create a thread from it
            let message = command.get_interaction_response(&ctx.http).await?;

            // Create the debate thread from the message
            let thread_name = format!("{} vs {} - {}",
                persona1_name, persona2_name,
                if topic.len() > 40 { format!("{}...", &topic[..37]) } else { topic.clone() });

            let thread = match channel_id
                .create_public_thread(&ctx.http, message.id, |t| {
                    t.name(&thread_name)
                        .auto_archive_duration(60) // Archive after 1 hour of inactivity
                })
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    error!("[{request_id}] Failed to create debate thread: {e}");
                    command
                        .edit_original_interaction_response(&ctx.http, |r| {
                            r.content(format!(
                                "**Debate Failed**\n\n\
                                Could not create thread. Make sure I have permission to create threads.\n\
                                Error: {}",
                                e
                            ))
                        })
                        .await?;
                    return Ok(());
                }
            };

            // Update the original message
            command
                .edit_original_interaction_response(&ctx.http, |r| {
                    r.content(format!(
                        "**Debate Started!**\n\n\
                        **Topic:** {}\n\
                        **Debaters:** {} vs {}\n\
                        **Rounds:** {}\n\n\
                        The debate is happening in the thread below!",
                        topic, persona1_name, persona2_name, rounds
                    ))
                })
                .await?;

            // Post introduction in the thread
            let intro_embed = serenity::builder::CreateEmbed::default()
                .title("Debate Beginning")
                .description(format!(
                    "**Topic:** {}\n\n\
                    **{} vs {}**\n\n\
                    {} rounds of debate ahead. Let the discourse begin!",
                    topic, persona1_name, persona2_name, rounds
                ))
                .color(0x7289DA) // Discord blurple
                .to_owned();

            let _ = thread.id.send_message(&ctx.http, |m| {
                m.set_embed(intro_embed)
            }).await;

            (thread.id, None, None)
        };

        // Create debate config with optional initial history
        let config = DebateConfig {
            persona1_id: persona1_id.clone(),
            persona2_id: persona2_id.clone(),
            topic: topic.clone(),
            rounds,
            initiator_id: command.user.id.to_string(),
            guild_id: command.guild_id.map(|g| g.to_string()),
            initial_history,
            previous_debaters,
        };

        // Clone what we need for the async closure
        let openai_model = self.openai_model.clone();
        let usage_tracker = self.usage_tracker.clone();
        let user_id = command.user.id.to_string();
        let guild_id = command.guild_id.map(|g| g.to_string());
        let channel_id_str = thread_id.to_string();

        // Run the debate (this spawns the orchestrator)
        let ctx_clone = ctx.clone();

        tokio::spawn(async move {
            // Create a closure for getting AI responses
            let get_response = |system_prompt: String, user_message: String, history: Vec<(String, String)>| {
                let model = openai_model.clone();
                let tracker = usage_tracker.clone();
                let uid = user_id.clone();
                let gid = guild_id.clone();
                let cid = channel_id_str.clone();

                async move {
                    // Build messages for OpenAI
                    let mut messages = vec![
                        openai::chat::ChatCompletionMessage {
                            role: openai::chat::ChatCompletionMessageRole::System,
                            content: Some(system_prompt),
                            name: None,
                            function_call: None,
                            tool_call_id: None,
                            tool_calls: None,
                        },
                    ];

                    // Add conversation history
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

                    // Add user message
                    messages.push(openai::chat::ChatCompletionMessage {
                        role: openai::chat::ChatCompletionMessageRole::User,
                        content: Some(user_message),
                        name: None,
                        function_call: None,
                        tool_call_id: None,
                        tool_calls: None,
                    });

                    // Call OpenAI
                    let chat_completion = openai::chat::ChatCompletion::builder(&model, messages)
                        .create()
                        .await
                        .map_err(|e| anyhow::anyhow!("OpenAI API error: {}", e))?;

                    // Log usage
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
                        );
                    }

                    // Extract response
                    chat_completion
                        .choices
                        .first()
                        .and_then(|c| c.message.content.clone())
                        .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))
                }
            };

            if let Err(e) = orchestrator.run_debate(&ctx_clone, thread_id, config, get_response).await {
                error!("Debate failed: {e}");
                let _ = thread_id.send_message(&ctx_clone.http, |m| {
                    m.content("The debate encountered an error and could not continue.")
                }).await;
            }
        });

        Ok(())
    }

    /// Handle the /ask slash command
    ///
    /// Allows users to ask any persona a question with optional context from
    /// the current channel or thread.
    async fn handle_slash_ask(
        &self,
        ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        use serenity::model::application::interaction::InteractionResponseType;
        use serenity::builder::GetMessages;

        let start_time = Instant::now();

        // Extract command options
        let persona_id = get_string_option(&command.data.options, "persona")
            .ok_or_else(|| anyhow::anyhow!("Missing persona argument"))?;
        let prompt = get_string_option(&command.data.options, "prompt")
            .ok_or_else(|| anyhow::anyhow!("Missing prompt argument"))?;
        let ignore_context = get_bool_option(&command.data.options, "ignore_context")
            .unwrap_or(false);

        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id;
        let guild_id = command.guild_id.map(|id| id.to_string());

        info!(
            "[{request_id}] /ask command | Persona: {persona_id} | User: {user_id} | Ignore context: {ignore_context}"
        );

        // Validate persona exists
        let persona = self.persona_manager.get_persona_with_portrait(&persona_id);
        if persona.is_none() {
            command
                .create_interaction_response(&ctx.http, |r| {
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

        // Get system prompt for the persona
        let system_prompt = self.persona_manager.get_system_prompt(&persona_id, None);

        // Defer the interaction (required for AI calls that may take time)
        info!("[{request_id}] Deferring interaction response");
        command
            .create_interaction_response(&ctx.http, |r| {
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
            let in_thread = self.is_in_thread_channel(ctx, channel_id).await?;

            if in_thread {
                debug!("[{request_id}] Fetching thread context");
                // Fetch recent messages from thread
                let messages = channel_id
                    .messages(&ctx.http, |builder: &mut GetMessages| builder.limit(20))
                    .await
                    .unwrap_or_default();

                let bot_id = ctx.http.get_current_user().await?.id;
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
                // Fetch from database for channels (works for both guild and DM)
                self.database
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
        self.database
            .log_usage(&user_id, "ask", Some(&persona_id))
            .await?;

        // Get AI response
        info!("[{request_id}] Calling OpenAI API");
        let ai_response = self
            .get_ai_response_with_context(
                &system_prompt,
                &prompt,
                conversation_history,
                request_id,
                Some(&user_id),
                guild_id.as_deref(),
                Some(&channel_id.to_string()),
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
                if response.len() > 4096 {
                    // Split into chunks for long responses
                    let chunks: Vec<&str> = response
                        .as_bytes()
                        .chunks(4096)
                        .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                        .collect();

                    debug!("[{request_id}] Response split into {} chunks", chunks.len());

                    if let Some(first_chunk) = chunks.first() {
                        let embed = Self::build_persona_embed(&persona, first_chunk);
                        command
                            .edit_original_interaction_response(&ctx.http, |r| r.set_embed(embed))
                            .await?;
                    }

                    // Send remaining chunks as follow-ups
                    for chunk in chunks.iter().skip(1) {
                        if !chunk.trim().is_empty() {
                            let embed = Self::build_continuation_embed(&persona, chunk);
                            command
                                .create_followup_message(&ctx.http, |m| m.set_embed(embed))
                                .await?;
                        }
                    }
                } else {
                    let embed = Self::build_persona_embed(&persona, &response);
                    command
                        .edit_original_interaction_response(&ctx.http, |r| r.set_embed(embed))
                        .await?;
                }

                info!("[{request_id}] /ask response sent successfully");
            }
            Err(e) => {
                error!("[{request_id}] AI response failed: {e}");
                command
                    .edit_original_interaction_response(&ctx.http, |r| {
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

    /// Check if a channel is a thread
    async fn is_in_thread_channel(
        &self,
        ctx: &Context,
        channel_id: serenity::model::id::ChannelId,
    ) -> Result<bool> {
        use serenity::model::channel::{Channel, ChannelType};

        match ctx.http.get_channel(channel_id.0).await {
            Ok(Channel::Guild(guild_channel)) => Ok(matches!(
                guild_channel.kind,
                ChannelType::PublicThread | ChannelType::PrivateThread
            )),
            _ => Ok(false),
        }
    }
}