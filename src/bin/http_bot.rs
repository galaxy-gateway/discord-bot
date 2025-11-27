use anyhow::Result;
use dotenvy::dotenv;
use log::{info, error};

use persona::core::Config;
use persona::database::Database;
use persona::commands::CommandHandler;
use persona::http_server::start_http_server;
use persona::features::analytics::{InteractionTracker, UsageTracker};

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenv().ok();

    // Initialize logging
    env_logger::init();

    info!("🚀 Starting Discord Bot HTTP Server...");

    // Load configuration
    let config = Config::from_env()?;
    info!("✅ Configuration loaded");

    // Ensure OPENAI_API_KEY is set in environment for the openai crate
    // Set both OPENAI_API_KEY and OPENAI_KEY for compatibility
    std::env::set_var("OPENAI_API_KEY", &config.openai_api_key);
    std::env::set_var("OPENAI_KEY", &config.openai_api_key);

    // Initialize database
    let database = Database::new(&config.database_path).await?;
    info!("✅ Database connected");

    // Create usage tracker
    let usage_tracker = UsageTracker::new(database.clone());
    let interaction_tracker = InteractionTracker::new(database.clone());

    // Create command handler
    let command_handler = CommandHandler::new(
        database,
        config.openai_api_key.clone(),
        config.openai_model.clone(),
        config.conflict_mediation_enabled,
        &config.conflict_sensitivity,
        config.mediation_cooldown_minutes,
        usage_tracker,
        interaction_tracker,
    );

    info!("✅ Command handler initialized");

    // Start HTTP server on port 6666 (matches ngrok configuration)
    let port = 6666;
    info!("🌐 Starting HTTP server on port {port}");
    info!("📡 Interactions endpoint: https://0fbf2d802093.ngrok-free.app/interactions");
    
    if let Err(e) = start_http_server(config, command_handler, port).await {
        error!("❌ HTTP server failed: {e}");
        return Err(e);
    }

    Ok(())
}