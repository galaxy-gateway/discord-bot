//! # IPC Server
//!
//! Unix socket server for the bot to communicate with TUI clients.
//!
//! - **Version**: 1.6.0
//! - **Since**: 3.17.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.6.0: Added GetChannelsWithHistory handler, fixed username lookup in GetChannelHistory
//! - 1.5.0: Implemented SetFeature, SetGuildSetting, and SetChannelPersona handlers
//! - 1.4.0: Added cache_user method and TopUser struct support for username resolution
//! - 1.3.0: Added SendMessage implementation with Discord HTTP client support
//! - 1.2.0: Added GetUsageStats and GetSystemMetrics commands with database integration
//! - 1.1.0: Added command processing support and shared state for guilds/bot info
//! - 1.0.0: Initial IPC implementation with Unix socket protocol

use crate::database::Database;
use crate::ipc::get_socket_path;
use crate::ipc::protocol::{
    encode_message, BotEvent, ChannelHistorySummary, DisplayMessage, DmSessionInfo, ErrorInfo,
    GuildInfo, TopUser, TuiCommand, UserStats, UserSummary,
};
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use log::{debug, error, info, warn};
use serenity::http::Http;
use serenity::model::id::ChannelId;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, RwLock};

/// Maximum number of connected TUI clients
const MAX_CLIENTS: usize = 10;

/// Broadcast channel capacity for events
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Command channel capacity per client
const COMMAND_CHANNEL_CAPACITY: usize = 64;

/// IPC Server handle for the bot
#[derive(Clone)]
pub struct IpcServer {
    /// Broadcast sender for events to all clients
    event_tx: broadcast::Sender<BotEvent>,
    /// Receiver for commands from TUI clients
    command_rx: Arc<RwLock<mpsc::Receiver<TuiCommand>>>,
    /// Sender for commands (used by client handlers)
    command_tx: mpsc::Sender<TuiCommand>,
    /// Set of channels being watched
    watched_channels: Arc<RwLock<std::collections::HashSet<u64>>>,
    /// Connected client count
    client_count: Arc<RwLock<usize>>,
    /// Cached guild information (updated on Ready event)
    guilds: Arc<RwLock<Vec<GuildInfo>>>,
    /// Bot user ID
    bot_user_id: Arc<RwLock<Option<u64>>>,
    /// Bot username
    bot_username: Arc<RwLock<Option<String>>>,
    /// Server start time for uptime calculation
    start_time: Instant,
    /// Active conversation sessions count
    active_sessions: Arc<RwLock<usize>>,
    /// Database connection for stats queries
    database: Option<Database>,
    /// Database file path for size queries
    db_path: Option<String>,
    /// Discord HTTP client for sending messages
    http: Arc<RwLock<Option<Arc<Http>>>>,
}

