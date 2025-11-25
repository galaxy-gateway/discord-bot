use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;
use anyhow::{anyhow, Result};
use ed25519_dalek::{VerifyingKey, Signature, Verifier};
use hex;
use uuid;
use log::{info, error, warn, debug};

use crate::commands::CommandHandler;
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub command_handler: CommandHandler,
    pub public_key: VerifyingKey,
}

#[derive(Deserialize)]
pub struct InteractionPayload {
    #[serde(rename = "type")]
    pub interaction_type: u8,
    pub data: Option<Value>,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub member: Option<Value>,
    pub user: Option<Value>,
    pub token: String,
    pub id: String,
    pub application_id: String,
    pub version: Option<u8>,
}

#[derive(Serialize)]
pub struct InteractionResponse {
    #[serde(rename = "type")]
    pub response_type: u8,
    pub data: Option<Value>,
}

// Discord interaction types
const PING: u8 = 1;
const APPLICATION_COMMAND: u8 = 2;
const MESSAGE_COMPONENT: u8 = 3;
const APPLICATION_COMMAND_AUTOCOMPLETE: u8 = 4;
const MODAL_SUBMIT: u8 = 5;

// Discord interaction response types
const PONG: u8 = 1;
const CHANNEL_MESSAGE_WITH_SOURCE: u8 = 4;
const DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE: u8 = 5;
#[allow(dead_code)] // Reserved for future component update features
const DEFERRED_UPDATE_MESSAGE: u8 = 6;
#[allow(dead_code)] // Reserved for future message update features
const UPDATE_MESSAGE: u8 = 7;
const APPLICATION_COMMAND_AUTOCOMPLETE_RESULT: u8 = 8;
#[allow(dead_code)] // Reserved for future modal dialog features
const MODAL: u8 = 9;

pub async fn create_server(config: &Config, command_handler: CommandHandler) -> Result<Router> {
    // Parse Discord public key for signature verification
    info!("🔑 Loading Discord public key for signature verification");
    let discord_public_key = config.discord_public_key.as_ref()
        .ok_or_else(|| anyhow!("DISCORD_PUBLIC_KEY environment variable is required for HTTP interactions"))?;
    
    debug!("📋 Discord public key from config: {discord_public_key}");
    debug!("📏 Public key length: {} characters", discord_public_key.len());
    
    info!("🔓 Decoding public key from hex");
    let public_key_bytes = hex::decode(discord_public_key)
        .map_err(|e| {
            error!("❌ Failed to decode Discord public key as hex: {e}");
            anyhow!("Failed to decode Discord public key: {}", e)
        })?;
    
    debug!("📏 Decoded public key bytes length: {}", public_key_bytes.len());
    let public_key_len = public_key_bytes.len();
    
    info!("🔐 Creating VerifyingKey from decoded bytes");
    let public_key = VerifyingKey::from_bytes(&public_key_bytes.try_into()
        .map_err(|_| {
            error!("❌ Public key must be exactly 32 bytes, got {public_key_len}");
            anyhow!("Public key must be 32 bytes")
        })?)
        .map_err(|e| {
            error!("❌ Invalid Discord public key format: {e}");
            anyhow!("Invalid Discord public key: {}", e)
        })?;
    
    info!("✅ Discord public key loaded and validated successfully");

    let state = AppState {
        command_handler,
        public_key,
    };

    let app = Router::new()
        .route("/", get(health_check))
        .route("/interactions", post(handle_interaction))
        .layer(CorsLayer::permissive())
        .with_state(state);

    Ok(app)
}

