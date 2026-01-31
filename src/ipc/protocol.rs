//! # IPC Protocol
//!
//! Message types for bot <-> TUI communication over Unix socket.
//!
//! Uses length-prefixed JSON framing:
//! - 4 bytes: message length (big-endian u32)
//! - N bytes: JSON payload

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::io::{Read, Write};
use anyhow::{Result, anyhow};

// ============================================================================
// Bot -> TUI Events
// ============================================================================

/// Events sent from the bot to connected TUI clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BotEvent {
    /// Discord message created
    MessageCreate {
        channel_id: u64,
        guild_id: Option<u64>,
        message: DisplayMessage,
    },
    /// Discord message deleted
    MessageDelete {
        channel_id: u64,
        message_id: u64,
    },
    /// User presence update
    PresenceUpdate {
        user_id: u64,
        status: String,
    },
    /// Bot is ready and connected
    Ready {
        guilds: Vec<GuildInfo>,
        bot_user_id: u64,
        bot_username: String,
    },
    /// Bot disconnected from Discord
    Disconnected {
        reason: Option<String>,
    },
    /// Response to a command
    CommandResponse {
        request_id: String,
        success: bool,
        message: Option<String>,
        data: Option<serde_json::Value>,
    },
    /// Status update (response to GetStatus)
    StatusUpdate {
        connected: bool,
        uptime_seconds: u64,
        guild_count: usize,
        active_sessions: usize,
    },
    /// Heartbeat to keep connection alive
    Heartbeat {
        timestamp: i64,
    },
}

/// Simplified message for display in TUI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayMessage {
    pub id: u64,
    pub author_id: u64,
    pub author_name: String,
    pub author_discriminator: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub is_bot: bool,
    pub attachments: Vec<AttachmentInfo>,
    pub embeds_count: usize,
}

/// Attachment metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub filename: String,
    pub size: u64,
    pub url: String,
}

/// Guild information for Ready event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildInfo {
    pub id: u64,
    pub name: String,
    pub channels: Vec<ChannelInfo>,
    pub member_count: Option<u64>,
}

/// Channel information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub id: u64,
    pub name: String,
    pub channel_type: ChannelType,
}

/// Channel type enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChannelType {
    Text,
    Voice,
    Category,
    News,
    Thread,
    Forum,
    Other,
}

// ============================================================================
// TUI -> Bot Commands
// ============================================================================

/// Commands sent from TUI to the bot
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TuiCommand {
    /// Send a message to a channel
    SendMessage {
        request_id: String,
        channel_id: u64,
        content: String,
    },
    /// Start watching a channel for messages
    WatchChannel {
        channel_id: u64,
    },
    /// Stop watching a channel
    UnwatchChannel {
        channel_id: u64,
    },
    /// Toggle a feature on/off
    SetFeature {
        request_id: String,
        feature: String,
        enabled: bool,
        guild_id: Option<u64>,
    },
    /// Set a channel's persona
    SetChannelPersona {
        request_id: String,
        channel_id: u64,
        persona: String,
    },
    /// Set a guild setting
    SetGuildSetting {
        request_id: String,
        guild_id: u64,
        key: String,
        value: String,
    },
    /// Request current status
    GetStatus,
    /// Request guild list
    GetGuilds,
    /// Request channel messages (history)
    GetChannelHistory {
        channel_id: u64,
        limit: u32,
    },
    /// Heartbeat response
    Pong {
        timestamp: i64,
    },
}

// ============================================================================
// Framing - Length-prefixed JSON messages
// ============================================================================

/// Encode a message with length prefix
pub fn encode_message<T: Serialize>(msg: &T) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(msg)?;
    let len = json.len() as u32;
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&json);
    Ok(buf)
}

/// Read a length-prefixed message from a reader
pub fn decode_message<T: for<'de> Deserialize<'de>, R: Read>(reader: &mut R) -> Result<T> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 10 * 1024 * 1024 {
        return Err(anyhow!("Message too large: {} bytes", len));
    }

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;

    Ok(serde_json::from_slice(&buf)?)
}

/// Write a framed message to a writer
pub fn write_message<T: Serialize, W: Write>(writer: &mut W, msg: &T) -> Result<()> {
    let encoded = encode_message(msg)?;
    writer.write_all(&encoded)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_encode_decode_roundtrip() {
        let event = BotEvent::Heartbeat { timestamp: 12345 };
        let encoded = encode_message(&event).unwrap();

        let mut cursor = Cursor::new(encoded);
        let decoded: BotEvent = decode_message(&mut cursor).unwrap();

        match decoded {
            BotEvent::Heartbeat { timestamp } => assert_eq!(timestamp, 12345),
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_command_serialization() {
        let cmd = TuiCommand::SendMessage {
            request_id: "test-123".to_string(),
            channel_id: 123456789,
            content: "Hello, world!".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("SendMessage"));
        assert!(json.contains("test-123"));
    }
}
