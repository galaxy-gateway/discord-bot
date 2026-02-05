//! # IPC Client
//!
//! Unix socket client for the TUI to communicate with the bot.

use crate::ipc::get_socket_path;
use crate::ipc::protocol::{encode_message, BotEvent, TuiCommand};
use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};

/// Connection timeout
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Read timeout for events
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// IPC Client for the TUI
pub struct IpcClient {
    /// Event receiver channel
    event_rx: mpsc::Receiver<BotEvent>,
    /// Command sender channel
    command_tx: mpsc::Sender<TuiCommand>,
    /// Connection status
    connected: Arc<RwLock<bool>>,
    /// Reconnect flag
    should_reconnect: Arc<RwLock<bool>>,
}

impl IpcClient {
    /// Connect to the bot's IPC server
    pub async fn connect() -> Result<Self> {
        let socket_path = get_socket_path();

        info!("Connecting to IPC server at {}", socket_path);

        let stream = timeout(CONNECT_TIMEOUT, UnixStream::connect(&socket_path))
            .await
            .map_err(|_| anyhow!("Connection timeout"))?
            .map_err(|e| anyhow!("Failed to connect: {}", e))?;

        info!("Connected to IPC server");

        let (event_tx, event_rx) = mpsc::channel(256);
        let (command_tx, command_rx) = mpsc::channel(64);
        let connected = Arc::new(RwLock::new(true));
        let should_reconnect = Arc::new(RwLock::new(true));

        // Start the connection handler
        let connected_clone = connected.clone();
        tokio::spawn(async move {
            Self::connection_loop(stream, event_tx, command_rx, connected_clone).await;
        });

        Ok(IpcClient {
            event_rx,
            command_tx,
            connected,
            should_reconnect,
        })
    }

    /// Main connection loop - handles reading events and writing commands
    async fn connection_loop(
        stream: UnixStream,
        event_tx: mpsc::Sender<BotEvent>,
        mut command_rx: mpsc::Receiver<TuiCommand>,
        connected: Arc<RwLock<bool>>,
    ) {
        let (mut reader, mut writer) = stream.into_split();

        // Spawn command writer task
        let write_connected = connected.clone();
        let write_handle = tokio::spawn(async move {
            while let Some(cmd) = command_rx.recv().await {
                match encode_message(&cmd) {
                    Ok(data) => {
                        if let Err(e) = writer.write_all(&data).await {
                            error!("Failed to write command: {}", e);
                            *write_connected.write().await = false;
                            break;
                        }
                        if let Err(e) = writer.flush().await {
                            error!("Failed to flush command: {}", e);
                            *write_connected.write().await = false;
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to encode command: {}", e);
                    }
                }
            }
        });

        // Event reader loop
        loop {
            // Read length prefix with timeout
            let mut len_buf = [0u8; 4];
            match timeout(READ_TIMEOUT, reader.read_exact(&mut len_buf)).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    if e.kind() != std::io::ErrorKind::UnexpectedEof {
                        error!("Read error: {}", e);
                    }
                    break;
                }
                Err(_) => {
                    // Timeout - send pong to keep connection alive
                    debug!("Read timeout, connection may be idle");
                    continue;
                }
            }

            let len = u32::from_be_bytes(len_buf) as usize;

            if len > 10 * 1024 * 1024 {
                error!("Message too large: {} bytes", len);
                break;
            }

            // Read message body
            let mut buf = vec![0u8; len];
            if let Err(e) = reader.read_exact(&mut buf).await {
                error!("Failed to read message body: {}", e);
                break;
            }

            // Parse event
            match serde_json::from_slice::<BotEvent>(&buf) {
                Ok(event) => {
                    // Handle heartbeat internally
                    if let BotEvent::Heartbeat { timestamp } = &event {
                        debug!("Received heartbeat: {}", timestamp);
                    }

                    if event_tx.send(event).await.is_err() {
                        debug!("Event receiver closed");
                        break;
                    }
                }
                Err(e) => {
                    warn!("Failed to parse event: {}", e);
                }
            }
        }

