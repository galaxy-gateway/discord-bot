use anyhow::Result;
use dotenvy::dotenv;
use log::{error, info};
use serenity::async_trait;
use serenity::model::application::interaction::Interaction;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::sync::Arc;

use persona::commands::{
    register_global_commands_with_plugins, register_guild_commands_with_plugins, CommandHandler,
};
use persona::core::Config;
use persona::database::Database;
use persona::features::analytics::{metrics_collection_loop, InteractionTracker, UsageTracker};
use persona::features::personas::PersonaManager;
use persona::features::plugins::{
    JobManager, OutputHandler, Plugin, PluginConfig, PluginExecutor, PluginManager,
};
use persona::features::reminders::ReminderScheduler;
use persona::features::startup::StartupNotifier;
use persona::ipc::{
    AttachmentInfo, BotEvent, ChannelInfo, ChannelType, DisplayMessage, GuildInfo, IpcServer,
};
use persona::message_components::MessageComponentHandler;
use serenity::model::guild::Guild;
use serenity::model::id::GuildId;

struct Handler {
    command_handler: Arc<CommandHandler>,
    component_handler: Arc<MessageComponentHandler>,
    guild_id: Option<GuildId>,
    startup_notifier: StartupNotifier,
    plugins: Vec<Plugin>,
    ipc_server: Option<Arc<IpcServer>>,
    start_time: std::time::Instant,
}

impl Handler {
    fn new(
        command_handler: CommandHandler,
        component_handler: MessageComponentHandler,
        guild_id: Option<GuildId>,
        startup_notifier: StartupNotifier,
        plugins: Vec<Plugin>,
        ipc_server: Option<Arc<IpcServer>>,
    ) -> Self {
        Handler {
            command_handler: Arc::new(command_handler),
            component_handler: Arc::new(component_handler),
            guild_id,
            startup_notifier,
            plugins,
            ipc_server,
            start_time: std::time::Instant::now(),
        }
    }

    /// Convert a Serenity message to a DisplayMessage for IPC
    fn to_display_message(msg: &Message) -> DisplayMessage {
        // Convert serenity timestamp to chrono DateTime
        let timestamp = chrono::DateTime::from_timestamp(msg.timestamp.unix_timestamp(), 0)
            .unwrap_or_else(chrono::Utc::now);

        DisplayMessage {
            id: msg.id.0,
            author_id: msg.author.id.0,
            author_name: msg.author.name.clone(),
            author_discriminator: msg.author.discriminator.to_string(),
            content: msg.content.clone(),
            timestamp,
            is_bot: msg.author.bot,
            attachments: msg
                .attachments
                .iter()
                .map(|a| AttachmentInfo {
                    filename: a.filename.clone(),
                    size: a.size,
                    url: a.url.clone(),
                })
                .collect(),
            embeds_count: msg.embeds.len(),
        }
    }

    /// Build guild info list from cache for all specified guilds
    fn build_guild_info_from_cache(&self, ctx: &Context, guild_ids: &[GuildId]) -> Vec<GuildInfo> {
        use serenity::model::channel::Channel as SerenityChannel;
        use serenity::model::channel::ChannelType as SerenityChannelType;

        guild_ids
            .iter()
            .filter_map(|&guild_id| {
                ctx.cache.guild(guild_id).map(|guild| {
                    let channels: Vec<ChannelInfo> = guild
                        .channels
                        .iter()
                        .filter_map(|(channel_id, channel)| {
                            if let SerenityChannel::Guild(gc) = channel {
                                let channel_type = match gc.kind {
                                    SerenityChannelType::Text => ChannelType::Text,
                                    SerenityChannelType::Voice => ChannelType::Voice,
                                    SerenityChannelType::Category => ChannelType::Category,
                                    SerenityChannelType::News => ChannelType::News,
                                    SerenityChannelType::NewsThread
                                    | SerenityChannelType::PublicThread
                                    | SerenityChannelType::PrivateThread => ChannelType::Thread,
                                    _ => ChannelType::Other,
                                };

                                Some(ChannelInfo {
                                    id: channel_id.0,
                                    name: gc.name.clone(),
                                    channel_type,
                                })
                            } else {
                                None
                            }
                        })
                        .collect();

                    // Also include threads from the guild's thread list (Vec<GuildChannel>)
                    let mut all_channels = channels;
                    for thread in guild.threads.iter() {
                        all_channels.push(ChannelInfo {
                            id: thread.id.0,
                            name: thread.name.clone(),
                            channel_type: ChannelType::Thread,
                        });
                    }

                    GuildInfo {
                        id: guild_id.0,
                        name: guild.name.clone(),
                        channels: all_channels,
                        member_count: Some(guild.member_count),
                    }
                })
            })
            .collect()
    }

