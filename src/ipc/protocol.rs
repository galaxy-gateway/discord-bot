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
    /// Usage stats update
    UsageStatsUpdate {
        total_cost: f64,
        period_cost: f64,
        total_tokens: u64,
        total_calls: u64,
        cost_by_service: Vec<(String, f64)>,
        daily_breakdown: Vec<(String, f64)>,
        top_users: Vec<TopUser>,
        period_days: Option<u32>,
    },
    /// System metrics update
    SystemMetricsUpdate {
        cpu_percent: f32,
        memory_bytes: u64,
        memory_total: u64,
        db_size: u64,
        uptime_seconds: u64,
    },
    /// Channel information response
    ChannelInfoResponse {
        channel_id: u64,
        name: String,
        guild_name: Option<String>,
        message_count: u64,
        last_activity: Option<DateTime<Utc>>,
    },
    /// Channel history response (messages from database)
    ChannelHistoryResponse {
        channel_id: u64,
        messages: Vec<DisplayMessage>,
    },
    /// Historical metrics response (time-series data)
    HistoricalMetricsResponse {
        metric_type: String,
        data_points: Vec<(i64, f64)>,
    },
    /// User list response
    UserListResponse {
        users: Vec<UserSummary>,
    },
    /// User details response
    UserDetailsResponse {
        user_id: String,
        stats: UserStats,
        dm_sessions: Vec<DmSessionInfo>,
    },
    /// Recent errors response
    RecentErrorsResponse {
        errors: Vec<ErrorInfo>,
    },
    /// Feature states response (enabled/disabled for each feature)
    FeatureStatesResponse {
        /// Map of feature_id -> enabled
        states: std::collections::HashMap<String, bool>,
        /// Guild ID these states apply to (None = global)
        guild_id: Option<u64>,
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

/// User summary for list display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    pub user_id: String,
    pub username: Option<String>,
    pub message_count: u64,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub session_count: u64,
    pub last_activity: Option<DateTime<Utc>>,
}

/// Detailed user statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStats {
    pub user_id: String,
    pub username: Option<String>,
    pub message_count: u64,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub total_api_calls: u64,
    pub dm_session_count: u64,
    pub chat_calls: u64,
    pub whisper_calls: u64,
    pub dalle_calls: u64,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_activity: Option<DateTime<Utc>>,
    pub favorite_persona: Option<String>,
}

/// DM session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmSessionInfo {
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub message_count: u32,
    pub api_cost: f64,
    pub total_tokens: u64,
}

/// Error log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub id: i64,
    pub error_type: String,
    pub error_message: String,
    pub stack_trace: Option<String>,
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub command: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Top user by cost with optional cached username
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopUser {
    pub user_id: String,
    pub username: Option<String>,
    pub cost: f64,
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
        guild_id: u64,
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
    /// Request usage statistics
    GetUsageStats {
        /// Number of days to query (None = all time)
        period_days: Option<u32>,
    },
    /// Request system metrics
    GetSystemMetrics,
    /// Heartbeat response
    Pong {
        timestamp: i64,
    },
    /// Request channel information
    GetChannelInfo {
        channel_id: u64,
    },
    /// Request historical metrics (time-series data)
    GetHistoricalMetrics {
        metric_type: String,
        hours: u32,
    },
    /// Request user list with stats
    GetUserList {
        limit: u32,
    },
    /// Request detailed user statistics
    GetUserDetails {
        user_id: String,
    },
    /// Request recent errors
    GetRecentErrors {
        limit: u32,
    },
    /// Request DM sessions for a user
    GetDmSessions {
        user_id: String,
        limit: u32,
    },
    /// Request feature states (enabled/disabled) for a guild
    GetFeatureStates {
        guild_id: Option<u64>,
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