impl IpcServer {
    /// Create a new IPC server (does not start listening yet)
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);

        IpcServer {
            event_tx,
            command_rx: Arc::new(RwLock::new(command_rx)),
            command_tx,
            watched_channels: Arc::new(RwLock::new(std::collections::HashSet::new())),
            client_count: Arc::new(RwLock::new(0)),
            guilds: Arc::new(RwLock::new(Vec::new())),
            bot_user_id: Arc::new(RwLock::new(None)),
            bot_username: Arc::new(RwLock::new(None)),
            start_time: Instant::now(),
            active_sessions: Arc::new(RwLock::new(0)),
            database: None,
            db_path: None,
            http: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the database connection for stats queries
    pub fn with_database(mut self, database: Database, db_path: String) -> Self {
        self.database = Some(database);
        self.db_path = Some(db_path);
        self
    }

    /// Set the Discord HTTP client for sending messages
    pub async fn set_http(&self, http: Arc<Http>) {
        let mut http_lock = self.http.write().await;
        *http_lock = Some(http);
        info!("IPC server HTTP client configured");
    }

    /// Start the IPC server in a background task
    pub async fn start(self: Arc<Self>) -> Result<()> {
        let socket_path = get_socket_path();

        // Remove existing socket file if it exists
        if std::path::Path::new(&socket_path).exists() {
            std::fs::remove_file(&socket_path)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        info!("IPC server listening on {socket_path}");

        // Spawn the accept loop
        let server = self.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let client_count = *server.client_count.read().await;
                        if client_count >= MAX_CLIENTS {
                            warn!(
                                "Maximum IPC clients reached ({MAX_CLIENTS}), rejecting connection"
                            );
                            continue;
                        }

                        *server.client_count.write().await += 1;
                        info!("TUI client connected (total: {})", client_count + 1);

                        let server_clone = server.clone();
                        let client_count_ref = server.client_count.clone();
                        tokio::spawn(async move {
                            if let Err(e) = server_clone.handle_client(stream).await {
                                debug!("Client handler ended: {e}");
                            }
                            *client_count_ref.write().await -= 1;
                            info!("TUI client disconnected");
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept IPC connection: {e}");
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle a connected client
    async fn handle_client(self: Arc<Self>, stream: UnixStream) -> Result<()> {
        let (mut reader, mut writer) = stream.into_split();

        // Subscribe to event broadcast
        let mut event_rx = self.event_tx.subscribe();

        // Spawn writer task for events
        let write_handle = tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(event) => match encode_message(&event) {
                        Ok(data) => {
                            if let Err(e) = writer.write_all(&data).await {
                                debug!("Failed to write to client: {e}");
                                break;
                            }
                            if let Err(e) = writer.flush().await {
                                debug!("Failed to flush to client: {e}");
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to encode event: {e}");
                        }
                    },
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Client lagged behind by {n} events");
                    }
                }
            }
        });

        // Read commands from client
        let command_tx = self.command_tx.clone();
        let watched_channels = self.watched_channels.clone();

        loop {
            // Read length prefix
            let mut len_buf = [0u8; 4];
            if reader.read_exact(&mut len_buf).await.is_err() {
                break;
            }
            let len = u32::from_be_bytes(len_buf) as usize;

            if len > 10 * 1024 * 1024 {
                error!("Message too large from client: {len} bytes");
                break;
            }

            // Read message body
            let mut buf = vec![0u8; len];
            if reader.read_exact(&mut buf).await.is_err() {
                break;
            }

            // Parse command
            match serde_json::from_slice::<TuiCommand>(&buf) {
                Ok(cmd) => {
                    // Handle watch/unwatch locally
                    match &cmd {
                        TuiCommand::WatchChannel { channel_id } => {
                            watched_channels.write().await.insert(*channel_id);
                            debug!("Now watching channel {channel_id}");
                        }
                        TuiCommand::UnwatchChannel { channel_id } => {
                            watched_channels.write().await.remove(channel_id);
                            debug!("Stopped watching channel {channel_id}");
                        }
                        _ => {}
                    }

                    // Forward command to bot
                    if let Err(e) = command_tx.send(cmd).await {
                        error!("Failed to forward command: {e}");
                        break;
                    }
                }
                Err(e) => {
                    warn!("Failed to parse command from client: {e}");
                }
            }
        }

        write_handle.abort();
        Ok(())
    }

    /// Broadcast an event to all connected TUI clients
    pub fn broadcast(&self, event: BotEvent) {
        // Only send message events for watched channels
        if let BotEvent::MessageCreate { channel_id, .. } = &event {
            // Use try_read to avoid blocking
            if let Ok(watched) = self.watched_channels.try_read() {
                if !watched.contains(channel_id) {
                    return;
                }
            }
        }

        let _ = self.event_tx.send(event);
    }

    /// Check if a channel is being watched
    pub async fn is_channel_watched(&self, channel_id: u64) -> bool {
        self.watched_channels.read().await.contains(&channel_id)
    }

    /// Try to receive a command (non-blocking)
    pub async fn try_recv_command(&self) -> Option<TuiCommand> {
        self.command_rx.write().await.try_recv().ok()
    }

    /// Receive a command (blocking)
    pub async fn recv_command(&self) -> Option<TuiCommand> {
        self.command_rx.write().await.recv().await
    }

    /// Get connected client count
    pub async fn client_count(&self) -> usize {
        *self.client_count.read().await
    }

    /// Send a heartbeat to all clients
    pub fn send_heartbeat(&self) {
        let timestamp = chrono::Utc::now().timestamp();
        self.broadcast(BotEvent::Heartbeat { timestamp });
    }

    /// Update cached guild information (call this on Ready event)
    pub async fn set_guilds(&self, guilds: Vec<GuildInfo>) {
        *self.guilds.write().await = guilds;
    }

    /// Get cached guild information
    pub async fn get_guilds(&self) -> Vec<GuildInfo> {
        self.guilds.read().await.clone()
    }

    /// Update bot user information (call this on Ready event)
    pub async fn set_bot_info(&self, user_id: u64, username: String) {
        *self.bot_user_id.write().await = Some(user_id);
        *self.bot_username.write().await = Some(username);
    }

    /// Get bot user ID
    pub async fn get_bot_user_id(&self) -> Option<u64> {
        *self.bot_user_id.read().await
    }

    /// Get bot username
    pub async fn get_bot_username(&self) -> Option<String> {
        self.bot_username.read().await.clone()
    }

    /// Get uptime in seconds
    pub fn get_uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Update active session count
    pub async fn set_active_sessions(&self, count: usize) {
        *self.active_sessions.write().await = count;
    }

    /// Get active session count
    pub async fn get_active_sessions(&self) -> usize {
        *self.active_sessions.read().await
    }

    /// Add a watched channel
    pub async fn add_watched_channel(&self, channel_id: u64) {
        self.watched_channels.write().await.insert(channel_id);
        debug!("Added watched channel {channel_id}");
    }

    /// Remove a watched channel
    pub async fn remove_watched_channel(&self, channel_id: u64) {
        self.watched_channels.write().await.remove(&channel_id);
        debug!("Removed watched channel {channel_id}");
    }

    /// Get list of watched channels
    pub async fn get_watched_channels(&self) -> Vec<u64> {
        self.watched_channels.read().await.iter().copied().collect()
    }

    /// Cache a Discord user's information for TUI display
    pub async fn cache_user(&self, user_id: u64, username: &str, discriminator: u16, is_bot: bool) {
        if let Some(ref db) = self.database {
            if let Err(e) = db
                .cache_user(user_id, username, discriminator, is_bot)
                .await
            {
                warn!("Failed to cache user {user_id}: {e}");
            }
        }
    }

    /// Process a single TUI command and generate appropriate response
    pub async fn process_command(&self, cmd: TuiCommand) {
        match cmd {
            TuiCommand::GetStatus => {
                let guild_count = self.guilds.read().await.len();
                let active_sessions = self.get_active_sessions().await;
                self.broadcast(BotEvent::StatusUpdate {
                    connected: true,
                    uptime_seconds: self.get_uptime_seconds(),
                    guild_count,
                    active_sessions,
                });
                debug!("Sent StatusUpdate response");
            }
            TuiCommand::GetGuilds => {
                let guilds = self.get_guilds().await;
                let bot_user_id = self.get_bot_user_id().await.unwrap_or(0);
                let bot_username = self
                    .get_bot_username()
                    .await
                    .unwrap_or_else(|| "Unknown".to_string());
                self.broadcast(BotEvent::Ready {
                    guilds,
                    bot_user_id,
                    bot_username,
                });
                debug!("Sent Ready response with guild info");
            }
            TuiCommand::WatchChannel { channel_id } => {
                // Already handled in handle_client, but log it
                debug!("WatchChannel command processed for channel {channel_id}");
            }
            TuiCommand::UnwatchChannel { channel_id } => {
                // Already handled in handle_client, but log it
                debug!(
                    "UnwatchChannel command processed for channel {channel_id}"
                );
            }
            TuiCommand::Pong { timestamp } => {
                debug!("Received Pong with timestamp {timestamp}");
            }
            TuiCommand::SendMessage {
                request_id,
                channel_id,
                content,
            } => {
                let http_guard = self.http.read().await;
                if let Some(http) = http_guard.as_ref() {
                    let http = http.clone();
                    drop(http_guard); // Release the lock before async operation

                    let channel = ChannelId(channel_id);
                    match channel.say(&http, &content).await {
                        Ok(msg) => {
                            info!(
                                "Message sent to channel {} (msg id: {})",
                                channel_id, msg.id
                            );
                            self.broadcast(BotEvent::CommandResponse {
                                request_id,
                                success: true,
                                message: Some(format!("Message sent (id: {})", msg.id)),
                                data: None,
                            });
                        }
                        Err(e) => {
                            error!("Failed to send message to channel {channel_id}: {e}");
                            self.broadcast(BotEvent::CommandResponse {
                                request_id,
                                success: false,
                                message: Some(format!("Failed to send message: {e}")),
                                data: None,
                            });
                        }
                    }
                } else {
                    warn!("SendMessage command received but HTTP client not configured");
                    self.broadcast(BotEvent::CommandResponse {
                        request_id,
                        success: false,
                        message: Some(
                            "HTTP client not configured - bot may not be ready".to_string(),
                        ),
                        data: None,
                    });
                }
            }
            TuiCommand::SetFeature {
                request_id,
                feature,
                enabled,
                guild_id,
            } => {
                if let Some(ref db) = self.database {
                    let guild_id_str = guild_id.map(|id| id.to_string());

                    // Set the feature flag
                    match db
                        .set_feature_flag(
                            &feature,
                            enabled,
                            None, // user_id - global toggle
                            guild_id_str.as_deref(),
                        )
                        .await
                    {
                        Ok(_) => {
                            // Record the toggle in audit trail
                            let _ = db
                                .record_feature_toggle(
                                    &feature,
                                    "1.0.0", // version - could look up from FEATURES
                                    guild_id_str.as_deref(),
                                    "TUI", // toggled_by
                                    enabled,
                                )
                                .await;

                            info!(
                                "Feature '{feature}' set to {enabled} for guild {guild_id:?}"
                            );
                            self.broadcast(BotEvent::CommandResponse {
                                request_id,
                                success: true,
                                message: Some(format!(
                                    "Feature '{}' {}",
                                    feature,
                                    if enabled { "enabled" } else { "disabled" }
                                )),
                                data: None,
                            });
                        }
                        Err(e) => {
                            error!("Failed to set feature flag: {e}");
                            self.broadcast(BotEvent::CommandResponse {
                                request_id,
                                success: false,
                                message: Some(format!("Failed to set feature: {e}")),
                                data: None,
                            });
                        }
                    }
                } else {
                    warn!("SetFeature command received but database not configured");
                    self.broadcast(BotEvent::CommandResponse {
                        request_id,
                        success: false,
                        message: Some("Database not configured".to_string()),
                        data: None,
                    });
                }
            }
            TuiCommand::SetChannelPersona {
                request_id,
                guild_id,
                channel_id,
                persona,
            } => {
                if let Some(ref db) = self.database {
                    let persona_opt = if persona.is_empty() {
                        None
                    } else {
                        Some(persona.as_str())
                    };
                    match db
                        .set_channel_persona(
                            &guild_id.to_string(),
                            &channel_id.to_string(),
                            persona_opt,
                        )
                        .await
                    {
                        Ok(_) => {
                            info!(
                                "Channel {channel_id} persona set to '{persona}' in guild {guild_id}"
                            );
                            self.broadcast(BotEvent::CommandResponse {
                                request_id,
                                success: true,
                                message: Some(format!("Channel persona set to '{persona}'")),
                                data: None,
                            });
                        }
                        Err(e) => {
                            error!("Failed to set channel persona: {e}");
                            self.broadcast(BotEvent::CommandResponse {
                                request_id,
                                success: false,
                                message: Some(format!("Failed to set channel persona: {e}")),
                                data: None,
                            });
                        }
                    }
                } else {
                    warn!("SetChannelPersona command received but database not configured");
                    self.broadcast(BotEvent::CommandResponse {
                        request_id,
                        success: false,
                        message: Some("Database not configured".to_string()),
                        data: None,
                    });
                }
            }
            TuiCommand::SetGuildSetting {
                request_id,
                guild_id,
                key,
                value,
            } => {
                if let Some(ref db) = self.database {
                    match db
                        .set_guild_setting(&guild_id.to_string(), &key, &value)
                        .await
                    {
                        Ok(_) => {
                            info!(
                                "Guild setting '{key}' set to '{value}' for guild {guild_id}"
                            );
                            self.broadcast(BotEvent::CommandResponse {
                                request_id,
                                success: true,
                                message: Some(format!("Setting '{key}' updated to '{value}'")),
                                data: None,
                            });
                        }
                        Err(e) => {
                            error!("Failed to set guild setting: {e}");
                            self.broadcast(BotEvent::CommandResponse {
                                request_id,
                                success: false,
                                message: Some(format!("Failed to set setting: {e}")),
                                data: None,
                            });
                        }
                    }
                } else {
                    warn!("SetGuildSetting command received but database not configured");
                    self.broadcast(BotEvent::CommandResponse {
                        request_id,
                        success: false,
                        message: Some("Database not configured".to_string()),
                        data: None,
                    });
                }
            }
            TuiCommand::GetChannelHistory { channel_id, limit } => {
                // Fetch from database conversation_history
                if let Some(ref db) = self.database {
                    match db
                        .get_channel_messages(&channel_id.to_string(), limit)
                        .await
                    {
                        Ok(messages) => {
                            let msg_count = messages.len();
                            let mut display_messages: Vec<DisplayMessage> =
                                Vec::with_capacity(msg_count);

                            for m in messages {
                                let timestamp = NaiveDateTime::parse_from_str(
                                    &m.timestamp,
                                    "%Y-%m-%d %H:%M:%S",
                                )
                                .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                                .unwrap_or_else(|_| Utc::now());

                                // Look up cached username, fallback to user_id
                                let author_name = match db.get_cached_username(&m.user_id).await {
                                    Ok(Some(name)) => name,
                                    _ => m.user_id.clone(),
                                };

                                display_messages.push(DisplayMessage {
                                    id: 0,
                                    author_id: m.user_id.parse().unwrap_or(0),
                                    author_name,
                                    author_discriminator: "0000".to_string(),
                                    content: m.content,
                                    timestamp,
                                    is_bot: m.role == "assistant",
                                    attachments: vec![],
                                    embeds_count: 0,
                                });
                            }

                            self.broadcast(BotEvent::ChannelHistoryResponse {
                                channel_id,
                                messages: display_messages,
                            });
                            debug!("Sent ChannelHistoryResponse with {msg_count} messages");
                        }
                        Err(e) => {
                            warn!("Failed to get channel history: {e}");
                        }
                    }
                } else {
                    warn!("GetChannelHistory command received but no database configured");
                }
            }
            TuiCommand::GetUsageStats { period_days } => {
                if let Some(ref db) = self.database {
                    match db.get_global_usage_stats(period_days).await {
                        Ok((
                            total_cost,
                            period_cost,
                            total_tokens,
                            total_calls,
                            cost_by_service,
                            cost_by_bucket,
                            daily_breakdown,
                            top_users_raw,
                        )) => {
                            // Convert (user_id, username, cost) tuples to TopUser structs
                            let top_users: Vec<TopUser> = top_users_raw
                                .into_iter()
                                .map(|(user_id, username, cost)| TopUser {
                                    user_id,
                                    username,
                                    cost,
                                })
                                .collect();
                            self.broadcast(BotEvent::UsageStatsUpdate {
                                total_cost,
                                period_cost,
                                total_tokens,
                                total_calls,
                                cost_by_service,
                                cost_by_bucket,
                                daily_breakdown,
                                top_users,
                                period_days,
                            });
                            debug!("Sent UsageStatsUpdate response");
                        }
                        Err(e) => {
                            warn!("Failed to get usage stats: {e}");
                        }
                    }
                } else {
                    warn!("GetUsageStats command received but no database configured");
                }
            }
            TuiCommand::GetSystemMetrics => {
                use sysinfo::System;
                let mut sys = System::new();
                // CPU needs two refreshes for accurate reading
                sys.refresh_cpu_usage();
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                sys.refresh_cpu_usage();
                sys.refresh_memory();

                let cpu_percent = sys.global_cpu_usage();
                let memory_bytes = sys.used_memory();
                let memory_total = sys.total_memory();

                // Get database file size
                let db_size = if let Some(ref path) = self.db_path {
                    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                };

                self.broadcast(BotEvent::SystemMetricsUpdate {
                    cpu_percent,
                    memory_bytes,
                    memory_total,
                    db_size,
                    uptime_seconds: self.get_uptime_seconds(),
                });
                debug!("Sent SystemMetricsUpdate response");

                // Also log performance metrics to database for historical tracking
                if let Some(ref db) = self.database {
                    let _ = db
                        .log_performance_metric("cpu", cpu_percent as f64, Some("%"), None)
                        .await;
                    let _ = db
                        .log_performance_metric("memory", memory_bytes as f64, Some("bytes"), None)
                        .await;
                }
            }
            TuiCommand::GetChannelInfo { channel_id } => {
                // Look up channel in cached guilds
                let guilds = self.get_guilds().await;
                let channel_info = guilds
                    .iter()
                    .flat_map(|g| g.channels.iter().map(move |c| (c, g.id, &g.name)))
                    .find(|(c, _, _)| c.id == channel_id);

                if let Some((channel, guild_id, guild_name)) = channel_info {
                    self.broadcast(BotEvent::ChannelInfoResponse {
                        channel_id,
                        name: channel.name.clone(),
                        guild_id: Some(guild_id),
                        guild_name: Some(guild_name.clone()),
                        message_count: 0, // Would need to query Discord
                        last_activity: None,
                    });
                    debug!("Sent ChannelInfoResponse for channel {channel_id}");
                } else {
                    // Send minimal response for unknown channel
                    self.broadcast(BotEvent::ChannelInfoResponse {
                        channel_id,
                        name: format!("{channel_id}"),
                        guild_id: None,
                        guild_name: None,
                        message_count: 0,
                        last_activity: None,
                    });
                }
            }
            TuiCommand::GetHistoricalMetrics { metric_type, hours } => {
                if let Some(ref db) = self.database {
                    match db.get_historical_metrics(&metric_type, hours).await {
                        Ok(data_points) => {
                            self.broadcast(BotEvent::HistoricalMetricsResponse {
                                metric_type,
                                data_points,
                            });
                            debug!("Sent HistoricalMetricsResponse");
                        }
                        Err(e) => {
                            warn!("Failed to get historical metrics: {e}");
                        }
                    }
                }
            }
            TuiCommand::GetUserList { limit } => {
                if let Some(ref db) = self.database {
                    match db.get_user_list(limit).await {
                        Ok(entries) => {
                            let users: Vec<UserSummary> = entries
                                .into_iter()
                                .map(|e| {
                                    let last_activity = e.last_activity.and_then(|s| {
                                        NaiveDateTime::parse_from_str(
                                            &format!("{s} 00:00:00"),
                                            "%Y-%m-%d %H:%M:%S",
                                        )
                                        .map(|dt| {
                                            DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)
                                        })
                                        .ok()
                                    });
                                    UserSummary {
                                        user_id: e.user_id,
                                        username: e.username,
                                        message_count: 0, // Could add if needed
                                        total_cost: e.total_cost,
                                        total_tokens: e.total_tokens,
                                        session_count: e.dm_session_count,
                                        last_activity,
                                    }
                                })
                                .collect();
                            self.broadcast(BotEvent::UserListResponse { users });
                            debug!("Sent UserListResponse");
                        }
                        Err(e) => {
                            warn!("Failed to get user list: {e}");
                        }
                    }
                }
            }
            TuiCommand::GetUserDetails { user_id } => {
                if let Some(ref db) = self.database {
                    match db.get_user_details(&user_id).await {
                        Ok(details) => {
                            let first_seen = details.first_seen.and_then(|s| {
                                NaiveDateTime::parse_from_str(
                                    &format!("{s} 00:00:00"),
                                    "%Y-%m-%d %H:%M:%S",
                                )
                                .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                                .ok()
                            });
                            let last_activity = details.last_activity.and_then(|s| {
                                NaiveDateTime::parse_from_str(
                                    &format!("{s} 00:00:00"),
                                    "%Y-%m-%d %H:%M:%S",
                                )
                                .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                                .ok()
                            });

                            let stats = UserStats {
                                user_id: details.user_id.clone(),
                                username: details.username,
                                message_count: details.message_count,
                                total_cost: details.total_cost,
                                total_tokens: details.total_tokens,
                                total_api_calls: details.total_calls,
                                dm_session_count: details.dm_session_count,
                                chat_calls: details.chat_calls,
                                whisper_calls: details.whisper_calls,
                                dalle_calls: details.dalle_calls,
                                first_seen,
                                last_activity,
                                favorite_persona: details.favorite_persona,
                            };

                            // Also get DM sessions
                            let dm_sessions = match db.get_user_dm_sessions(&user_id, 20).await {
                                Ok(sessions) => sessions
                                    .into_iter()
                                    .map(|s| {
                                        let started_at = NaiveDateTime::parse_from_str(
                                            &s.started_at,
                                            "%Y-%m-%d %H:%M:%S",
                                        )
                                        .map(|dt| {
                                            DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)
                                        })
                                        .unwrap_or_else(|_| Utc::now());
                                        let ended_at = s.ended_at.and_then(|e| {
                                            NaiveDateTime::parse_from_str(&e, "%Y-%m-%d %H:%M:%S")
                                                .map(|dt| {
                                                    DateTime::<Utc>::from_naive_utc_and_offset(
                                                        dt, Utc,
                                                    )
                                                })
                                                .ok()
                                        });
                                        DmSessionInfo {
                                            session_id: s.session_id,
                                            started_at,
                                            ended_at,
                                            message_count: s.message_count,
                                            api_cost: s.api_cost,
                                            total_tokens: s.total_tokens,
                                        }
                                    })
                                    .collect(),
                                Err(_) => vec![],
                            };

                            self.broadcast(BotEvent::UserDetailsResponse {
                                user_id,
                                stats,
                                dm_sessions,
                            });
                            debug!("Sent UserDetailsResponse");
                        }
                        Err(e) => {
                            warn!("Failed to get user details: {e}");
                        }
                    }
                }
            }
            TuiCommand::GetRecentErrors { limit } => {
                if let Some(ref db) = self.database {
                    match db.get_recent_errors(limit).await {
                        Ok(entries) => {
                            let errors: Vec<ErrorInfo> = entries
                                .into_iter()
                                .map(|e| {
                                    let timestamp = NaiveDateTime::parse_from_str(
                                        &e.timestamp,
                                        "%Y-%m-%d %H:%M:%S",
                                    )
                                    .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                                    .unwrap_or_else(|_| Utc::now());
                                    ErrorInfo {
                                        id: e.id,
                                        error_type: e.error_type,
                                        error_message: e.error_message,
                                        stack_trace: e.stack_trace,
                                        user_id: e.user_id,
                                        channel_id: e.channel_id,
                                        command: e.command,
                                        timestamp,
                                    }
                                })
                                .collect();
                            self.broadcast(BotEvent::RecentErrorsResponse { errors });
                            debug!("Sent RecentErrorsResponse");
                        }
                        Err(e) => {
                            warn!("Failed to get recent errors: {e}");
                        }
                    }
                }
            }
            TuiCommand::GetDmSessions { user_id, limit } => {
                if let Some(ref db) = self.database {
                    match db.get_user_dm_sessions(&user_id, limit).await {
                        Ok(sessions) => {
                            let dm_sessions: Vec<DmSessionInfo> = sessions
                                .into_iter()
                                .map(|s| {
                                    let started_at = NaiveDateTime::parse_from_str(
                                        &s.started_at,
                                        "%Y-%m-%d %H:%M:%S",
                                    )
                                    .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                                    .unwrap_or_else(|_| Utc::now());
                                    let ended_at = s.ended_at.and_then(|e| {
                                        NaiveDateTime::parse_from_str(&e, "%Y-%m-%d %H:%M:%S")
                                            .map(|dt| {
                                                DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)
                                            })
                                            .ok()
                                    });
                                    DmSessionInfo {
                                        session_id: s.session_id,
                                        started_at,
                                        ended_at,
                                        message_count: s.message_count,
                                        api_cost: s.api_cost,
                                        total_tokens: s.total_tokens,
                                    }
                                })
                                .collect();

                            // Get user stats for the response
                            match db.get_user_details(&user_id).await {
                                Ok(details) => {
                                    let stats = UserStats {
                                        user_id: details.user_id.clone(),
                                        username: details.username,
                                        message_count: details.message_count,
                                        total_cost: details.total_cost,
                                        total_tokens: details.total_tokens,
                                        total_api_calls: details.total_calls,
                                        dm_session_count: details.dm_session_count,
                                        chat_calls: details.chat_calls,
                                        whisper_calls: details.whisper_calls,
                                        dalle_calls: details.dalle_calls,
                                        first_seen: None,
                                        last_activity: None,
                                        favorite_persona: details.favorite_persona,
                                    };
                                    self.broadcast(BotEvent::UserDetailsResponse {
                                        user_id,
                                        stats,
                                        dm_sessions,
                                    });
                                }
                                Err(_) => {
                                    // Send minimal response
                                    self.broadcast(BotEvent::UserDetailsResponse {
                                        user_id: user_id.clone(),
                                        stats: UserStats {
                                            user_id: user_id.clone(),
                                            username: None,
                                            message_count: 0,
                                            total_cost: 0.0,
                                            total_tokens: 0,
                                            total_api_calls: 0,
                                            dm_session_count: dm_sessions.len() as u64,
                                            chat_calls: 0,
                                            whisper_calls: 0,
                                            dalle_calls: 0,
                                            first_seen: None,
                                            last_activity: None,
                                            favorite_persona: None,
                                        },
                                        dm_sessions,
                                    });
                                }
                            }
                            debug!("Sent UserDetailsResponse for DM sessions");
                        }
                        Err(e) => {
                            warn!("Failed to get DM sessions: {e}");
                        }
                    }
                }
            }
            TuiCommand::GetFeatureStates { guild_id } => {
                if let Some(ref db) = self.database {
                    let guild_id_str = guild_id.map(|id| id.to_string());
                    match db
                        .get_guild_feature_flags(guild_id_str.as_deref().unwrap_or("global"))
                        .await
                    {
                        Ok(states) => {
                            // Ensure all toggleable features have a state (default to true if not set)
                            let mut full_states = std::collections::HashMap::new();
                            for feature in crate::features::FEATURES.iter() {
                                if feature.toggleable {
                                    let enabled = states.get(feature.id).copied().unwrap_or(true);
                                    full_states.insert(feature.id.to_string(), enabled);
                                }
                            }
                            self.broadcast(BotEvent::FeatureStatesResponse {
                                states: full_states,
                                guild_id,
                            });
                            debug!("Sent FeatureStatesResponse with {} states", states.len());
                        }
                        Err(e) => {
                            warn!("Failed to get feature states: {e}");
                            // Send empty states on error
                            self.broadcast(BotEvent::FeatureStatesResponse {
                                states: std::collections::HashMap::new(),
                                guild_id,
                            });
                        }
                    }
                } else {
                    // No database - send default states (all enabled)
                    let mut states = std::collections::HashMap::new();
                    for feature in crate::features::FEATURES.iter() {
                        if feature.toggleable {
                            states.insert(feature.id.to_string(), true);
                        }
                    }
                    self.broadcast(BotEvent::FeatureStatesResponse { states, guild_id });
                }
            }
            TuiCommand::GetChannelsWithHistory { guild_id } => {
                if let Some(ref db) = self.database {
                    let guild_id_str = guild_id.map(|id| id.to_string());
                    match db.get_channels_with_history(guild_id_str.as_deref()).await {
                        Ok(entries) => {
                            // Enrich with channel/guild names from cached guilds
                            let guilds = self.get_guilds().await;

                            let channels: Vec<ChannelHistorySummary> = entries
                                .into_iter()
                                .map(|e| {
                                    let channel_id = e.channel_id.parse().unwrap_or(0);
                                    let db_guild_id =
                                        e.guild_id.as_ref().and_then(|g| g.parse::<u64>().ok());

                                    // Look up channel and guild names from cache
                                    let (channel_name, guild_name, found_guild_id) = guilds
                                        .iter()
                                        .find_map(|g| {
                                            g.channels.iter().find(|c| c.id == channel_id).map(
                                                |c| {
                                                    (
                                                        Some(c.name.clone()),
                                                        Some(g.name.clone()),
                                                        Some(g.id),
                                                    )
                                                },
                                            )
                                        })
                                        .unwrap_or((None, None, db_guild_id));

                                    // Parse last_activity to DateTime
                                    let last_activity = e.last_activity.and_then(|s| {
                                        NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                                            .map(|dt| {
                                                DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)
                                            })
                                            .ok()
                                    });

                                    ChannelHistorySummary {
                                        channel_id,
                                        channel_name,
                                        guild_id: found_guild_id,
                                        guild_name,
                                        message_count: e.message_count as u64,
                                        last_activity,
                                    }
                                })
                                .collect();

                            let count = channels.len();
                            self.broadcast(BotEvent::ChannelsWithHistoryResponse { channels });
                            debug!("Sent ChannelsWithHistoryResponse with {count} channels");
                        }
                        Err(e) => {
                            warn!("Failed to get channels with history: {e}");
                            // Send empty response on error
                            self.broadcast(BotEvent::ChannelsWithHistoryResponse {
                                channels: vec![],
                            });
                        }
                    }
                } else {
                    // No database - send empty response
                    self.broadcast(BotEvent::ChannelsWithHistoryResponse { channels: vec![] });
                }
            }
        }
    }

    /// Start the command processing loop (call this after server start)
    pub fn start_command_processor(self: Arc<Self>) {
        let server = self.clone();
        tokio::spawn(async move {
            info!("📡 IPC command processor started");
            loop {
                if let Some(cmd) = server.try_recv_command().await {
                    debug!("Processing TUI command: {cmd:?}");
                    server.process_command(cmd).await;
                }
                // Small sleep to avoid busy-waiting
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        });
    }
}

impl Default for IpcServer {
    fn default() -> Self {
        Self::new()
    }
}
