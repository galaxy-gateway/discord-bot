use crate::audio::AudioTranscriber;
use crate::conflict_detector::ConflictDetector;
use crate::image_gen::{ImageGenerator, ImageSize, ImageStyle};
use crate::conflict_mediator::ConflictMediator;
use crate::database::Database;
use crate::introspection::get_component_snippet;
use crate::message_components::MessageComponentHandler;
use crate::personas::PersonaManager;
use crate::rate_limiter::RateLimiter;
use crate::commands::slash::{get_string_option, get_channel_option, get_role_option, get_integer_option};
use anyhow::Result;
use log::{debug, error, info, warn};
use tokio::time::{timeout, Duration as TokioDuration, Instant};
use uuid::Uuid;
use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};
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
}

impl CommandHandler {
    pub fn new(
        database: Database,
        openai_api_key: String,
        openai_model: String,
        conflict_enabled: bool,
        conflict_sensitivity: &str,
        mediation_cooldown_minutes: u64,
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
        }
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

        if !msg.attachments.is_empty() && should_transcribe {
            debug!("[{}] 🎵 Processing {} audio attachments (mode: {})", request_id, msg.attachments.len(), audio_mode);
            self.handle_audio_attachments(ctx, msg).await?;
        }

        let content = msg.content.trim();
        debug!("[{}] 🔍 Analyzing message content | Length: {} | Is DM: {} | Starts with command: {}",
               request_id, content.len(), is_dm, content.starts_with('!') || content.starts_with('/'));

        // Store guild messages FIRST (needed for conflict detection to have data)
        if !is_dm && !content.is_empty() && !content.starts_with('!') && !content.starts_with('/') {
            debug!("[{request_id}] 💾 Storing guild message for analysis");
            self.database.store_message(&user_id, &channel_id, "user", content, None).await?;
        }

        // Conflict detection - check both env var AND feature flag
        let guild_conflict_enabled = if let Some(gid) = guild_id_opt {
            self.database.is_feature_enabled("conflict_mediation", None, Some(gid)).await?
        } else {
            false // No conflict detection in DMs
        };

        if !is_dm && self.conflict_enabled && guild_conflict_enabled && !content.is_empty() && !content.starts_with('!') && !content.starts_with('/') {
            debug!("[{request_id}] 🔍 Running conflict detection analysis");
            if let Err(e) = self.check_and_mediate_conflicts(ctx, msg, &channel_id, guild_id_opt).await {
                warn!("[{request_id}] ⚠️ Conflict detection error: {e}");
                // Don't fail the whole message processing if conflict detection fails
            }
        }

