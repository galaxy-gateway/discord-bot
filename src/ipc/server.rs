//! # IPC Server
//!
//! Unix socket server for the bot to communicate with TUI clients.

use crate::ipc::protocol::{BotEvent, TuiCommand, encode_message};
use crate::ipc::get_socket_path;
use anyhow::Result;
use log::{debug, error, info, warn};
use std::sync::Arc;
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
        }
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
}

impl Default for IpcServer {
    fn default() -> Self {
        Self::new()
    }
}