async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "message": "Discord Bot HTTP Server is running",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn handle_interaction(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<InteractionResponse>, StatusCode> {
    let request_id = uuid::Uuid::new_v4();
    
    info!("[{}] 📥 HTTP interaction received | Body length: {} | Headers: {:?}", 
          request_id, body.len(), 
          headers.iter().map(|(k, v)| format!("{}={}", k, v.to_str().unwrap_or("<invalid>"))).collect::<Vec<_>>().join(", "));
    
    debug!("[{}] 📝 Request body: {}", request_id,
           if body.len() > 500 {
               format!("{}...", String::from_utf8_lossy(&body[..500]))
           } else {
               String::from_utf8_lossy(&body).to_string()
           });
    
    // Verify Discord signature
    info!("[{request_id}] 🔐 Starting Discord signature verification");
    if let Err(e) = verify_discord_signature(&state.public_key, &headers, &body, request_id) {
        error!("[{request_id}] ❌ Signature verification failed: {e}");
        error!("[{request_id}] 🚫 Returning 401 Unauthorized to Discord");
        return Err(StatusCode::UNAUTHORIZED);
    }
    info!("[{request_id}] ✅ Discord signature verification passed");

    // Parse interaction payload
    let interaction: InteractionPayload = serde_json::from_slice(&body)
        .map_err(|e| {
            error!("Failed to parse interaction payload: {e}");
            StatusCode::BAD_REQUEST
        })?;

    debug!("Received interaction type: {}", interaction.interaction_type);

    match interaction.interaction_type {
        PING => {
            info!("Received ping interaction");
            Ok(Json(InteractionResponse {
                response_type: PONG,
                data: None,
            }))
        }
        APPLICATION_COMMAND => {
            info!("Received application command interaction");
            handle_application_command(interaction, state).await
        }
        MESSAGE_COMPONENT => {
            info!("Received message component interaction");
            handle_message_component(interaction, state).await
        }
        APPLICATION_COMMAND_AUTOCOMPLETE => {
            info!("Received autocomplete interaction");
            handle_autocomplete(interaction, state).await
        }
        MODAL_SUBMIT => {
            info!("Received modal submit interaction");
            handle_modal_submit(interaction, state).await
        }
        _ => {
            warn!("Unknown interaction type: {}", interaction.interaction_type);
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

fn verify_discord_signature(
    public_key: &VerifyingKey,
    headers: &HeaderMap,
    body: &[u8],
    request_id: uuid::Uuid,
) -> Result<()> {
    debug!("[{request_id}] 🔍 Looking for signature headers");
    
    let signature_header = headers
        .get("x-signature-ed25519")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            error!("[{request_id}] ❌ Missing x-signature-ed25519 header");
            anyhow!("Missing signature header")
        })?;

    let timestamp_header = headers
        .get("x-signature-timestamp")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            error!("[{request_id}] ❌ Missing x-signature-timestamp header");
            anyhow!("Missing timestamp header")
        })?;

    debug!("[{request_id}] 📋 Found headers | Signature: {signature_header} | Timestamp: {timestamp_header}");

    debug!("[{request_id}] 🔓 Decoding signature from hex");
    let signature_bytes = hex::decode(signature_header)
        .map_err(|e| {
            error!("[{request_id}] ❌ Invalid signature hex format: {e}");
            anyhow!("Invalid signature format: {}", e)
        })?;

    debug!("[{}] 📏 Signature bytes length: {}", request_id, signature_bytes.len());
    let signature_bytes_len = signature_bytes.len();
    let signature_array: [u8; 64] = signature_bytes.try_into()
        .map_err(|_| {
            error!("[{request_id}] ❌ Signature must be exactly 64 bytes, got {signature_bytes_len}");
            anyhow!("Signature must be 64 bytes")
        })?;
    let signature = Signature::from_bytes(&signature_array);

    let message = [timestamp_header.as_bytes(), body].concat();
    debug!("[{}] 📝 Verification message: timestamp({}) + body({} bytes) = {} bytes total",
           request_id, timestamp_header, body.len(), message.len());
    debug!("[{}] 📝 First 100 bytes of verification message: '{}'",
           request_id, String::from_utf8_lossy(&message[..message.len().min(100)]));

    debug!("[{request_id}] 🔐 Performing ed25519 signature verification");
    public_key
        .verify(&message, &signature)
        .map_err(|e| {
            error!("[{request_id}] ❌ ed25519 signature verification failed: {e}");
            error!("[{request_id}] 🔑 Public key being used: {public_key:?}");
            error!("[{}] 📝 Message being verified: '{}'", request_id,
                   String::from_utf8_lossy(if message.len() > 200 { &message[..200] } else { &message }));
            error!("[{}] ✍️ Signature: {}", request_id, hex::encode(signature_array));
            anyhow!("Signature verification failed: {}", e)
        })?;

    debug!("[{request_id}] ✅ ed25519 signature verification successful");
    Ok(())
}