        *connected.write().await = false;
        write_handle.abort();
        info!("IPC connection closed");
    }

    /// Try to receive an event (non-blocking)
    pub fn try_recv(&mut self) -> Option<BotEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Receive an event (blocking)
    pub async fn recv(&mut self) -> Option<BotEvent> {
        self.event_rx.recv().await
    }

    /// Send a command to the bot
    pub async fn send(&self, cmd: TuiCommand) -> Result<()> {
        self.command_tx
            .send(cmd)
            .await
            .map_err(|e| anyhow!("Failed to send command: {}", e))
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Request bot status
    pub async fn request_status(&self) -> Result<()> {
        self.send(TuiCommand::GetStatus).await
    }

    /// Request guild list
    pub async fn request_guilds(&self) -> Result<()> {
        self.send(TuiCommand::GetGuilds).await
    }

    /// Watch a channel for messages
    pub async fn watch_channel(&self, channel_id: u64) -> Result<()> {
        self.send(TuiCommand::WatchChannel { channel_id }).await
    }

    /// Stop watching a channel
    pub async fn unwatch_channel(&self, channel_id: u64) -> Result<()> {
        self.send(TuiCommand::UnwatchChannel { channel_id }).await
    }

    /// Send a message to a channel
    pub async fn send_message(&self, channel_id: u64, content: String) -> Result<String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        self.send(TuiCommand::SendMessage {
            request_id: request_id.clone(),
            channel_id,
            content,
        })
        .await?;
        Ok(request_id)
    }

    /// Set a feature enabled/disabled
    pub async fn set_feature(
        &self,
        feature: String,
        enabled: bool,
        guild_id: Option<u64>,
    ) -> Result<String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        self.send(TuiCommand::SetFeature {
            request_id: request_id.clone(),
            feature,
            enabled,
            guild_id,
        })
        .await?;
        Ok(request_id)
    }

    /// Set channel persona
    pub async fn set_channel_persona(
        &self,
        guild_id: u64,
        channel_id: u64,
        persona: String,
    ) -> Result<String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        self.send(TuiCommand::SetChannelPersona {
            request_id: request_id.clone(),
            guild_id,
            channel_id,
            persona,
        })
        .await?;
        Ok(request_id)
    }

    /// Set guild setting
    pub async fn set_guild_setting(
        &self,
        guild_id: u64,
        key: String,
        value: String,
    ) -> Result<String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        self.send(TuiCommand::SetGuildSetting {
            request_id: request_id.clone(),
            guild_id,
            key,
            value,
        })
        .await?;
        Ok(request_id)
    }

    /// Get channel message history
    pub async fn get_channel_history(&self, channel_id: u64, limit: u32) -> Result<()> {
        self.send(TuiCommand::GetChannelHistory { channel_id, limit })
            .await
    }

    /// Request usage statistics
    pub async fn request_usage_stats(&self, period_days: Option<u32>) -> Result<()> {
        self.send(TuiCommand::GetUsageStats { period_days }).await
    }

    /// Request system metrics
    pub async fn request_system_metrics(&self) -> Result<()> {
        self.send(TuiCommand::GetSystemMetrics).await
    }

    /// Request channel information
    pub async fn request_channel_info(&self, channel_id: u64) -> Result<()> {
        self.send(TuiCommand::GetChannelInfo { channel_id }).await
    }

    /// Request historical metrics
    pub async fn request_historical_metrics(&self, metric_type: String, hours: u32) -> Result<()> {
        self.send(TuiCommand::GetHistoricalMetrics { metric_type, hours })
            .await
    }

    /// Request user list with stats
    pub async fn request_user_list(&self, limit: u32) -> Result<()> {
        self.send(TuiCommand::GetUserList { limit }).await
    }

    /// Request detailed user statistics
    pub async fn request_user_details(&self, user_id: String) -> Result<()> {
        self.send(TuiCommand::GetUserDetails { user_id }).await
    }

    /// Request recent errors
    pub async fn request_recent_errors(&self, limit: u32) -> Result<()> {
        self.send(TuiCommand::GetRecentErrors { limit }).await
    }

    /// Request DM sessions for a user
    pub async fn request_dm_sessions(&self, user_id: String, limit: u32) -> Result<()> {
        self.send(TuiCommand::GetDmSessions { user_id, limit })
            .await
    }

    /// Request feature states (enabled/disabled) for a guild
    pub async fn request_feature_states(&self, guild_id: Option<u64>) -> Result<()> {
        self.send(TuiCommand::GetFeatureStates { guild_id }).await
    }

    /// Request channels with conversation history (for browse mode)
    pub async fn request_channels_with_history(&self, guild_id: Option<u64>) -> Result<()> {
        self.send(TuiCommand::GetChannelsWithHistory { guild_id })
            .await
    }

    /// Disable auto-reconnect (for clean shutdown)
    pub async fn disable_reconnect(&self) {
        *self.should_reconnect.write().await = false;
    }
}

/// Try to connect with retries
pub async fn connect_with_retry(max_attempts: u32, delay: Duration) -> Result<IpcClient> {
    for attempt in 1..=max_attempts {
        match IpcClient::connect().await {
            Ok(client) => return Ok(client),
            Err(e) => {
                if attempt < max_attempts {
                    warn!(
                        "Connection attempt {} failed: {}. Retrying in {:?}...",
                        attempt, e, delay
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    return Err(anyhow!(
                        "Failed to connect after {} attempts: {}",
                        max_attempts,
                        e
                    ));
                }
            }
        }
    }
    unreachable!()
}