    /// Build guild info for a single guild
    fn build_single_guild_info(&self, _ctx: &Context, guild: &Guild) -> GuildInfo {
        use serenity::model::channel::Channel as SerenityChannel;
        use serenity::model::channel::ChannelType as SerenityChannelType;

        let channels: Vec<ChannelInfo> = guild
            .channels
            .iter()
            .filter_map(|(channel_id, channel)| {
                if let SerenityChannel::Guild(gc) = channel {
                    let channel_type = match gc.kind {
                        SerenityChannelType::Text => ChannelType::Text,
                        SerenityChannelType::Voice => ChannelType::Voice,
                        SerenityChannelType::Category => ChannelType::Category,
                        SerenityChannelType::News => ChannelType::News,
                        SerenityChannelType::NewsThread
                        | SerenityChannelType::PublicThread
                        | SerenityChannelType::PrivateThread => ChannelType::Thread,
                        _ => ChannelType::Other,
                    };

                    Some(ChannelInfo {
                        id: channel_id.0,
                        name: gc.name.clone(),
                        channel_type,
                    })
                } else {
                    None
                }
            })
            .collect();

        // Also include threads (Vec<GuildChannel>)
        let mut all_channels = channels;
        for thread in guild.threads.iter() {
            all_channels.push(ChannelInfo {
                id: thread.id.0,
                name: thread.name.clone(),
                channel_type: ChannelType::Thread,
            });
        }

        GuildInfo {
            id: guild.id.0,
            name: guild.name.clone(),
            channels: all_channels,
            member_count: Some(guild.member_count),
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        // Cache user info for TUI display (always, regardless of watched status)
        if let Some(ipc) = &self.ipc_server {
            ipc.cache_user(
                msg.author.id.0,
                &msg.author.name,
                msg.author.discriminator,
                msg.author.bot,
            )
            .await;
        }

        // Forward message to IPC clients if channel is being watched
        if let Some(ipc) = &self.ipc_server {
            let channel_id = msg.channel_id.0;
            if ipc.is_channel_watched(channel_id).await {
                let display_msg = Self::to_display_message(&msg);
                ipc.broadcast(BotEvent::MessageCreate {
                    channel_id,
                    guild_id: msg.guild_id.map(|g| g.0),
                    message: display_msg,
                });
            }
        }

        if let Err(e) = self.command_handler.handle_message(&ctx, &msg).await {
            error!("Error handling message: {e}");
            if let Err(why) = msg
                .channel_id
                .say(
                    &ctx.http,
                    "Sorry, I encountered an error processing your message.",
                )
                .await
            {
                error!("Failed to send error message: {why}");
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("🎉 {} is connected and ready!", ready.user.name);
        info!("📡 Connected to {} guilds", ready.guilds.len());
        info!("🔗 Gateway session ID: {:?}", ready.session_id);
        info!("🤖 Bot ID: {}", ready.user.id);
        info!("🌐 Gateway version: {}", ready.version);

        // Log shard information
        if let Some(shard) = ready.shard {
            info!("⚡ Shard: {}/{}", shard[0] + 1, shard[1]);
        }

        // Log plugin information
        let enabled_plugins: Vec<_> = self.plugins.iter().filter(|p| p.enabled).collect();
        if !enabled_plugins.is_empty() {
            info!(
                "🔌 {} plugins loaded ({} enabled)",
                self.plugins.len(),
                enabled_plugins.len()
            );
            for plugin in &enabled_plugins {
                info!("   - /{} ({})", plugin.command.name, plugin.name);
            }
        }

        // Forward Ready event to IPC clients
        if let Some(ipc) = &self.ipc_server {
            // Build guild info list using cache for actual names and channels
            let mut guilds: Vec<GuildInfo> = Vec::new();

            for unavailable_guild in &ready.guilds {
                let guild_id = unavailable_guild.id;

                // Try to get guild info from cache
                if let Some(guild) = ctx.cache.guild(guild_id) {
                    use serenity::model::channel::Channel as SerenityChannel;
                    use serenity::model::channel::ChannelType as SerenityChannelType;

                    let mut channels: Vec<ChannelInfo> = guild
                        .channels
                        .iter()
                        .filter_map(|(channel_id, channel)| {
                            // Extract GuildChannel from Channel enum
                            if let SerenityChannel::Guild(gc) = channel {
                                let channel_type = match gc.kind {
                                    SerenityChannelType::Text => ChannelType::Text,
                                    SerenityChannelType::Voice => ChannelType::Voice,
                                    SerenityChannelType::Category => ChannelType::Category,
                                    SerenityChannelType::News => ChannelType::News,
                                    SerenityChannelType::NewsThread
                                    | SerenityChannelType::PublicThread
                                    | SerenityChannelType::PrivateThread => ChannelType::Thread,
                                    _ => ChannelType::Other,
                                };

                                Some(ChannelInfo {
                                    id: channel_id.0,
                                    name: gc.name.clone(),
                                    channel_type,
                                })
                            } else {
                                None
                            }
                        })
                        .collect();

                    // Also include threads from guild.threads (Vec<GuildChannel>)
                    for thread in guild.threads.iter() {
                        channels.push(ChannelInfo {
                            id: thread.id.0,
                            name: thread.name.clone(),
                            channel_type: ChannelType::Thread,
                        });
                    }

                    guilds.push(GuildInfo {
                        id: guild_id.0,
                        name: guild.name.clone(),
                        channels,
                        member_count: Some(guild.member_count),
                    });
                } else {
                    // Guild not in cache yet, use placeholder - will be updated by cache_ready
                    guilds.push(GuildInfo {
                        id: guild_id.0,
                        name: format!("Guild {}", guild_id.0),
                        channels: Vec::new(),
                        member_count: None,
                    });
                }
            }

            // Store in IPC server for later queries
            ipc.set_guilds(guilds.clone()).await;
            ipc.set_bot_info(ready.user.id.0, ready.user.name.clone())
                .await;
            ipc.set_http(ctx.http.clone()).await;

            ipc.broadcast(BotEvent::Ready {
                guilds,
                bot_user_id: ready.user.id.0,
                bot_username: ready.user.name.clone(),
            });

            info!(
                "📡 IPC: Sent Ready event to TUI clients with {} guilds",
                ready.guilds.len()
            );
        }

        // Register slash commands - use guild commands for development (instant), global for production
        if let Some(guild_id) = self.guild_id {
            info!("🔧 Development mode: Registering commands for guild {guild_id}");
            if let Err(e) =
                register_guild_commands_with_plugins(&ctx, guild_id, &self.plugins).await
            {
                error!("❌ Failed to register guild slash commands: {e}");
            } else {
                info!("✅ Successfully registered slash commands for guild {guild_id} (instant update)");
            }
        } else {
            info!("🌍 Production mode: Registering commands globally");
            if let Err(e) = register_global_commands_with_plugins(&ctx, &self.plugins).await {
                error!("❌ Failed to register global slash commands: {e}");
            } else {
                info!("✅ Successfully registered slash commands globally (may take up to 1 hour to propagate)");
            }
        }

        // Send startup notification if enabled (includes plugin versions and commit details)
        self.startup_notifier
            .send_if_enabled(&ctx.http, &ready, &self.plugins)
            .await;
    }

    async fn cache_ready(&self, ctx: Context, guilds: Vec<GuildId>) {
        info!("📦 Cache ready with {} guilds fully loaded", guilds.len());

        // Update IPC with full guild/channel info now that cache is ready
        if let Some(ipc) = &self.ipc_server {
            let guild_info = self.build_guild_info_from_cache(&ctx, &guilds);
            let guild_count = guild_info.len();

            ipc.set_guilds(guild_info.clone()).await;

            // Broadcast updated Ready event with full channel info
            if let Some(user_id) = ipc.get_bot_user_id().await {
                if let Some(username) = ipc.get_bot_username().await {
                    ipc.broadcast(BotEvent::Ready {
                        guilds: guild_info,
                        bot_user_id: user_id,
                        bot_username: username,
                    });
                    info!(
                        "📡 IPC: Sent updated Ready event with {guild_count} guilds and full channel data"
                    );
                }
            }
        }
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: bool) {
        if is_new {
            info!("🆕 Joined new guild: {} ({})", guild.name, guild.id);
        } else {
            info!(
                "📥 Guild available: {} ({}) - {} channels",
                guild.name,
                guild.id,
                guild.channels.len()
            );
        }

        // Update IPC with this guild's info
        if let Some(ipc) = &self.ipc_server {
            let guild_info = self.build_single_guild_info(&ctx, &guild);

            // Get current guilds and update/add this one
            let mut current_guilds = ipc.get_guilds().await;
            if let Some(existing) = current_guilds.iter_mut().find(|g| g.id == guild.id.0) {
                *existing = guild_info;
            } else {
                current_guilds.push(guild_info);
            }

            ipc.set_guilds(current_guilds.clone()).await;

            // Broadcast updated guild list
            if let Some(user_id) = ipc.get_bot_user_id().await {
                if let Some(username) = ipc.get_bot_username().await {
                    ipc.broadcast(BotEvent::Ready {
                        guilds: current_guilds,
                        bot_user_id: user_id,
                        bot_username: username,
                    });
                }
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::ApplicationCommand(command) => {
                if let Err(e) = self
                    .command_handler
                    .handle_slash_command(&ctx, &command)
                    .await
                {
                    error!(
                        "Error handling slash command '{}': {}",
                        command.data.name, e
                    );

                    // Try to edit the deferred response with error message
                    let error_message = if e.to_string().contains("timeout")
                        || e.to_string().contains("OpenAI")
                    {
                        "⏱️ Sorry, the AI service is taking longer than expected. Please try again in a moment."
                    } else {
                        "❌ Sorry, I encountered an error processing your command. Please try again."
                    };

                    // Try to edit the deferred response, fallback to new response if that fails
                    #[allow(clippy::redundant_pattern_matching)]
                    if let Err(_) = command
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.content(error_message)
                        })
                        .await
                    {
                        let _ = command.create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|message| {
                                    message.content(error_message)
                                })
                        }).await;
                    }
                }
            }
            Interaction::MessageComponent(component) => {
                if let Err(e) = self
                    .component_handler
                    .handle_component_interaction(&ctx, &component)
                    .await
                {
                    error!(
                        "Error handling component interaction '{}': {}",
                        component.data.custom_id, e
                    );

                    let error_message = "❌ Sorry, I encountered an error processing your interaction. Please try again.";

                    // Try to update the message, fallback to new response if that fails
                    #[allow(clippy::redundant_pattern_matching)]
                    if let Err(_) = component.create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(serenity::model::application::interaction::InteractionResponseType::UpdateMessage)
                            .interaction_response_data(|message| {
                                message.content(error_message)
                            })
                    }).await {
                        let _ = component.create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|message| {
                                    message.content(error_message)
                                })
                        }).await;
                    }
                }
            }
            Interaction::ModalSubmit(modal) => {
                if let Err(e) = self
                    .component_handler
                    .handle_modal_submit(&ctx, &modal)
                    .await
                {
                    error!(
                        "Error handling modal submit '{}': {}",
                        modal.data.custom_id, e
                    );

                    let error_message = if e.to_string().contains("timeout")
                        || e.to_string().contains("OpenAI")
                    {
                        "⏱️ Sorry, the AI service is taking longer than expected. Please try again in a moment."
                    } else {
                        "❌ Sorry, I encountered an error processing your submission. Please try again."
                    };

                    // Try to edit the deferred response, fallback to new response if that fails
                    #[allow(clippy::redundant_pattern_matching)]
                    if let Err(_) = modal
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.content(error_message)
                        })
                        .await
                    {
                        let _ = modal.create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(serenity::model::application::interaction::InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|message| {
                                    message.content(error_message)
                                })
                        }).await;
                    }
                }
            }
            Interaction::Autocomplete(autocomplete) => {
                info!(
                    "Autocomplete interaction received for command: {}",
                    autocomplete.data.name
                );

                // Handle autocomplete based on command
                let _ = match autocomplete.data.name.as_str() {
                    "set_user" => {
                        // Get the setting option to determine which choices to show
                        let setting = autocomplete
                            .data
                            .options
                            .iter()
                            .find(|opt| opt.name == "setting")
                            .and_then(|opt| opt.value.as_ref())
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        autocomplete
                            .create_autocomplete_response(&ctx.http, |response| match setting {
                                "persona" => response
                                    .add_string_choice("obi - Obi-Wan Kenobi (wise mentor)", "obi")
                                    .add_string_choice(
                                        "muppet - Enthusiastic Muppet friend",
                                        "muppet",
                                    )
                                    .add_string_choice("chef - Passionate cooking expert", "chef")
                                    .add_string_choice("teacher - Patient educator", "teacher")
                                    .add_string_choice("analyst - Step-by-step analyst", "analyst")
                                    .add_string_choice(
                                        "visionary - Future-focused big thinker",
                                        "visionary",
                                    )
                                    .add_string_choice("noir - Hard-boiled detective", "noir")
                                    .add_string_choice("zen - Contemplative sage", "zen")
                                    .add_string_choice("bard - Charismatic storyteller", "bard")
                                    .add_string_choice("coach - Motivational coach", "coach")
                                    .add_string_choice(
                                        "scientist - Curious researcher",
                                        "scientist",
                                    )
                                    .add_string_choice(
                                        "gamer - Friendly gaming enthusiast",
                                        "gamer",
                                    ),
                                _ => response,
                            })
                            .await
                    }
                    "set_channel" => {
                        // Get the setting option to determine which choices to show
                        let setting = autocomplete
                            .data
                            .options
                            .iter()
                            .find(|opt| opt.name == "setting")
                            .and_then(|opt| opt.value.as_ref())
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        autocomplete
                            .create_autocomplete_response(&ctx.http, |response| match setting {
                                "verbosity" => response
                                    .add_string_choice(
                                        "concise - Brief responses (2-3 sentences)",
                                        "concise",
                                    )
                                    .add_string_choice("normal - Balanced responses", "normal")
                                    .add_string_choice(
                                        "detailed - Comprehensive responses",
                                        "detailed",
                                    ),
                                "persona" => response
                                    .add_string_choice("obi - Obi-Wan Kenobi (wise mentor)", "obi")
                                    .add_string_choice(
                                        "muppet - Enthusiastic Muppet friend",
                                        "muppet",
                                    )
                                    .add_string_choice("chef - Passionate cooking expert", "chef")
                                    .add_string_choice("teacher - Patient educator", "teacher")
                                    .add_string_choice("analyst - Step-by-step analyst", "analyst")
                                    .add_string_choice(
                                        "visionary - Future-focused big thinker",
                                        "visionary",
                                    )
                                    .add_string_choice("noir - Hard-boiled detective", "noir")
                                    .add_string_choice("zen - Contemplative sage", "zen")
                                    .add_string_choice("bard - Charismatic storyteller", "bard")
                                    .add_string_choice("coach - Motivational coach", "coach")
                                    .add_string_choice(
                                        "scientist - Curious researcher",
                                        "scientist",
                                    )
                                    .add_string_choice(
                                        "gamer - Friendly gaming enthusiast",
                                        "gamer",
                                    )
                                    .add_string_choice(
                                        "clear - Remove channel persona override",
                                        "clear",
                                    ),
                                "conflict_mediation" => response
                                    .add_string_choice(
                                        "enabled - Enable conflict mediation in this channel",
                                        "enabled",
                                    )
                                    .add_string_choice(
                                        "disabled - Disable conflict mediation in this channel",
                                        "disabled",
                                    ),
                                _ => response,
                            })
                            .await
                    }
                    "set_guild" => {
                        // Get the setting option to determine which choices to show
                        let setting = autocomplete
                            .data
                            .options
                            .iter()
                            .find(|opt| opt.name == "setting")
                            .and_then(|opt| opt.value.as_ref())
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        autocomplete
                            .create_autocomplete_response(&ctx.http, |response| {
                                match setting {
                                    "default_verbosity" => response
                                        .add_string_choice(
                                            "concise - Brief responses (2-3 sentences)",
                                            "concise",
                                        )
                                        .add_string_choice("normal - Balanced responses", "normal")
                                        .add_string_choice(
                                            "detailed - Comprehensive responses",
                                            "detailed",
                                        ),
                                    "default_persona" => response
                                        .add_string_choice(
                                            "obi - Obi-Wan Kenobi (wise mentor)",
                                            "obi",
                                        )
                                        .add_string_choice(
                                            "muppet - Enthusiastic Muppet friend",
                                            "muppet",
                                        )
                                        .add_string_choice(
                                            "chef - Passionate cooking expert",
                                            "chef",
                                        )
                                        .add_string_choice("teacher - Patient educator", "teacher")
                                        .add_string_choice(
                                            "analyst - Step-by-step analyst",
                                            "analyst",
                                        )
                                        .add_string_choice(
                                            "visionary - Future-focused big thinker",
                                            "visionary",
                                        )
                                        .add_string_choice("noir - Hard-boiled detective", "noir")
                                        .add_string_choice("zen - Contemplative sage", "zen")
                                        .add_string_choice("bard - Charismatic storyteller", "bard")
                                        .add_string_choice("coach - Motivational coach", "coach")
                                        .add_string_choice(
                                            "scientist - Curious researcher",
                                            "scientist",
                                        )
                                        .add_string_choice(
                                            "gamer - Friendly gaming enthusiast",
                                            "gamer",
                                        ),
                                    "conflict_mediation" => response
                                        .add_string_choice(
                                            "enabled - Bot will mediate conflicts",
                                            "enabled",
                                        )
                                        .add_string_choice(
                                            "disabled - No conflict mediation",
                                            "disabled",
                                        ),
                                    "conflict_sensitivity" => response
                                        .add_string_choice(
                                            "low - Only obvious conflicts (0.7 threshold)",
                                            "low",
                                        )
                                        .add_string_choice(
                                            "medium - Balanced detection (0.5 threshold)",
                                            "medium",
                                        )
                                        .add_string_choice(
                                            "high - More sensitive (0.35 threshold)",
                                            "high",
                                        )
                                        .add_string_choice(
                                            "ultra - Maximum sensitivity (0.3 threshold)",
                                            "ultra",
                                        ),
                                    "mediation_cooldown" => response
                                        .add_string_choice("1 minute", "1")
                                        .add_string_choice("5 minutes (default)", "5")
                                        .add_string_choice("10 minutes", "10")
                                        .add_string_choice("15 minutes", "15")
                                        .add_string_choice("30 minutes", "30")
                                        .add_string_choice("60 minutes", "60"),
                                    "max_context_messages" => response
                                        .add_string_choice("10 messages (minimal context)", "10")
                                        .add_string_choice("20 messages (light context)", "20")
                                        .add_string_choice("40 messages (default)", "40")
                                        .add_string_choice("60 messages (extended context)", "60"),
                                    "audio_transcription" => response
                                        .add_string_choice(
                                            "enabled - Transcribe audio files",
                                            "enabled",
                                        )
                                        .add_string_choice(
                                            "disabled - Skip audio processing",
                                            "disabled",
                                        ),
                                    "audio_transcription_mode" => response
                                        .add_string_choice(
                                            "always - Transcribe all audio files",
                                            "always",
                                        )
                                        .add_string_choice(
                                            "mention_only - Only when @mentioned",
                                            "mention_only",
                                        ),
                                    "audio_transcription_output" => response
                                        .add_string_choice(
                                            "transcription_only - Just the transcription",
                                            "transcription_only",
                                        )
                                        .add_string_choice(
                                            "with_commentary - Add AI commentary",
                                            "with_commentary",
                                        ),
                                    "mention_responses" => response
                                        .add_string_choice(
                                            "enabled - Respond when @mentioned",
                                            "enabled",
                                        )
                                        .add_string_choice(
                                            "disabled - Ignore mentions",
                                            "disabled",
                                        ),
                                    "response_embeds" => response
                                        .add_string_choice(
                                            "enabled - Responses in embed boxes (default)",
                                            "enabled",
                                        )
                                        .add_string_choice(
                                            "disabled - Plain text responses",
                                            "disabled",
                                        ),
                                    // Startup notification settings (global)
                                    "startup_notification" => response
                                        .add_string_choice(
                                            "enabled - Send notification on startup",
                                            "enabled",
                                        )
                                        .add_string_choice(
                                            "disabled - No startup notification",
                                            "disabled",
                                        ),
                                    "startup_dm_commit_count" | "startup_channel_commit_count" => {
                                        response
                                            .add_string_choice("0 - No commits shown", "0")
                                            .add_string_choice("1 - Most recent commit only", "1")
                                            .add_string_choice("3 - Last 3 commits", "3")
                                            .add_string_choice("5 - Last 5 commits (default)", "5")
                                            .add_string_choice("10 - Last 10 commits", "10")
                                    }
                                    // For ID fields, don't show autocomplete - user must type the ID directly
                                    // Return empty response so Discord shows the text input
                                    "startup_notify_owner_id" | "startup_notify_channel_id" => {
                                        response
                                    }
                                    _ => response,
                                }
                            })
                            .await
                    }
                    _ => {
                        // Default empty response for unknown commands
                        autocomplete
                            .create_autocomplete_response(&ctx.http, |response| response)
                            .await
                    }
                };
            }
            Interaction::Ping(_) => {
                info!("Ping interaction received - Discord health check");
                // Ping interactions are automatically handled by Serenity
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv().ok();

    let config = Config::from_env()?;

    // Ensure OPENAI_API_KEY is set in environment for the openai crate
    // The openai crate reads from env vars, not from our config
    // Set both OPENAI_API_KEY and OPENAI_KEY for compatibility
    std::env::set_var("OPENAI_API_KEY", &config.openai_api_key);
    std::env::set_var("OPENAI_KEY", &config.openai_api_key);

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&config.log_level))
        .init();

    info!("Starting Persona Discord Bot...");

    // Create database first so IPC server can use it for stats queries
    let database = Database::new(&config.database_path).await?;

    // Start IPC server for TUI communication with database access
    let ipc_server =
        Arc::new(IpcServer::new().with_database(database.clone(), config.database_path.clone()));
    if let Err(e) = ipc_server.clone().start().await {
        error!(
            "Failed to start IPC server: {e}. TUI control will be unavailable."
        );
    } else {
        info!("📡 IPC server started for TUI communication");
    }

    // Spawn IPC heartbeat task
    let heartbeat_ipc = ipc_server.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            heartbeat_ipc.send_heartbeat();
        }
    });

    // Start IPC command processor
    ipc_server.clone().start_command_processor();
    let usage_tracker = UsageTracker::new(database.clone());
    let interaction_tracker = InteractionTracker::new(database.clone());
    let persona_manager = PersonaManager::new();
    let mut command_handler = CommandHandler::new(
        database.clone(),
        config.openai_api_key.clone(),
        config.openai_model.clone(),
        config.conflict_mediation_enabled,
        &config.conflict_sensitivity,
        config.mediation_cooldown_minutes,
        usage_tracker.clone(),
        interaction_tracker,
    );

    // Load plugins from config file
    let plugins_path =
        std::env::var("PLUGINS_CONFIG_PATH").unwrap_or_else(|_| "plugins.yaml".to_string());
    let (plugins, _plugin_manager): (Vec<Plugin>, Option<Arc<PluginManager>>) =
        match PluginConfig::load(&plugins_path) {
            Ok(plugin_config) => {
                info!("📄 Loaded plugin config from {plugins_path}");
                let plugins = plugin_config.plugins.clone();

                // Create allowed commands list
                let allowed_commands = vec!["docker".to_string(), "sh".to_string()];

                // Create plugin manager with usage tracker for AI summary tracking
                let job_manager = Arc::new(JobManager::new(database.clone()));
                let executor = PluginExecutor::new(allowed_commands);
                let output_handler = OutputHandler::new(config.openai_model.clone())
                    .with_usage_tracker(usage_tracker.clone());

                let pm = Arc::new(PluginManager {
                    config: plugin_config,
                    executor,
                    job_manager,
                    output_handler,
                });

                // Set plugin manager on command handler
                command_handler.set_plugin_manager(pm.clone());

                (plugins, Some(pm))
            }
            Err(e) => {
                if std::path::Path::new(&plugins_path).exists() {
                    error!("❌ Failed to load plugins from {plugins_path}: {e}");
                } else {
                    info!(
                        "📄 No plugins.yaml found at {plugins_path} - plugin system disabled"
                    );
                }
                (vec![], None)
            }
        };

    let component_handler =
        MessageComponentHandler::new(command_handler.clone(), persona_manager, database.clone());

    // Parse guild ID if provided for development mode
    let guild_id = config
        .discord_guild_id
        .as_ref()
        .and_then(|id| id.parse::<u64>().ok())
        .map(GuildId);

    // Create startup notifier (reads config from database)
    let startup_notifier = StartupNotifier::new(Arc::new(database.clone()));

    let handler = Handler::new(
        command_handler,
        component_handler,
        guild_id,
        startup_notifier,
        plugins,
        Some(ipc_server),
    );

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    // Build the Discord client with proper gateway configuration
    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| {
            error!("Failed to create Discord client: {e}");
            error!("This could indicate:");
            error!("  - Invalid bot token format");
            error!("  - Network issues reaching Discord API");
            error!("  - Insufficient permissions");
            anyhow::anyhow!("Client creation failed: {}", e)
        })?;

    info!("Bot configured successfully. Connecting to Discord gateway...");

    // Start the reminder scheduler
    let scheduler =
        ReminderScheduler::new(database.clone(), config.openai_model.clone(), usage_tracker);
    let http = client.cache_and_http.http.clone();
    tokio::spawn(async move {
        scheduler.run(http).await;
    });

    // Start the system metrics collection task
    let metrics_db = Arc::new(database);
    let db_path = config.database_path.clone();
    tokio::spawn(async move {
        metrics_collection_loop(metrics_db, db_path).await;
    });

    // Log gateway connection attempt
    info!("Establishing WebSocket connection to Discord gateway...");
    info!("Gateway intents: {intents:?}");

    if let Err(why) = client.start().await {
        error!("Gateway connection failed: {why:?}");
        error!("This could be due to:");
        error!("  - Invalid bot token");
        error!("  - Network connectivity issues");
        error!("  - Discord API outage");
        error!("  - Missing required permissions");
        return Err(anyhow::anyhow!(
            "Failed to establish gateway connection: {}",
            why
        ));
    }

    Ok(())
}