async fn handle_application_command(
    interaction: InteractionPayload,
    _state: AppState,
) -> Result<Json<InteractionResponse>, StatusCode> {
    // For AI-powered commands, defer the response
    let command_name = interaction
        .data
        .as_ref()
        .and_then(|d| d.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("unknown");

    match command_name {
        "ping" | "help" | "personas" => {
            // Quick responses - respond immediately
            let response_data = match command_name {
                "ping" => json!({"content": "🏓 Pong! HTTP interactions are working!"}),
                "help" => json!({"content": "📚 **Available Commands:**\n- `/ping` - Test bot responsiveness\n- `/hey <message>` - Chat with your persona\n- `/personas` - List available personas\n- `/set_persona <persona>` - Set your default persona"}),
                "personas" => json!({"content": "🎭 **Available Personas:**\n- 🐸 **muppet** - Enthusiastic Muppet expert\n- 👨‍🍳 **chef** - Passionate cooking expert\n- 👩‍🏫 **teacher** - Patient educator\n- 📊 **analyst** - Step-by-step analyst"}),
                _ => json!({"content": "Unknown command"}),
            };

            Ok(Json(InteractionResponse {
                response_type: CHANNEL_MESSAGE_WITH_SOURCE,
                data: Some(response_data),
            }))
        }
        _ => {
            // AI-powered commands - defer response
            info!("Deferring response for AI command: {command_name}");
            
            // TODO: Implement actual command processing with follow-up
            // For now, just defer and the bot will need to edit the response later
            
            Ok(Json(InteractionResponse {
                response_type: DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE,
                data: None,
            }))
        }
    }
}

async fn handle_message_component(
    interaction: InteractionPayload,
    _state: AppState,
) -> Result<Json<InteractionResponse>, StatusCode> {
    let custom_id = interaction
        .data
        .as_ref()
        .and_then(|d| d.get("custom_id"))
        .and_then(|id| id.as_str())
        .unwrap_or("unknown");

    info!("Handling message component: {custom_id}");

    let response_content = match custom_id {
        id if id.starts_with("persona_") => {
            let persona = id.strip_prefix("persona_").unwrap_or("muppet");
            format!("🎭 Persona switched to: **{persona}**")
        }
        "help_detailed" => "📚 **Detailed Help:**\n\nThis bot provides AI-powered conversations through different personas. Use slash commands to interact!".to_string(),
        _ => "Button clicked!".to_string(),
    };

    Ok(Json(InteractionResponse {
        response_type: CHANNEL_MESSAGE_WITH_SOURCE,
        data: Some(json!({"content": response_content})),
    }))
}

async fn handle_autocomplete(
    interaction: InteractionPayload,
    _state: AppState,
) -> Result<Json<InteractionResponse>, StatusCode> {
    // Get command name and options from the interaction
    let command_name = interaction.data.as_ref()
        .and_then(|d| d.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");

    let choices = match command_name {
        "set_guild_setting" => {
            // Get the setting option to determine which choices to show
            let setting = interaction.data.as_ref()
                .and_then(|d| d.get("options"))
                .and_then(|o| o.as_array())
                .and_then(|opts| opts.iter().find(|opt| {
                    opt.get("name").and_then(|n| n.as_str()) == Some("setting")
                }))
                .and_then(|opt| opt.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match setting {
                "default_verbosity" => json!([
                    {"name": "concise - Brief responses (2-3 sentences)", "value": "concise"},
                    {"name": "normal - Balanced responses", "value": "normal"},
                    {"name": "detailed - Comprehensive responses", "value": "detailed"}
                ]),
                "default_persona" => json!([
                    {"name": "obi - Obi-Wan Kenobi (wise mentor)", "value": "obi"},
                    {"name": "muppet - Enthusiastic Muppet expert", "value": "muppet"},
                    {"name": "chef - Passionate cooking expert", "value": "chef"},
                    {"name": "teacher - Patient educator", "value": "teacher"},
                    {"name": "analyst - Step-by-step analyst", "value": "analyst"}
                ]),
                "conflict_mediation" => json!([
                    {"name": "enabled - Bot will mediate conflicts", "value": "enabled"},
                    {"name": "disabled - No conflict mediation", "value": "disabled"}
                ]),
                "conflict_sensitivity" => json!([
                    {"name": "low - Only obvious conflicts (0.7 threshold)", "value": "low"},
                    {"name": "medium - Balanced detection (0.5 threshold)", "value": "medium"},
                    {"name": "high - More sensitive (0.35 threshold)", "value": "high"},
                    {"name": "ultra - Maximum sensitivity (0.3 threshold)", "value": "ultra"}
                ]),
                "mediation_cooldown" => json!([
                    {"name": "1 minute", "value": "1"},
                    {"name": "5 minutes (default)", "value": "5"},
                    {"name": "10 minutes", "value": "10"},
                    {"name": "15 minutes", "value": "15"},
                    {"name": "30 minutes", "value": "30"},
                    {"name": "60 minutes", "value": "60"}
                ]),
                "max_context_messages" => json!([
                    {"name": "10 messages (minimal context)", "value": "10"},
                    {"name": "20 messages (light context)", "value": "20"},
                    {"name": "40 messages (default)", "value": "40"},
                    {"name": "60 messages (extended context)", "value": "60"}
                ]),
                "audio_transcription" => json!([
                    {"name": "enabled - Transcribe audio files", "value": "enabled"},
                    {"name": "disabled - Skip audio processing", "value": "disabled"}
                ]),
                "audio_transcription_mode" => json!([
                    {"name": "always - Transcribe all audio files", "value": "always"},
                    {"name": "mention_only - Only when @mentioned", "value": "mention_only"},
                    {"name": "disabled - Never transcribe", "value": "disabled"}
                ]),
                "audio_transcription_output" => json!([
                    {"name": "transcription_only - Just the transcription", "value": "transcription_only"},
                    {"name": "with_commentary - Add AI commentary", "value": "with_commentary"}
                ]),
                "mention_responses" => json!([
                    {"name": "enabled - Respond when @mentioned", "value": "enabled"},
                    {"name": "disabled - Ignore mentions", "value": "disabled"}
                ]),
                _ => json!([])
            }
        }
        _ => json!([])
    };

    Ok(Json(InteractionResponse {
        response_type: APPLICATION_COMMAND_AUTOCOMPLETE_RESULT,
        data: Some(json!({"choices": choices})),
    }))
}

async fn handle_modal_submit(
    _interaction: InteractionPayload,
    _state: AppState,
) -> Result<Json<InteractionResponse>, StatusCode> {
    info!("Handling modal submit");
    
    // Defer response for modal processing
    Ok(Json(InteractionResponse {
        response_type: DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE,
        data: None,
    }))
}

pub async fn start_http_server(
    config: Config,
    command_handler: CommandHandler,
    port: u16,
) -> Result<()> {
    let app = create_server(&config, command_handler).await?;
    
    let listener = tokio::net::TcpListener::bind(&format!("0.0.0.0:{port}"))
        .await
        .map_err(|e| anyhow!("Failed to bind to port {}: {}", port, e))?;

    info!("HTTP server starting on port {port}");
    info!("Interactions endpoint: http://0.0.0.0:{port}/interactions");
    
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow!("HTTP server error: {}", e))?;

    Ok(())
}