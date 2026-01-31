//! # IPC Server
//!
//! Unix socket server for the bot to communicate with TUI clients.
//!
//! - **Version**: 1.2.0
//! - **Since**: 3.17.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.2.0: Added GetUsageStats and GetSystemMetrics commands with database integration
//! - 1.1.0: Added command processing support and shared state for guilds/bot info
//! - 1.0.0: Initial IPC implementation with Unix socket protocol

use crate::database::Database;
use crate::ipc::protocol::{BotEvent, TuiCommand, encode_message, GuildInfo};
use crate::ipc::get_socket_path;
use anyhow::Result;
use log::{debug, error, info, warn};
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
        }
    }

    /// Set the database connection for stats queries
    pub fn with_database(mut self, database: Database, db_path: String) -> Self {
        self.database = Some(database);
        self.db_path = Some(db_path);
        self
    }

    /// Start the IPC server in a background task
    pub async fn start(self: Arc<Self>) -> Result<()> {
        let socket_path = get_socket_path();

        // Remove existing socket file if it exists
        if std::path::Path::new(&socket_path).exists() {
            std::fs::remove_file(&socket_path)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        info!("IPC server listening on {}", socket_path);

        // Spawn the accept loop
        let server = self.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let client_count = *server.client_count.read().await;
                        if client_count >= MAX_CLIENTS {
                            warn!("Maximum IPC clients reached ({}), rejecting connection", MAX_CLIENTS);
                            continue;
                        }

                        *server.client_count.write().await += 1;
                        info!("TUI client connected (total: {})", client_count + 1);

                        let server_clone = server.clone();
                        let client_count_ref = server.client_count.clone();
                        tokio::spawn(async move {
                            if let Err(e) = server_clone.handle_client(stream).await {
                                debug!("Client handler ended: {}", e);
                            }
                            *client_count_ref.write().await -= 1;
                            info!("TUI client disconnected");
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept IPC connection: {}", e);
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
                    Ok(event) => {
                        match encode_message(&event) {
                            Ok(data) => {
                                if let Err(e) = writer.write_all(&data).await {
                                    debug!("Failed to write to client: {}", e);
                                    break;
                                }
                                if let Err(e) = writer.flush().await {
                                    debug!("Failed to flush to client: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to encode event: {}", e);
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Client lagged behind by {} events", n);
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
                error!("Message too large from client: {} bytes", len);
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
                            debug!("Now watching channel {}", channel_id);
                        }
                        TuiCommand::UnwatchChannel { channel_id } => {
                            watched_channels.write().await.remove(channel_id);
                            debug!("Stopped watching channel {}", channel_id);
                        }
                        _ => {}
                    }

                    // Forward command to bot
                    if let Err(e) = command_tx.send(cmd).await {
                        error!("Failed to forward command: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    warn!("Failed to parse command from client: {}", e);
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
        debug!("Added watched channel {}", channel_id);
    }

    /// Remove a watched channel
    pub async fn remove_watched_channel(&self, channel_id: u64) {
        self.watched_channels.write().await.remove(&channel_id);
        debug!("Removed watched channel {}", channel_id);
    }

    /// Get list of watched channels
    pub async fn get_watched_channels(&self) -> Vec<u64> {
        self.watched_channels.read().await.iter().copied().collect()
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
                let bot_username = self.get_bot_username().await.unwrap_or_else(|| "Unknown".to_string());
                self.broadcast(BotEvent::Ready {
                    guilds,
                    bot_user_id,
                    bot_username,
                });
                debug!("Sent Ready response with guild info");
            }
            TuiCommand::WatchChannel { channel_id } => {
                // Already handled in handle_client, but log it
                debug!("WatchChannel command processed for channel {}", channel_id);
            }
            TuiCommand::UnwatchChannel { channel_id } => {
                // Already handled in handle_client, but log it
                debug!("UnwatchChannel command processed for channel {}", channel_id);
            }
            TuiCommand::Pong { timestamp } => {
                debug!("Received Pong with timestamp {}", timestamp);
            }
            TuiCommand::SendMessage { request_id, channel_id, content } => {
                // This would need HTTP client access to actually send
                // For now, just acknowledge receipt
                warn!("SendMessage command received but not implemented (channel: {}, content length: {})",
                      channel_id, content.len());
                self.broadcast(BotEvent::CommandResponse {
                    request_id,
                    success: false,
                    message: Some("SendMessage not yet implemented in IPC".to_string()),
                    data: None,
                });
            }
            TuiCommand::SetFeature { request_id, feature, enabled, guild_id } => {
                // This would need database access to actually set
                warn!("SetFeature command received but not implemented (feature: {}, enabled: {}, guild: {:?})",
                      feature, enabled, guild_id);
                self.broadcast(BotEvent::CommandResponse {
                    request_id,
                    success: false,
                    message: Some("SetFeature not yet implemented in IPC".to_string()),
                    data: None,
                });
            }
            TuiCommand::SetChannelPersona { request_id, channel_id, persona } => {
                warn!("SetChannelPersona command received but not implemented (channel: {}, persona: {})",
                      channel_id, persona);
                self.broadcast(BotEvent::CommandResponse {
                    request_id,
                    success: false,
                    message: Some("SetChannelPersona not yet implemented in IPC".to_string()),
                    data: None,
                });
            }
            TuiCommand::SetGuildSetting { request_id, guild_id, key, value } => {
                warn!("SetGuildSetting command received but not implemented (guild: {}, key: {}, value: {})",
                      guild_id, key, value);
                self.broadcast(BotEvent::CommandResponse {
                    request_id,
                    success: false,
                    message: Some("SetGuildSetting not yet implemented in IPC".to_string()),
                    data: None,
                });
            }
            TuiCommand::GetChannelHistory { channel_id, limit } => {
                warn!("GetChannelHistory command received but not implemented (channel: {}, limit: {})",
                      channel_id, limit);
                // Would need Discord API access to fetch history
            }
            TuiCommand::GetUsageStats { period_days } => {
                if let Some(ref db) = self.database {
                    match db.get_global_usage_stats(period_days).await {
                        Ok((total_cost, period_cost, total_tokens, total_calls, cost_by_service, daily_breakdown, top_users)) => {
                            self.broadcast(BotEvent::UsageStatsUpdate {
                                total_cost,
                                period_cost,
                                total_tokens,
                                total_calls,
                                cost_by_service,
                                daily_breakdown,
                                top_users,
                                period_days,
                            });
                            debug!("Sent UsageStatsUpdate response");
                        }
                        Err(e) => {
                            warn!("Failed to get usage stats: {}", e);
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
                    debug!("Processing TUI command: {:?}", cmd);
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
