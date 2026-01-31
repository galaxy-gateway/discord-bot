//! # IPC Module
//!
//! Inter-process communication between the bot and TUI.
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.17.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.0.0: Initial IPC implementation with Unix socket protocol

pub mod protocol;
pub mod server;
pub mod client;

pub use protocol::{
    BotEvent, TuiCommand, DisplayMessage, GuildInfo, ChannelInfo, ChannelType, AttachmentInfo,
    UserSummary, UserStats, DmSessionInfo, ErrorInfo,
};
pub use server::IpcServer;
pub use client::{IpcClient, connect_with_retry};

/// Default socket path for IPC communication
pub const DEFAULT_SOCKET_PATH: &str = "/tmp/obi-bot.sock";

/// Get the socket path from environment or use default
pub fn get_socket_path() -> String {
    std::env::var("OBI_IPC_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET_PATH.to_string())
}