        if content.starts_with('!') || content.starts_with('/') {
            info!("[{}] 🎯 Processing command: {}", request_id, content.split_whitespace().next().unwrap_or(""));
            self.handle_command_with_id(ctx, msg, request_id).await?;
        } else if is_dm && !content.is_empty() {
            info!("[{request_id}] 💬 Processing DM message (auto-response mode)");
            self.handle_dm_message_with_id(ctx, msg, request_id).await?;
        } else if !is_dm && self.is_bot_mentioned(ctx, msg).await? && !content.is_empty() {
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

    async fn handle_dm_message_with_id(&self, ctx: &Context, msg: &Message, request_id: Uuid) -> Result<()> {
        let user_id = msg.author.id.to_string();
        let channel_id = msg.channel_id.to_string();
        let user_message = msg.content.trim();

        debug!("[{}] 💬 Processing DM auto-response | User: {} | Message: '{}'",
               request_id, user_id, user_message.chars().take(100).collect::<String>());

        // Get user's persona
        debug!("[{request_id}] 🎭 Fetching user persona from database");
        let user_persona = self.database.get_user_persona(&user_id).await?;
        debug!("[{request_id}] 🎭 User persona: {user_persona}");

        // Store user message in conversation history
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

        // Get AI response with conversation history
        info!("[{request_id}] 🚀 Calling OpenAI API for DM response");
        match self.get_ai_response_with_id(&system_prompt, user_message, conversation_history, request_id).await {
            Ok(ai_response) => {
                info!("[{}] ✅ OpenAI response received | Response length: {}",
                      request_id, ai_response.len());

                // Stop typing
                typing.stop();
                debug!("[{request_id}] ⌨️ Stopped typing indicator");

                // Send response (handle long messages)
                if ai_response.len() > 2000 {
                    debug!("[{request_id}] 📄 Response too long, splitting into chunks");
                    let chunks: Vec<&str> = ai_response.as_bytes()
                        .chunks(2000)
                        .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                        .collect();

                    debug!("[{}] 📄 Split response into {} chunks", request_id, chunks.len());

                    for (i, chunk) in chunks.iter().enumerate() {
                        if !chunk.trim().is_empty() {
                            debug!("[{}] 📤 Sending chunk {} of {} ({} chars)",
                                   request_id, i + 1, chunks.len(), chunk.len());
                            msg.channel_id.say(&ctx.http, chunk).await?;
                            debug!("[{}] ✅ Chunk {} sent successfully", request_id, i + 1);
                        }
                    }
                    info!("[{request_id}] ✅ All DM response chunks sent successfully");
                } else {
                    debug!("[{}] 📤 Sending DM response ({} chars)", request_id, ai_response.len());
                    msg.channel_id.say(&ctx.http, &ai_response).await?;
                    info!("[{request_id}] ✅ DM response sent successfully");
                }

                // Store assistant response in conversation history
                debug!("[{request_id}] 💾 Storing assistant response to conversation history");
                self.database.store_message(&user_id, &channel_id, "assistant", &ai_response, Some(&user_persona)).await?;
                debug!("[{request_id}] ✅ Assistant response stored successfully");
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

        // Get user's persona with guild default fallback
        debug!("[{request_id}] 🎭 Fetching user persona from database");
        let user_persona = self.database.get_user_persona_with_guild(&user_id, guild_id_opt).await?;
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

        // Retrieve conversation history based on context type
        let conversation_history = if is_thread {
            // Thread context: Fetch messages from Discord
            info!("[{request_id}] 🧵 Fetching thread context from Discord");
            self.fetch_thread_messages(ctx, msg, max_context as u8, request_id).await?
        } else {
            // Channel context: Use database history
            info!("[{request_id}] 📚 Fetching channel context from database");

            // Store user message in conversation history for channels
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

        // Get AI response with conversation history
        info!("[{request_id}] 🚀 Calling OpenAI API for mention response");
        match self.get_ai_response_with_id(&system_prompt, user_message, conversation_history, request_id).await {
            Ok(ai_response) => {
                info!("[{}] ✅ OpenAI response received | Response length: {}",
                      request_id, ai_response.len());

                // Stop typing
                typing.stop();
                debug!("[{request_id}] ⌨️ Stopped typing indicator");

                // Send response as threaded reply (handle long messages)
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
            "set_persona" => {
                debug!("[{request_id}] ⚙️ Handling set_persona command");
                self.handle_slash_set_persona_with_id(ctx, command, request_id).await?;
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
            "set_channel_verbosity" => {
                debug!("[{request_id}] ⚙️ Handling set_channel_verbosity command");
                self.handle_set_channel_verbosity(ctx, command, request_id).await?;
            }
            "set_guild_setting" => {
                debug!("[{request_id}] ⚙️ Handling set_guild_setting command");
                self.handle_set_guild_setting(ctx, command, request_id).await?;
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
            _ => {
                warn!("[{}] ❓ Unknown slash command: {}", request_id, command.data.name);
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

        info!("[{request_id}] ✅ Slash command processing completed");
        Ok(())
    }

    async fn handle_command_with_id(&self, ctx: &Context, msg: &Message, request_id: Uuid) -> Result<()> {
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
            "!ping" => {
                debug!("[{request_id}] 🏓 Processing ping command");
                self.database.log_usage(&user_id, "ping", None).await?;
                debug!("[{request_id}] 📤 Sending pong response to Discord");
                msg.channel_id.say(&ctx.http, "Pong!").await?;
                info!("[{request_id}] ✅ Pong response sent successfully");
            }
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
`/set_persona` - Set your default persona
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

    async fn handle_slash_set_persona(&self, ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
        let persona_name = get_string_option(&command.data.options, "persona")
            .ok_or_else(|| anyhow::anyhow!("Missing persona parameter"))?;

        if self.persona_manager.get_persona(&persona_name).is_none() {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content("Invalid persona. Use `/personas` to see available options.")
                        })
                })
                .await?;
            return Ok(());
        }

        let user_id = command.user.id.to_string();
        self.database.set_user_persona(&user_id, &persona_name).await?;
        
        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(format!("Your persona has been set to: `{persona_name}`"))
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
        debug!("[{}] 👤 Processing for user: {} | Message: '{}'", 
               request_id, user_id, user_message.chars().take(100).collect::<String>());

        debug!("[{request_id}] 🔍 Getting user persona from database");
        let user_persona = self.database.get_user_persona(&user_id).await?;
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
            self.database.get_channel_verbosity(&guild_id.to_string(), &command.channel_id.to_string()).await?
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
        info!("[{request_id}] 🚀 Calling OpenAI API");
        match self.get_ai_response_with_id(&system_prompt, &user_message, Vec::new(), request_id).await {
            Ok(ai_response) => {
                let processing_time = start_time.elapsed();
                info!("[{}] ✅ OpenAI response received | Processing time: {:?} | Response length: {}", 
                      request_id, processing_time, ai_response.len());
                
                if ai_response.len() > 2000 {
                    debug!("[{request_id}] 📄 Response too long, splitting into chunks");
                    // For long responses, edit with the first part and send follow-ups
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
        match self.image_generator.generate_image(&prompt, size, style).await {
            Ok(generated_image) => {
                let generation_time = start_time.elapsed();
                info!("[{request_id}] ✅ Image generated | Time: {generation_time:?}");

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

    async fn handle_slash_set_persona_with_id(&self, ctx: &Context, command: &ApplicationCommandInteraction, request_id: Uuid) -> Result<()> {
        debug!("[{request_id}] ⚙️ Processing set_persona slash command");
        self.handle_slash_set_persona(ctx, command).await
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
`/set_persona <name>` - Set your default persona
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
        self.get_ai_response_with_id(system_prompt, user_message, Vec::new(), Uuid::new_v4()).await
    }

    pub async fn get_ai_response_with_id(&self, system_prompt: &str, user_message: &str, conversation_history: Vec<(String, String)>, request_id: Uuid) -> Result<String> {
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

    async fn handle_audio_attachments(&self, ctx: &Context, msg: &Message) -> Result<()> {
        let user_id = msg.author.id.to_string();
        
        for attachment in &msg.attachments {
            if self.is_audio_attachment(&attachment.filename) {
                info!("Processing audio attachment: {}", attachment.filename);
                
                msg.channel_id
                    .say(&ctx.http, "🎵 Transcribing your audio... please wait!")
                    .await?;

                match self
                    .audio_transcriber
                    .download_and_transcribe_attachment(&attachment.url, &attachment.filename)
                    .await
                {
                    Ok(transcription) => {
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

                            if !msg.content.trim().is_empty() {
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
        
        Ok(())
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
            let mediation_text = match self.generate_mediation_response(&recent_messages, &conflict_type, confidence).await {
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

    /// Handle /set_channel_verbosity command
    async fn handle_set_channel_verbosity(
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

        let level = get_string_option(&command.data.options, "level")
            .ok_or_else(|| anyhow::anyhow!("Missing level parameter"))?;

        // Validate level
        if !["concise", "normal", "detailed"].contains(&level.as_str()) {
            command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content("❌ Invalid verbosity level. Use: `concise`, `normal`, or `detailed`.")
                        })
                })
                .await?;
            return Ok(());
        }

        // Get target channel (default to current channel)
        let target_channel_id = get_channel_option(&command.data.options, "channel")
            .map(|id| id.to_string())
            .unwrap_or_else(|| command.channel_id.to_string());

        info!("[{request_id}] Setting verbosity for channel {target_channel_id} to {level}");

        // Set the verbosity
        self.database.set_channel_verbosity(&guild_id, &target_channel_id, &level).await?;

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(format!(
                            "✅ Verbosity for <#{target_channel_id}> set to **{level}**"
                        ))
                    })
            })
            .await?;

        Ok(())
    }

    /// Handle /set_guild_setting command
    async fn handle_set_guild_setting(
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

        let setting = get_string_option(&command.data.options, "setting")
            .ok_or_else(|| anyhow::anyhow!("Missing setting parameter"))?;

        let value = get_string_option(&command.data.options, "value")
            .ok_or_else(|| anyhow::anyhow!("Missing value parameter"))?;

        // Validate setting and value
        let (is_valid, error_msg) = match setting.as_str() {
            "default_verbosity" => {
                if ["concise", "normal", "detailed"].contains(&value.as_str()) {
                    (true, "")
                } else {
                    (false, "Invalid verbosity level. Use: `concise`, `normal`, or `detailed`.")
                }
            }
            "default_persona" => {
                if ["obi", "muppet", "chef", "teacher", "analyst"].contains(&value.as_str()) {
                    (true, "")
                } else {
                    (false, "Invalid persona. Use: `obi`, `muppet`, `chef`, `teacher`, or `analyst`.")
                }
            }
            "conflict_mediation" => {
                if ["enabled", "disabled"].contains(&value.as_str()) {
                    (true, "")
                } else {
                    (false, "Invalid value. Use: `enabled` or `disabled`.")
                }
            }
            "conflict_sensitivity" => {
                if ["low", "medium", "high", "ultra"].contains(&value.as_str()) {
                    (true, "")
                } else {
                    (false, "Invalid sensitivity. Use: `low`, `medium`, `high`, or `ultra`.")
                }
            }
            "mediation_cooldown" => {
                if ["1", "5", "10", "15", "30", "60"].contains(&value.as_str()) {
                    (true, "")
                } else {
                    (false, "Invalid cooldown. Use: `1`, `5`, `10`, `15`, `30`, or `60` (minutes).")
                }
            }
            "max_context_messages" => {
                if ["10", "20", "40", "60"].contains(&value.as_str()) {
                    (true, "")
                } else {
                    (false, "Invalid context size. Use: `10`, `20`, `40`, or `60` (messages).")
                }
            }
            "audio_transcription" => {
                if ["enabled", "disabled"].contains(&value.as_str()) {
                    (true, "")
                } else {
                    (false, "Invalid value. Use: `enabled` or `disabled`.")
                }
            }
            "audio_transcription_mode" => {
                if ["always", "mention_only", "disabled"].contains(&value.as_str()) {
                    (true, "")
                } else {
                    (false, "Invalid mode. Use: `always`, `mention_only`, or `disabled`.")
                }
            }
            "mention_responses" => {
                if ["enabled", "disabled"].contains(&value.as_str()) {
                    (true, "")
                } else {
                    (false, "Invalid value. Use: `enabled` or `disabled`.")
                }
            }
            _ => (false, "Unknown setting. Use `/settings` to see available options."),
        };

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

        info!("[{request_id}] Setting guild {guild_id} setting '{setting}' to '{value}'");

        // Set the guild setting
        self.database.set_guild_setting(&guild_id, &setting, &value).await?;

        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(format!(
                            "✅ Guild setting `{setting}` set to **{value}**"
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

        // Get channel settings
        let (channel_verbosity, conflict_enabled) = self.database.get_channel_settings(&guild_id, &channel_id).await?;

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
        let guild_mention_responses = self.database.get_guild_setting(&guild_id, "mention_responses").await?
            .unwrap_or_else(|| "enabled".to_string());

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
            • Mention Responses: `{}`\n\
            • Bot Admin Role: {}\n",
            channel_id,
            channel_verbosity,
            if conflict_enabled { "Enabled ✅" } else { "Disabled ❌" },
            guild_default_verbosity,
            guild_default_persona,
            guild_conflict_mediation,
            guild_conflict_sensitivity,
            guild_mediation_cooldown,
            guild_max_context,
            guild_audio_transcription,
            guild_audio_mode,
            guild_mention_responses,
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

        // Get user's persona
        let persona_name = self.database.get_user_persona_with_guild(&user_id, guild_id.as_deref()).await?;

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

        let response = match chat_completion {
            Ok(completion) => {
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
        use crate::system_info::{CurrentMetrics, HistoricalSummary, format_history};

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

    /// Generate a context-aware mediation response using OpenAI
    async fn generate_mediation_response(
        &self,
        messages: &[(String, String, String)], // (user_id, content, timestamp)
        conflict_type: &str,
        confidence: f32,
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

        let response = chat_completion
            .choices
            .first()
            .and_then(|choice| choice.message.content.clone())
            .unwrap_or_else(|| "I sense tension here. Perhaps a moment of calm reflection would serve us all well.".to_string());

        Ok(response)
    }
}