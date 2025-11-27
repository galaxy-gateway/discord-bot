use anyhow::Result;
use dotenvy::dotenv;
use log::{error, info};
use serenity::async_trait;
use serenity::model::application::interaction::Interaction;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::sync::Arc;

use persona::commands::{CommandHandler, register_global_commands, register_guild_commands};
use persona::core::Config;
use persona::database::Database;
use persona::features::analytics::{InteractionTracker, UsageTracker, metrics_collection_loop};
use persona::features::personas::PersonaManager;
use persona::features::reminders::ReminderScheduler;
use persona::features::startup::StartupNotifier;
use persona::message_components::MessageComponentHandler;
use serenity::model::id::GuildId;

struct Handler {
    command_handler: Arc<CommandHandler>,
    component_handler: Arc<MessageComponentHandler>,
    guild_id: Option<GuildId>,
    startup_notifier: StartupNotifier,
}

impl Handler {
    fn new(
        command_handler: CommandHandler,
        component_handler: MessageComponentHandler,
        guild_id: Option<GuildId>,
        startup_notifier: StartupNotifier,
    ) -> Self {
        Handler {
            command_handler: Arc::new(command_handler),
            component_handler: Arc::new(component_handler),
            guild_id,
            startup_notifier,
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        if let Err(e) = self.command_handler.handle_message(&ctx, &msg).await {
            error!("Error handling message: {e}");
            if let Err(why) = msg
                .channel_id
                .say(&ctx.http, "Sorry, I encountered an error processing your message.")
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

        // Register slash commands - use guild commands for development (instant), global for production
        if let Some(guild_id) = self.guild_id {
            info!("🔧 Development mode: Registering commands for guild {guild_id}");
            if let Err(e) = register_guild_commands(&ctx, guild_id).await {
                error!("❌ Failed to register guild slash commands: {e}");
            } else {
                info!("✅ Successfully registered slash commands for guild {guild_id} (instant update)");
            }
        } else {
            info!("🌍 Production mode: Registering commands globally");
            if let Err(e) = register_global_commands(&ctx).await {
                error!("❌ Failed to register global slash commands: {e}");
            } else {
                info!("✅ Successfully registered slash commands globally (may take up to 1 hour to propagate)");
            }
        }

        // Send startup notification if enabled
        self.startup_notifier.send_if_enabled(&ctx.http, &ready).await;
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::ApplicationCommand(command) => {
                if let Err(e) = self.command_handler.handle_slash_command(&ctx, &command).await {
                    error!("Error handling slash command '{}': {}", command.data.name, e);
                    
                    // Try to edit the deferred response with error message
                    let error_message = if e.to_string().contains("timeout") || e.to_string().contains("OpenAI") {
                        "⏱️ Sorry, the AI service is taking longer than expected. Please try again in a moment."
                    } else {
                        "❌ Sorry, I encountered an error processing your command. Please try again."
                    };
                    
                    // Try to edit the deferred response, fallback to new response if that fails
                    #[allow(clippy::redundant_pattern_matching)]
                    if let Err(_) = command.edit_original_interaction_response(&ctx.http, |response| {
                        response.content(error_message)
                    }).await {
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
                if let Err(e) = self.component_handler.handle_component_interaction(&ctx, &component).await {
                    error!("Error handling component interaction '{}': {}", component.data.custom_id, e);
                    
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
                if let Err(e) = self.component_handler.handle_modal_submit(&ctx, &modal).await {
                    error!("Error handling modal submit '{}': {}", modal.data.custom_id, e);
                    
                    let error_message = if e.to_string().contains("timeout") || e.to_string().contains("OpenAI") {
                        "⏱️ Sorry, the AI service is taking longer than expected. Please try again in a moment."
                    } else {
                        "❌ Sorry, I encountered an error processing your submission. Please try again."
                    };
                    
                    // Try to edit the deferred response, fallback to new response if that fails
                    #[allow(clippy::redundant_pattern_matching)]
                    if let Err(_) = modal.edit_original_interaction_response(&ctx.http, |response| {
                        response.content(error_message)
                    }).await {
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
                info!("Autocomplete interaction received for command: {}", autocomplete.data.name);

                // Handle autocomplete based on command
                let _ = match autocomplete.data.name.as_str() {
                    "set_guild_setting" => {
                        // Get the setting option to determine which choices to show
                        let setting = autocomplete.data.options.iter()
                            .find(|opt| opt.name == "setting")
                            .and_then(|opt| opt.value.as_ref())
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        autocomplete
                            .create_autocomplete_response(&ctx.http, |response| {
                                match setting {
                                    "default_verbosity" => {
                                        response
                                            .add_string_choice("concise - Brief responses (2-3 sentences)", "concise")
                                            .add_string_choice("normal - Balanced responses", "normal")
                                            .add_string_choice("detailed - Comprehensive responses", "detailed")
                                    }
                                    "default_persona" => {
                                        response
                                            .add_string_choice("obi - Obi-Wan Kenobi (wise mentor)", "obi")
                                            .add_string_choice("muppet - Enthusiastic Muppet expert", "muppet")
                                            .add_string_choice("chef - Passionate cooking expert", "chef")
                                            .add_string_choice("teacher - Patient educator", "teacher")
                                            .add_string_choice("analyst - Step-by-step analyst", "analyst")
                                    }
                                    "conflict_mediation" => {
                                        response
                                            .add_string_choice("enabled - Bot will mediate conflicts", "enabled")
                                            .add_string_choice("disabled - No conflict mediation", "disabled")
                                    }
                                    "conflict_sensitivity" => {
                                        response
                                            .add_string_choice("low - Only obvious conflicts (0.7 threshold)", "low")
                                            .add_string_choice("medium - Balanced detection (0.5 threshold)", "medium")
                                            .add_string_choice("high - More sensitive (0.35 threshold)", "high")
                                            .add_string_choice("ultra - Maximum sensitivity (0.3 threshold)", "ultra")
                                    }
                                    "mediation_cooldown" => {
                                        response
                                            .add_string_choice("1 minute", "1")
                                            .add_string_choice("5 minutes (default)", "5")
                                            .add_string_choice("10 minutes", "10")
                                            .add_string_choice("15 minutes", "15")
                                            .add_string_choice("30 minutes", "30")
                                            .add_string_choice("60 minutes", "60")
                                    }
                                    "max_context_messages" => {
                                        response
                                            .add_string_choice("10 messages (minimal context)", "10")
                                            .add_string_choice("20 messages (light context)", "20")
                                            .add_string_choice("40 messages (default)", "40")
                                            .add_string_choice("60 messages (extended context)", "60")
                                    }
                                    "audio_transcription" => {
                                        response
                                            .add_string_choice("enabled - Transcribe audio files", "enabled")
                                            .add_string_choice("disabled - Skip audio processing", "disabled")
                                    }
                                    "audio_transcription_mode" => {
                                        response
                                            .add_string_choice("always - Transcribe all audio files", "always")
                                            .add_string_choice("mention_only - Only when @mentioned", "mention_only")
                                    }
                                    "audio_transcription_output" => {
                                        response
                                            .add_string_choice("transcription_only - Just the transcription", "transcription_only")
                                            .add_string_choice("with_commentary - Add AI commentary", "with_commentary")
                                    }
                                    "mention_responses" => {
                                        response
                                            .add_string_choice("enabled - Respond when @mentioned", "enabled")
                                            .add_string_choice("disabled - Ignore mentions", "disabled")
                                    }
                                    // Startup notification settings (global)
                                    "startup_notification" => {
                                        response
                                            .add_string_choice("enabled - Send notification on startup", "enabled")
                                            .add_string_choice("disabled - No startup notification", "disabled")
                                    }
                                    // For ID fields, don't show autocomplete - user must type the ID directly
                                    // Return empty response so Discord shows the text input
                                    "startup_notify_owner_id" | "startup_notify_channel_id" => response,
                                    _ => response
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

    let database = Database::new(&config.database_path).await?;
    let usage_tracker = UsageTracker::new(database.clone());
    let interaction_tracker = InteractionTracker::new(database.clone());
    let persona_manager = PersonaManager::new();
    let command_handler = CommandHandler::new(
        database.clone(),
        config.openai_api_key.clone(),
        config.openai_model.clone(),
        config.conflict_mediation_enabled,
        &config.conflict_sensitivity,
        config.mediation_cooldown_minutes,
        usage_tracker.clone(),
        interaction_tracker,
    );
    let component_handler = MessageComponentHandler::new(
        command_handler.clone(),
        persona_manager,
        database.clone()
    );

    // Parse guild ID if provided for development mode
    let guild_id = config.discord_guild_id.as_ref().and_then(|id| id.parse::<u64>().ok()).map(GuildId);

    // Create startup notifier (reads config from database)
    let startup_notifier = StartupNotifier::new(Arc::new(database.clone()));

    let handler = Handler::new(command_handler, component_handler, guild_id, startup_notifier);

    let intents = GatewayIntents::GUILD_MESSAGES
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
    let scheduler = ReminderScheduler::new(database.clone(), config.openai_model.clone(), usage_tracker);
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
        return Err(anyhow::anyhow!("Failed to establish gateway connection: {}", why));
    }

    Ok(())
}

