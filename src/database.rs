//! # Feature: Database Multi-Tenancy
//!
//! SQLite database layer with multi-bot support via bot_id column.
//!
//! - **Version**: 3.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 3.0.0: Add startup session tables and methods for combined notifications
//! - 2.0.0: Added bot_id parameter to all methods for multi-bot support
//! - 1.0.0: Initial database implementation

use anyhow::Result;
use log::info;
use sqlite::{Connection, State};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Default bot_id used for backward compatibility with existing data
pub const DEFAULT_BOT_ID: &str = "default";

/// Startup notification session information
pub struct StartupSession {
    pub session_id: String,
    pub owner_message_id: Option<String>,
    pub channel_message_id: Option<String>,
    pub created_at: String,
    pub last_updated: String,
}

/// Information about a bot that started in a session
pub struct StartedBot {
    pub bot_id: String,
    pub bot_name: String,
    pub version: String,
    pub guilds_count: i64,
    pub shard_info: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    pub async fn new(database_path: &str) -> Result<Self> {
        let connection = sqlite::open(database_path)?;
        let db = Database {
            connection: Arc::new(Mutex::new(connection)),
        };

        db.init_tables().await?;
        info!("Database initialized at: {database_path}");
        Ok(db)
    }

    async fn init_tables(&self) -> Result<()> {
        let conn = self.connection.lock().await;

        // User preferences with bot_id for multi-bot support
        conn.execute(
            "CREATE TABLE IF NOT EXISTS user_preferences (
                bot_id TEXT NOT NULL DEFAULT 'default',
                user_id TEXT NOT NULL,
                default_persona TEXT DEFAULT 'obi',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (bot_id, user_id)
            )",
        )?;

        // Usage statistics with bot_id
        conn.execute(
            "CREATE TABLE IF NOT EXISTS usage_stats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                user_id TEXT NOT NULL,
                command TEXT NOT NULL,
                persona TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_usage ON usage_stats(bot_id, timestamp)",
        )?;

        // Conversation history with bot_id
        conn.execute(
            "CREATE TABLE IF NOT EXISTS conversation_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                user_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                persona TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation
             ON conversation_history(bot_id, user_id, channel_id, timestamp)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation_channel
             ON conversation_history(bot_id, channel_id, timestamp)",
        )?;

        // Enhanced Interaction Tracking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS message_metadata (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                message_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                attachment_urls TEXT,
                embed_data TEXT,
                reactions TEXT,
                edited_at DATETIME,
                deleted_at DATETIME,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_message_metadata
             ON message_metadata(bot_id, message_id)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS interaction_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                user_id TEXT NOT NULL,
                guild_id TEXT,
                session_start DATETIME DEFAULT CURRENT_TIMESTAMP,
                session_end DATETIME,
                message_count INTEGER DEFAULT 0,
                last_activity DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_user
             ON interaction_sessions(bot_id, user_id, session_start)",
        )?;

        // Feature-Specific Data
        conn.execute(
            "CREATE TABLE IF NOT EXISTS user_bookmarks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                user_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                bookmark_name TEXT,
                bookmark_note TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_bookmark_user
             ON user_bookmarks(bot_id, user_id)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS reminders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                user_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                reminder_text TEXT NOT NULL,
                remind_at DATETIME NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                completed BOOLEAN DEFAULT 0,
                completed_at DATETIME
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reminder_time
             ON reminders(bot_id, remind_at, completed)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS custom_commands (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                command_name TEXT NOT NULL,
                response_text TEXT NOT NULL,
                created_by_user_id TEXT NOT NULL,
                guild_id TEXT,
                is_global BOOLEAN DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(bot_id, command_name, guild_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_custom_command
             ON custom_commands(bot_id, command_name, guild_id)",
        )?;

        // Analytics & Metrics
        conn.execute(
            "CREATE TABLE IF NOT EXISTS daily_analytics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                date DATE NOT NULL,
                total_messages INTEGER DEFAULT 0,
                unique_users INTEGER DEFAULT 0,
                total_commands INTEGER DEFAULT 0,
                total_errors INTEGER DEFAULT 0,
                persona_usage TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(bot_id, date)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_analytics_date
             ON daily_analytics(bot_id, date)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS performance_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                metric_type TEXT NOT NULL,
                value REAL NOT NULL,
                unit TEXT,
                metadata TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_metrics_type
             ON performance_metrics(bot_id, metric_type, timestamp)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS error_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                error_type TEXT NOT NULL,
                error_message TEXT NOT NULL,
                stack_trace TEXT,
                user_id TEXT,
                channel_id TEXT,
                command TEXT,
                metadata TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_error_type
             ON error_logs(bot_id, error_type, timestamp)",
        )?;

        // Extended Configuration
        conn.execute(
            "CREATE TABLE IF NOT EXISTS feature_flags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                feature_name TEXT NOT NULL,
                enabled BOOLEAN DEFAULT 0,
                user_id TEXT,
                guild_id TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(bot_id, feature_name, user_id, guild_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_feature_flag
             ON feature_flags(bot_id, feature_name, user_id, guild_id)",
        )?;

        // Feature versions tracking for audit trail
        conn.execute(
            "CREATE TABLE IF NOT EXISTS feature_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                feature_name TEXT NOT NULL,
                version TEXT NOT NULL,
                guild_id TEXT,
                toggled_by TEXT,
                enabled BOOLEAN NOT NULL,
                changed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_feature_versions
             ON feature_versions(bot_id, feature_name, guild_id, changed_at)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS guild_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                guild_id TEXT NOT NULL,
                setting_key TEXT NOT NULL,
                setting_value TEXT,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(bot_id, guild_id, setting_key)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_guild_setting
             ON guild_settings(bot_id, guild_id, setting_key)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS extended_user_preferences (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                user_id TEXT NOT NULL,
                preference_key TEXT NOT NULL,
                preference_value TEXT,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(bot_id, user_id, preference_key)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_pref
             ON extended_user_preferences(bot_id, user_id, preference_key)",
        )?;

        // Conflict Detection & Mediation
        conn.execute(
            "CREATE TABLE IF NOT EXISTS conflict_detection (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                channel_id TEXT NOT NULL,
                guild_id TEXT,
                participants TEXT NOT NULL,
                detection_type TEXT NOT NULL,
                confidence_score REAL,
                last_message_id TEXT,
                mediation_triggered BOOLEAN DEFAULT 0,
                mediation_message_id TEXT,
                first_detected DATETIME DEFAULT CURRENT_TIMESTAMP,
                last_detected DATETIME DEFAULT CURRENT_TIMESTAMP,
                resolved_at DATETIME
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conflict_channel
             ON conflict_detection(bot_id, channel_id, guild_id)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conflict_timestamp
             ON conflict_detection(bot_id, first_detected)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS mediation_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                conflict_id INTEGER NOT NULL,
                channel_id TEXT NOT NULL,
                mediation_message TEXT,
                effectiveness_rating INTEGER,
                follow_up_messages INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(conflict_id) REFERENCES conflict_detection(id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mediation_conflict
             ON mediation_history(bot_id, conflict_id)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS user_interaction_patterns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                user_id_a TEXT NOT NULL,
                user_id_b TEXT NOT NULL,
                channel_id TEXT,
                guild_id TEXT,
                interaction_count INTEGER DEFAULT 0,
                last_interaction DATETIME,
                conflict_incidents INTEGER DEFAULT 0,
                avg_response_time_ms INTEGER,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(bot_id, user_id_a, user_id_b, channel_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_interaction_users
             ON user_interaction_patterns(bot_id, user_id_a, user_id_b)",
        )?;

        // Channel Settings (for per-channel verbosity and other settings)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS channel_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                guild_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                verbosity TEXT DEFAULT 'concise',
                conflict_enabled BOOLEAN DEFAULT 1,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(bot_id, guild_id, channel_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_channel_settings_guild
             ON channel_settings(bot_id, guild_id)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_channel_settings_channel
             ON channel_settings(bot_id, channel_id)",
        )?;

        // Bot Settings (for global bot configuration, not per-guild)
        // Note: This table remains WITHOUT bot_id - it stores global settings
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bot_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                setting_key TEXT NOT NULL UNIQUE,
                setting_value TEXT,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        // OpenAI Usage Tracking Tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS openai_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                request_id TEXT,
                user_id TEXT NOT NULL,
                guild_id TEXT,
                channel_id TEXT,
                service_type TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                audio_duration_seconds REAL DEFAULT 0,
                image_count INTEGER DEFAULT 0,
                image_size TEXT,
                estimated_cost_usd REAL NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_openai_usage_user_ts
             ON openai_usage(bot_id, user_id, timestamp)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_openai_usage_guild_ts
             ON openai_usage(bot_id, guild_id, timestamp)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_openai_usage_timestamp
             ON openai_usage(bot_id, timestamp)",
        )?;

        // Daily aggregates for fast queries (90-day retention)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS openai_usage_daily (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL DEFAULT 'default',
                date DATE NOT NULL,
                guild_id TEXT,
                user_id TEXT,
                service_type TEXT NOT NULL,
                request_count INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                total_audio_seconds REAL DEFAULT 0,
                total_images INTEGER DEFAULT 0,
                total_cost_usd REAL DEFAULT 0,
                UNIQUE(bot_id, date, guild_id, user_id, service_type)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_openai_daily_guild_date
             ON openai_usage_daily(bot_id, guild_id, date)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_openai_daily_user_date
             ON openai_usage_daily(bot_id, user_id, date)",
        )?;

        // Startup Notification Session Tracking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bot_startup_session (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL UNIQUE,
                owner_message_id TEXT,
                channel_message_id TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                last_updated DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_startup_session_created
             ON bot_startup_session(created_at)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS bot_startup_bots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                bot_id TEXT NOT NULL,
                bot_name TEXT NOT NULL,
                version TEXT NOT NULL,
                guilds_count INTEGER NOT NULL,
                shard_info TEXT,
                started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(session_id, bot_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_startup_bots_session
             ON bot_startup_bots(session_id)",
        )?;

        Ok(())
    }

    // ========================================================================
    // User Persona Methods
    // ========================================================================

    pub async fn get_user_persona(&self, bot_id: &str, user_id: &str) -> Result<String> {
        let conn = self.connection.lock().await;
        let mut statement =
            conn.prepare("SELECT default_persona FROM user_preferences WHERE bot_id = ? AND user_id = ?")?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;

        if let Ok(State::Row) = statement.next() {
            Ok(statement.read::<String, _>("default_persona")?)
        } else {
            // Check for PERSONA environment variable, fallback to 'obi'
            Ok(std::env::var("PERSONA").unwrap_or_else(|_| "obi".to_string()))
        }
    }

    /// Get user persona with guild default fallback
    /// Cascade: user preference -> guild default -> env var -> "obi"
    pub async fn get_user_persona_with_guild(
        &self,
        bot_id: &str,
        user_id: &str,
        guild_id: Option<&str>,
    ) -> Result<String> {
        let conn = self.connection.lock().await;

        // First check user preference
        let mut statement =
            conn.prepare("SELECT default_persona FROM user_preferences WHERE bot_id = ? AND user_id = ?")?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;

        if let Ok(State::Row) = statement.next() {
            return Ok(statement.read::<String, _>("default_persona")?);
        }

        // Check guild default if guild_id is provided
        if let Some(gid) = guild_id {
            drop(statement);
            let mut guild_stmt = conn.prepare(
                "SELECT setting_value FROM guild_settings WHERE bot_id = ? AND guild_id = ? AND setting_key = 'default_persona'",
            )?;
            guild_stmt.bind((1, bot_id))?;
            guild_stmt.bind((2, gid))?;

            if let Ok(State::Row) = guild_stmt.next() {
                return Ok(guild_stmt.read::<String, _>(0)?);
            }
        }

        // Fall back to PERSONA environment variable, then 'obi'
        Ok(std::env::var("PERSONA").unwrap_or_else(|_| "obi".to_string()))
    }

    pub async fn set_user_persona(&self, bot_id: &str, user_id: &str, persona: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO user_preferences (bot_id, user_id, default_persona, updated_at)
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, persona))?;
        statement.next()?;

        info!("Updated persona for user {user_id} to {persona} (bot: {bot_id})");
        Ok(())
    }

    // ========================================================================
    // Usage Logging Methods
    // ========================================================================

    pub async fn log_usage(
        &self,
        bot_id: &str,
        user_id: &str,
        command: &str,
        persona: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO usage_stats (bot_id, user_id, command, persona) VALUES (?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, command))?;
        statement.bind((4, persona.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    // ========================================================================
    // Conversation History Methods
    // ========================================================================

    pub async fn store_message(
        &self,
        bot_id: &str,
        user_id: &str,
        channel_id: &str,
        role: &str,
        content: &str,
        persona: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO conversation_history (bot_id, user_id, channel_id, role, content, persona) VALUES (?, ?, ?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, channel_id))?;
        statement.bind((4, role))?;
        statement.bind((5, content))?;
        statement.bind((6, persona.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_conversation_history(
        &self,
        bot_id: &str,
        user_id: &str,
        channel_id: &str,
        limit: i64,
    ) -> Result<Vec<(String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT role, content FROM conversation_history
             WHERE bot_id = ? AND user_id = ? AND channel_id = ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, channel_id))?;
        statement.bind((4, limit))?;

        let mut history = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let role = statement.read::<String, _>("role")?;
            let content = statement.read::<String, _>("content")?;
            history.push((role, content));
        }

        // Reverse to get chronological order (oldest first)
        history.reverse();
        Ok(history)
    }

    pub async fn clear_conversation_history(
        &self,
        bot_id: &str,
        user_id: &str,
        channel_id: &str,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM conversation_history WHERE bot_id = ? AND user_id = ? AND channel_id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, channel_id))?;
        statement.next()?;
        info!("Cleared conversation history for user {user_id} in channel {channel_id} (bot: {bot_id})");
        Ok(())
    }

    pub async fn cleanup_old_messages(&self, bot_id: &str, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM conversation_history WHERE bot_id = ? AND timestamp < datetime('now', ? || ' days')",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, format!("-{days}").as_str()))?;
        statement.next()?;
        info!("Cleaned up conversation history older than {days} days (bot: {bot_id})");
        Ok(())
    }

    // ========================================================================
    // Message Metadata Methods
    // ========================================================================

    pub async fn store_message_metadata(
        &self,
        bot_id: &str,
        message_id: &str,
        user_id: &str,
        channel_id: &str,
        attachment_urls: Option<&str>,
        embed_data: Option<&str>,
        reactions: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO message_metadata (bot_id, message_id, user_id, channel_id, attachment_urls, embed_data, reactions)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, message_id))?;
        statement.bind((3, user_id))?;
        statement.bind((4, channel_id))?;
        statement.bind((5, attachment_urls.unwrap_or("")))?;
        statement.bind((6, embed_data.unwrap_or("")))?;
        statement.bind((7, reactions.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    pub async fn update_message_metadata_reactions(
        &self,
        bot_id: &str,
        message_id: &str,
        reactions: &str,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement =
            conn.prepare("UPDATE message_metadata SET reactions = ? WHERE bot_id = ? AND message_id = ?")?;
        statement.bind((1, reactions))?;
        statement.bind((2, bot_id))?;
        statement.bind((3, message_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn mark_message_deleted(&self, bot_id: &str, message_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE message_metadata SET deleted_at = CURRENT_TIMESTAMP WHERE bot_id = ? AND message_id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, message_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn mark_message_edited(&self, bot_id: &str, message_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE message_metadata SET edited_at = CURRENT_TIMESTAMP WHERE bot_id = ? AND message_id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, message_id))?;
        statement.next()?;
        Ok(())
    }

    // ========================================================================
    // Interaction Session Methods
    // ========================================================================

    pub async fn start_session(&self, bot_id: &str, user_id: &str, guild_id: Option<&str>) -> Result<i64> {
        let conn = self.connection.lock().await;
        let mut statement =
            conn.prepare("INSERT INTO interaction_sessions (bot_id, user_id, guild_id) VALUES (?, ?, ?)")?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, guild_id.unwrap_or("")))?;
        statement.next()?;

        // Get the last inserted row id
        let mut stmt = conn.prepare("SELECT last_insert_rowid()")?;
        stmt.next()?;
        let session_id = stmt.read::<i64, _>(0)?;
        Ok(session_id)
    }

    pub async fn update_session_activity(&self, bot_id: &str, session_id: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE interaction_sessions
             SET message_count = message_count + 1, last_activity = CURRENT_TIMESTAMP
             WHERE bot_id = ? AND id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, session_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn end_session(&self, bot_id: &str, session_id: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE interaction_sessions SET session_end = CURRENT_TIMESTAMP WHERE bot_id = ? AND id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, session_id))?;
        statement.next()?;
        Ok(())
    }

    // ========================================================================
    // User Bookmark Methods
    // ========================================================================

    pub async fn add_bookmark(
        &self,
        bot_id: &str,
        user_id: &str,
        channel_id: &str,
        message_id: &str,
        bookmark_name: Option<&str>,
        bookmark_note: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO user_bookmarks (bot_id, user_id, channel_id, message_id, bookmark_name, bookmark_note)
             VALUES (?, ?, ?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, channel_id))?;
        statement.bind((4, message_id))?;
        statement.bind((5, bookmark_name.unwrap_or("")))?;
        statement.bind((6, bookmark_note.unwrap_or("")))?;
        statement.next()?;
        info!("Added bookmark for user {user_id} (bot: {bot_id})");
        Ok(())
    }

    pub async fn get_user_bookmarks(
        &self,
        bot_id: &str,
        user_id: &str,
    ) -> Result<Vec<(String, String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT message_id, channel_id, bookmark_name, bookmark_note
             FROM user_bookmarks WHERE bot_id = ? AND user_id = ?
             ORDER BY created_at DESC",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;

        let mut bookmarks = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let message_id = statement.read::<String, _>(0)?;
            let channel_id = statement.read::<String, _>(1)?;
            let bookmark_name = statement.read::<String, _>(2)?;
            let bookmark_note = statement.read::<String, _>(3)?;
            bookmarks.push((message_id, channel_id, bookmark_name, bookmark_note));
        }
        Ok(bookmarks)
    }

    pub async fn delete_bookmark(&self, bot_id: &str, user_id: &str, message_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM user_bookmarks WHERE bot_id = ? AND user_id = ? AND message_id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, message_id))?;
        statement.next()?;
        Ok(())
    }

    // ========================================================================
    // Reminder Methods
    // ========================================================================

    pub async fn add_reminder(
        &self,
        bot_id: &str,
        user_id: &str,
        channel_id: &str,
        reminder_text: &str,
        remind_at: &str,
    ) -> Result<i64> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO reminders (bot_id, user_id, channel_id, reminder_text, remind_at)
             VALUES (?, ?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, channel_id))?;
        statement.bind((4, reminder_text))?;
        statement.bind((5, remind_at))?;
        statement.next()?;

        let mut stmt = conn.prepare("SELECT last_insert_rowid()")?;
        stmt.next()?;
        let reminder_id = stmt.read::<i64, _>(0)?;
        info!("Added reminder {reminder_id} for user {user_id} (bot: {bot_id})");
        Ok(reminder_id)
    }

    pub async fn get_pending_reminders(&self, bot_id: &str) -> Result<Vec<(i64, String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT id, user_id, channel_id, reminder_text
             FROM reminders
             WHERE bot_id = ? AND completed = 0 AND remind_at <= datetime('now')
             ORDER BY remind_at ASC",
        )?;
        statement.bind((1, bot_id))?;

        let mut reminders = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let id = statement.read::<i64, _>(0)?;
            let user_id = statement.read::<String, _>(1)?;
            let channel_id = statement.read::<String, _>(2)?;
            let reminder_text = statement.read::<String, _>(3)?;
            reminders.push((id, user_id, channel_id, reminder_text));
        }
        Ok(reminders)
    }

    pub async fn complete_reminder(&self, bot_id: &str, reminder_id: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE reminders SET completed = 1, completed_at = CURRENT_TIMESTAMP WHERE bot_id = ? AND id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, reminder_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_user_reminders(
        &self,
        bot_id: &str,
        user_id: &str,
    ) -> Result<Vec<(i64, String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT id, channel_id, reminder_text, remind_at
             FROM reminders
             WHERE bot_id = ? AND user_id = ? AND completed = 0
             ORDER BY remind_at ASC",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;

        let mut reminders = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let id = statement.read::<i64, _>(0)?;
            let channel_id = statement.read::<String, _>(1)?;
            let reminder_text = statement.read::<String, _>(2)?;
            let remind_at = statement.read::<String, _>(3)?;
            reminders.push((id, channel_id, reminder_text, remind_at));
        }
        Ok(reminders)
    }

    pub async fn delete_reminder(&self, bot_id: &str, reminder_id: i64, user_id: &str) -> Result<bool> {
        let conn = self.connection.lock().await;
        let mut statement =
            conn.prepare("DELETE FROM reminders WHERE bot_id = ? AND id = ? AND user_id = ?")?;
        statement.bind((1, bot_id))?;
        statement.bind((2, reminder_id))?;
        statement.bind((3, user_id))?;
        statement.next()?;

        // Check if a row was actually deleted
        let mut check = conn.prepare("SELECT changes()")?;
        check.next()?;
        let changes = check.read::<i64, _>(0)?;

        if changes > 0 {
            info!("Deleted reminder {reminder_id} for user {user_id} (bot: {bot_id})");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    // ========================================================================
    // Custom Command Methods
    // ========================================================================

    pub async fn add_custom_command(
        &self,
        bot_id: &str,
        command_name: &str,
        response_text: &str,
        created_by_user_id: &str,
        guild_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let is_global = guild_id.is_none();
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO custom_commands (bot_id, command_name, response_text, created_by_user_id, guild_id, is_global, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, command_name))?;
        statement.bind((3, response_text))?;
        statement.bind((4, created_by_user_id))?;
        statement.bind((5, guild_id.unwrap_or("")))?;
        statement.bind((6, if is_global { 1i64 } else { 0i64 }))?;
        statement.next()?;
        info!("Added custom command: {command_name} (bot: {bot_id})");
        Ok(())
    }

    pub async fn get_custom_command(
        &self,
        bot_id: &str,
        command_name: &str,
        guild_id: Option<&str>,
    ) -> Result<Option<String>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT response_text FROM custom_commands
             WHERE bot_id = ? AND command_name = ? AND (guild_id = ? OR is_global = 1)
             ORDER BY is_global ASC
             LIMIT 1",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, command_name))?;
        statement.bind((3, guild_id.unwrap_or("")))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<String, _>(0)?))
        } else {
            Ok(None)
        }
    }

    pub async fn delete_custom_command(
        &self,
        bot_id: &str,
        command_name: &str,
        guild_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM custom_commands WHERE bot_id = ? AND command_name = ? AND guild_id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, command_name))?;
        statement.bind((3, guild_id.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    // ========================================================================
    // Analytics Methods
    // ========================================================================

    pub async fn increment_daily_stat(&self, bot_id: &str, stat_type: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

        // First ensure the row exists
        let mut insert_stmt = conn.prepare(
            "INSERT OR IGNORE INTO daily_analytics (bot_id, date, total_messages, total_commands, total_errors)
             VALUES (?, ?, 0, 0, 0)",
        )?;
        insert_stmt.bind((1, bot_id))?;
        insert_stmt.bind((2, date.as_str()))?;
        insert_stmt.next()?;

        // Then update the specific stat
        let update_sql = match stat_type {
            "message" => {
                "UPDATE daily_analytics SET total_messages = total_messages + 1 WHERE bot_id = ? AND date = ?"
            }
            "command" => {
                "UPDATE daily_analytics SET total_commands = total_commands + 1 WHERE bot_id = ? AND date = ?"
            }
            "error" => {
                "UPDATE daily_analytics SET total_errors = total_errors + 1 WHERE bot_id = ? AND date = ?"
            }
            _ => return Ok(()),
        };

        drop(insert_stmt);
        let mut update_stmt = conn.prepare(update_sql)?;
        update_stmt.bind((1, bot_id))?;
        update_stmt.bind((2, date.as_str()))?;
        update_stmt.next()?;

        Ok(())
    }

    pub async fn add_performance_metric(
        &self,
        bot_id: &str,
        metric_type: &str,
        value: f64,
        unit: Option<&str>,
        metadata: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO performance_metrics (bot_id, metric_type, value, unit, metadata) VALUES (?, ?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, metric_type))?;
        statement.bind((3, value))?;
        statement.bind((4, unit.unwrap_or("")))?;
        statement.bind((5, metadata.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    // ========================================================================
    // System Metrics Methods (for /sysinfo command)
    // ========================================================================

    /// Store a system metric snapshot (uses performance_metrics table)
    pub async fn store_system_metric(&self, bot_id: &str, metric_type: &str, value: f64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO performance_metrics (bot_id, metric_type, value, unit, metadata) VALUES (?, ?, ?, 'system', '')",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, metric_type))?;
        statement.bind((3, value))?;
        statement.next()?;
        Ok(())
    }

    /// Get historical metrics data for a specific metric type
    /// Returns (unix_timestamp, value) pairs ordered by time ascending
    pub async fn get_metrics_history(
        &self,
        bot_id: &str,
        metric_type: &str,
        hours: i64,
    ) -> Result<Vec<(i64, f64)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT strftime('%s', timestamp) as unix_time, value
             FROM performance_metrics
             WHERE bot_id = ? AND metric_type = ? AND timestamp >= datetime('now', ? || ' hours')
             ORDER BY timestamp ASC",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, metric_type))?;
        statement.bind((3, format!("-{}", hours).as_str()))?;

        let mut results = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let timestamp_str = statement.read::<String, _>(0)?;
            let timestamp = timestamp_str.parse::<i64>().unwrap_or(0);
            let value = statement.read::<f64, _>(1)?;
            results.push((timestamp, value));
        }
        Ok(results)
    }

    /// Cleanup old metrics data (keep last N days)
    pub async fn cleanup_old_metrics(&self, bot_id: &str, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM performance_metrics WHERE bot_id = ? AND unit = 'system' AND timestamp < datetime('now', ? || ' days')",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, format!("-{}", days).as_str()))?;
        statement.next()?;
        info!("Cleaned up system metrics older than {} days (bot: {bot_id})", days);
        Ok(())
    }

    // ========================================================================
    // Error Logging Methods
    // ========================================================================

    #[allow(clippy::too_many_arguments)]
    pub async fn log_error(
        &self,
        bot_id: &str,
        error_type: &str,
        error_message: &str,
        stack_trace: Option<&str>,
        user_id: Option<&str>,
        channel_id: Option<&str>,
        command: Option<&str>,
        metadata: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO error_logs (bot_id, error_type, error_message, stack_trace, user_id, channel_id, command, metadata)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, error_type))?;
        statement.bind((3, error_message))?;
        statement.bind((4, stack_trace.unwrap_or("")))?;
        statement.bind((5, user_id.unwrap_or("")))?;
        statement.bind((6, channel_id.unwrap_or("")))?;
        statement.bind((7, command.unwrap_or("")))?;
        statement.bind((8, metadata.unwrap_or("")))?;
        statement.next()?;

        // Also increment daily error count
        drop(statement);
        drop(conn);
        self.increment_daily_stat(bot_id, "error").await?;
        Ok(())
    }

    // ========================================================================
    // Feature Flag Methods
    // ========================================================================

    pub async fn set_feature_flag(
        &self,
        bot_id: &str,
        feature_name: &str,
        enabled: bool,
        user_id: Option<&str>,
        guild_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO feature_flags (bot_id, feature_name, enabled, user_id, guild_id, updated_at)
             VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, feature_name))?;
        statement.bind((3, if enabled { 1i64 } else { 0i64 }))?;
        statement.bind((4, user_id.unwrap_or("")))?;
        statement.bind((5, guild_id.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    /// Check if a feature is enabled for a guild
    /// Returns true by default if no record exists (features are enabled unless explicitly disabled)
    pub async fn is_feature_enabled(
        &self,
        bot_id: &str,
        feature_name: &str,
        user_id: Option<&str>,
        guild_id: Option<&str>,
    ) -> Result<bool> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT enabled FROM feature_flags
             WHERE bot_id = ? AND feature_name = ? AND user_id = ? AND guild_id = ?
             LIMIT 1",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, feature_name))?;
        statement.bind((3, user_id.unwrap_or("")))?;
        statement.bind((4, guild_id.unwrap_or("")))?;

        if let Ok(State::Row) = statement.next() {
            let enabled = statement.read::<i64, _>(0)?;
            Ok(enabled == 1)
        } else {
            // Default to enabled if no explicit setting exists
            Ok(true)
        }
    }

    /// Get all feature flags for a guild
    /// Returns a map of feature_name -> enabled status
    pub async fn get_guild_feature_flags(
        &self,
        bot_id: &str,
        guild_id: &str,
    ) -> Result<std::collections::HashMap<String, bool>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT feature_name, enabled FROM feature_flags
             WHERE bot_id = ? AND guild_id = ? AND user_id = ''",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, guild_id))?;

        let mut flags = std::collections::HashMap::new();
        while let Ok(State::Row) = statement.next() {
            let feature_name = statement.read::<String, _>(0)?;
            let enabled = statement.read::<i64, _>(1)? == 1;
            flags.insert(feature_name, enabled);
        }
        Ok(flags)
    }

    /// Record a feature toggle action in the audit trail
    pub async fn record_feature_toggle(
        &self,
        bot_id: &str,
        feature_name: &str,
        version: &str,
        guild_id: Option<&str>,
        toggled_by: &str,
        enabled: bool,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO feature_versions (bot_id, feature_name, version, guild_id, toggled_by, enabled)
             VALUES (?, ?, ?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, feature_name))?;
        statement.bind((3, version))?;
        statement.bind((4, guild_id.unwrap_or("")))?;
        statement.bind((5, toggled_by))?;
        statement.bind((6, if enabled { 1i64 } else { 0i64 }))?;
        statement.next()?;
        info!("Recorded feature toggle: {feature_name} -> {enabled} by {toggled_by} (bot: {bot_id})");
        Ok(())
    }

    // ========================================================================
    // Guild Settings Methods
    // ========================================================================

    pub async fn set_guild_setting(
        &self,
        bot_id: &str,
        guild_id: &str,
        setting_key: &str,
        setting_value: &str,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO guild_settings (bot_id, guild_id, setting_key, setting_value, updated_at)
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, setting_key))?;
        statement.bind((4, setting_value))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_guild_setting(
        &self,
        bot_id: &str,
        guild_id: &str,
        setting_key: &str,
    ) -> Result<Option<String>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT setting_value FROM guild_settings WHERE bot_id = ? AND guild_id = ? AND setting_key = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, setting_key))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<String, _>(0)?))
        } else {
            Ok(None)
        }
    }

    // ========================================================================
    // Bot Settings Methods (global, not per-guild, no bot_id)
    // ========================================================================

    pub async fn set_bot_setting(&self, setting_key: &str, setting_value: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO bot_settings (setting_key, setting_value, updated_at)
             VALUES (?, ?, CURRENT_TIMESTAMP)",
        )?;
        statement.bind((1, setting_key))?;
        statement.bind((2, setting_value))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_bot_setting(&self, setting_key: &str) -> Result<Option<String>> {
        let conn = self.connection.lock().await;
        let mut statement =
            conn.prepare("SELECT setting_value FROM bot_settings WHERE setting_key = ?")?;
        statement.bind((1, setting_key))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<String, _>(0)?))
        } else {
            Ok(None)
        }
    }

    // ========================================================================
    // Extended User Preferences Methods
    // ========================================================================

    pub async fn set_user_preference(
        &self,
        bot_id: &str,
        user_id: &str,
        preference_key: &str,
        preference_value: &str,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO extended_user_preferences (bot_id, user_id, preference_key, preference_value, updated_at)
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, preference_key))?;
        statement.bind((4, preference_value))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_user_preference(
        &self,
        bot_id: &str,
        user_id: &str,
        preference_key: &str,
    ) -> Result<Option<String>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT preference_value FROM extended_user_preferences WHERE bot_id = ? AND user_id = ? AND preference_key = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, preference_key))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<String, _>(0)?))
        } else {
            Ok(None)
        }
    }

    // ========================================================================
    // Conflict Detection & Mediation Methods
    // ========================================================================

    pub async fn record_conflict_detection(
        &self,
        bot_id: &str,
        channel_id: &str,
        guild_id: Option<&str>,
        participants: &str, // JSON array of user IDs
        detection_type: &str,
        confidence: f32,
        last_message_id: &str,
    ) -> Result<i64> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO conflict_detection
             (bot_id, channel_id, guild_id, participants, detection_type, confidence_score, last_message_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, guild_id.unwrap_or("")))?;
        statement.bind((4, participants))?;
        statement.bind((5, detection_type))?;
        statement.bind((6, confidence as f64))?;
        statement.bind((7, last_message_id))?;
        statement.next()?;

        // Get the ID of the inserted row
        let mut id_statement = conn.prepare("SELECT last_insert_rowid()")?;
        id_statement.next()?;
        let conflict_id = id_statement.read::<i64, _>(0)?;

        info!("Recorded conflict detection in channel {channel_id} with confidence {confidence} (bot: {bot_id})");
        Ok(conflict_id)
    }

    pub async fn mark_conflict_resolved(&self, bot_id: &str, conflict_id: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE conflict_detection SET resolved_at = CURRENT_TIMESTAMP WHERE bot_id = ? AND id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, conflict_id))?;
        statement.next()?;
        info!("Marked conflict {conflict_id} as resolved (bot: {bot_id})");
        Ok(())
    }

    pub async fn mark_mediation_triggered(
        &self,
        bot_id: &str,
        conflict_id: i64,
        message_id: &str,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE conflict_detection
             SET mediation_triggered = 1, mediation_message_id = ?
             WHERE bot_id = ? AND id = ?",
        )?;
        statement.bind((1, message_id))?;
        statement.bind((2, bot_id))?;
        statement.bind((3, conflict_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_channel_active_conflict(&self, bot_id: &str, channel_id: &str) -> Result<Option<i64>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT id FROM conflict_detection
             WHERE bot_id = ? AND channel_id = ? AND resolved_at IS NULL
             ORDER BY last_detected DESC LIMIT 1",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, channel_id))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<i64, _>(0)?))
        } else {
            Ok(None)
        }
    }

    pub async fn record_mediation(
        &self,
        bot_id: &str,
        conflict_id: i64,
        channel_id: &str,
        message_text: &str,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO mediation_history (bot_id, conflict_id, channel_id, mediation_message)
             VALUES (?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, conflict_id))?;
        statement.bind((3, channel_id))?;
        statement.bind((4, message_text))?;
        statement.next()?;
        info!("Recorded mediation for conflict {conflict_id} (bot: {bot_id})");
        Ok(())
    }

    /// Get the timestamp of the last mediation in a channel
    pub async fn get_last_mediation_timestamp(
        &self,
        bot_id: &str,
        channel_id: &str,
    ) -> Result<Option<i64>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT strftime('%s', mh.created_at) as unix_time
             FROM mediation_history mh
             WHERE mh.bot_id = ? AND mh.channel_id = ?
             ORDER BY mh.created_at DESC
             LIMIT 1",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, channel_id))?;

        if let Ok(State::Row) = statement.next() {
            let timestamp_str = statement.read::<String, _>(0)?;
            Ok(Some(timestamp_str.parse::<i64>()?))
        } else {
            Ok(None)
        }
    }

    pub async fn get_recent_channel_messages(
        &self,
        bot_id: &str,
        channel_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT user_id, content, strftime('%s', timestamp) as unix_time
             FROM conversation_history
             WHERE bot_id = ? AND channel_id = ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, limit as i64))?;

        let mut messages = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let user_id = statement.read::<String, _>(0)?;
            let content = statement.read::<String, _>(1)?;
            let timestamp = statement.read::<String, _>(2)?;
            messages.push((user_id, content, timestamp));
        }

        // Reverse to get chronological order
        messages.reverse();
        Ok(messages)
    }

    /// Get recent channel messages that occurred after a specific timestamp
    /// This is used to avoid re-analyzing messages that have already been mediated
    pub async fn get_recent_channel_messages_since(
        &self,
        bot_id: &str,
        channel_id: &str,
        since_timestamp: i64,
        limit: usize,
    ) -> Result<Vec<(String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT user_id, content, strftime('%s', timestamp) as unix_time
             FROM conversation_history
             WHERE bot_id = ? AND channel_id = ?
               AND CAST(strftime('%s', timestamp) AS INTEGER) > ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, since_timestamp))?;
        statement.bind((4, limit as i64))?;

        let mut messages = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let user_id = statement.read::<String, _>(0)?;
            let content = statement.read::<String, _>(1)?;
            let timestamp = statement.read::<String, _>(2)?;
            messages.push((user_id, content, timestamp));
        }

        // Reverse to get chronological order
        messages.reverse();
        Ok(messages)
    }

    pub async fn update_user_interaction_pattern(
        &self,
        bot_id: &str,
        user_id_a: &str,
        user_id_b: &str,
        channel_id: &str,
        is_conflict: bool,
    ) -> Result<()> {
        let conn = self.connection.lock().await;

        // Ensure user_id_a is always lexicographically smaller (for consistent lookups)
        let (user_a, user_b) = if user_id_a < user_id_b {
            (user_id_a, user_id_b)
        } else {
            (user_id_b, user_id_a)
        };

        let conflict_increment = if is_conflict { 1 } else { 0 };

        let mut statement = conn.prepare(
            "INSERT INTO user_interaction_patterns
             (bot_id, user_id_a, user_id_b, channel_id, interaction_count, conflict_incidents, last_interaction)
             VALUES (?, ?, ?, ?, 1, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(bot_id, user_id_a, user_id_b, channel_id) DO UPDATE SET
             interaction_count = interaction_count + 1,
             conflict_incidents = conflict_incidents + ?,
             last_interaction = CURRENT_TIMESTAMP",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_a))?;
        statement.bind((3, user_b))?;
        statement.bind((4, channel_id))?;
        statement.bind((5, conflict_increment))?;
        statement.bind((6, conflict_increment))?;
        statement.next()?;
        Ok(())
    }

    // ========================================================================
    // Channel Settings Methods
    // ========================================================================

    /// Get verbosity for a channel, falling back to guild default, then "concise"
    pub async fn get_channel_verbosity(
        &self,
        bot_id: &str,
        guild_id: &str,
        channel_id: &str,
    ) -> Result<String> {
        let conn = self.connection.lock().await;

        // First try channel-specific setting
        let mut statement = conn.prepare(
            "SELECT verbosity FROM channel_settings WHERE bot_id = ? AND guild_id = ? AND channel_id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, channel_id))?;

        if let Ok(State::Row) = statement.next() {
            return Ok(statement.read::<String, _>(0)?);
        }

        // Fall back to guild default
        drop(statement);
        let mut guild_stmt = conn.prepare(
            "SELECT setting_value FROM guild_settings WHERE bot_id = ? AND guild_id = ? AND setting_key = 'default_verbosity'",
        )?;
        guild_stmt.bind((1, bot_id))?;
        guild_stmt.bind((2, guild_id))?;

        if let Ok(State::Row) = guild_stmt.next() {
            return Ok(guild_stmt.read::<String, _>(0)?);
        }

        // Default to concise
        Ok("concise".to_string())
    }

    /// Set verbosity for a specific channel
    pub async fn set_channel_verbosity(
        &self,
        bot_id: &str,
        guild_id: &str,
        channel_id: &str,
        verbosity: &str,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO channel_settings (bot_id, guild_id, channel_id, verbosity, updated_at)
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(bot_id, guild_id, channel_id) DO UPDATE SET
             verbosity = excluded.verbosity,
             updated_at = CURRENT_TIMESTAMP",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, channel_id))?;
        statement.bind((4, verbosity))?;
        statement.next()?;
        info!("Set verbosity for channel {channel_id} to {verbosity} (bot: {bot_id})");
        Ok(())
    }

    /// Get all settings for a channel
    pub async fn get_channel_settings(
        &self,
        bot_id: &str,
        guild_id: &str,
        channel_id: &str,
    ) -> Result<(String, bool)> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT verbosity, conflict_enabled FROM channel_settings WHERE bot_id = ? AND guild_id = ? AND channel_id = ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, channel_id))?;

        if let Ok(State::Row) = statement.next() {
            let verbosity = statement.read::<String, _>(0)?;
            let conflict_enabled = statement.read::<i64, _>(1)? == 1;
            Ok((verbosity, conflict_enabled))
        } else {
            // Return defaults
            Ok(("concise".to_string(), true))
        }
    }

    /// Set whether conflict detection is enabled for a channel
    pub async fn set_channel_conflict_enabled(
        &self,
        bot_id: &str,
        guild_id: &str,
        channel_id: &str,
        enabled: bool,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO channel_settings (bot_id, guild_id, channel_id, conflict_enabled, updated_at)
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(bot_id, guild_id, channel_id) DO UPDATE SET
             conflict_enabled = excluded.conflict_enabled,
             updated_at = CURRENT_TIMESTAMP",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, channel_id))?;
        statement.bind((4, if enabled { 1i64 } else { 0i64 }))?;
        statement.next()?;
        info!("Set conflict_enabled for channel {channel_id} to {enabled} (bot: {bot_id})");
        Ok(())
    }

    /// Check if a user has the bot admin role for a guild
    pub async fn has_bot_admin_role(
        &self,
        bot_id: &str,
        guild_id: &str,
        user_roles: &[String],
    ) -> Result<bool> {
        // Get the bot admin role ID from guild settings
        let admin_role = self.get_guild_setting(bot_id, guild_id, "bot_admin_role").await?;

        if let Some(role_id) = admin_role {
            Ok(user_roles.iter().any(|r| r == &role_id))
        } else {
            // No bot admin role set - only Discord admins can manage
            Ok(false)
        }
    }

    // ========================================================================
    // OpenAI Usage Tracking Methods
    // ========================================================================

    /// Log a ChatCompletion (GPT) usage event
    #[allow(clippy::too_many_arguments)]
    pub async fn log_openai_chat_usage(
        &self,
        bot_id: &str,
        model: &str,
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
        estimated_cost: f64,
        user_id: &str,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
        request_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

        // Insert into raw usage table
        let mut statement = conn.prepare(
            "INSERT INTO openai_usage
             (bot_id, request_id, user_id, guild_id, channel_id, service_type, model,
              input_tokens, output_tokens, total_tokens, estimated_cost_usd)
             VALUES (?, ?, ?, ?, ?, 'chat', ?, ?, ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, request_id.unwrap_or("")))?;
        statement.bind((3, user_id))?;
        statement.bind((4, guild_id.unwrap_or("")))?;
        statement.bind((5, channel_id.unwrap_or("")))?;
        statement.bind((6, model))?;
        statement.bind((7, input_tokens as i64))?;
        statement.bind((8, output_tokens as i64))?;
        statement.bind((9, total_tokens as i64))?;
        statement.bind((10, estimated_cost))?;
        statement.next()?;

        // Update daily aggregate
        drop(statement);
        let mut agg_stmt = conn.prepare(
            "INSERT INTO openai_usage_daily
             (bot_id, date, guild_id, user_id, service_type, request_count, total_tokens, total_cost_usd)
             VALUES (?, ?, ?, ?, 'chat', 1, ?, ?)
             ON CONFLICT(bot_id, date, guild_id, user_id, service_type) DO UPDATE SET
             request_count = request_count + 1,
             total_tokens = total_tokens + excluded.total_tokens,
             total_cost_usd = total_cost_usd + excluded.total_cost_usd",
        )?;
        agg_stmt.bind((1, bot_id))?;
        agg_stmt.bind((2, date.as_str()))?;
        agg_stmt.bind((3, guild_id.unwrap_or("")))?;
        agg_stmt.bind((4, user_id))?;
        agg_stmt.bind((5, total_tokens as i64))?;
        agg_stmt.bind((6, estimated_cost))?;
        agg_stmt.next()?;

        Ok(())
    }

    /// Log a Whisper (audio transcription) usage event
    pub async fn log_openai_whisper_usage(
        &self,
        bot_id: &str,
        audio_duration_seconds: f64,
        estimated_cost: f64,
        user_id: &str,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

        // Insert into raw usage table
        let mut statement = conn.prepare(
            "INSERT INTO openai_usage
             (bot_id, user_id, guild_id, channel_id, service_type, model,
              audio_duration_seconds, estimated_cost_usd)
             VALUES (?, ?, ?, ?, 'whisper', 'whisper-1', ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, guild_id.unwrap_or("")))?;
        statement.bind((4, channel_id.unwrap_or("")))?;
        statement.bind((5, audio_duration_seconds))?;
        statement.bind((6, estimated_cost))?;
        statement.next()?;

        // Update daily aggregate
        drop(statement);
        let mut agg_stmt = conn.prepare(
            "INSERT INTO openai_usage_daily
             (bot_id, date, guild_id, user_id, service_type, request_count, total_audio_seconds, total_cost_usd)
             VALUES (?, ?, ?, ?, 'whisper', 1, ?, ?)
             ON CONFLICT(bot_id, date, guild_id, user_id, service_type) DO UPDATE SET
             request_count = request_count + 1,
             total_audio_seconds = total_audio_seconds + excluded.total_audio_seconds,
             total_cost_usd = total_cost_usd + excluded.total_cost_usd",
        )?;
        agg_stmt.bind((1, bot_id))?;
        agg_stmt.bind((2, date.as_str()))?;
        agg_stmt.bind((3, guild_id.unwrap_or("")))?;
        agg_stmt.bind((4, user_id))?;
        agg_stmt.bind((5, audio_duration_seconds))?;
        agg_stmt.bind((6, estimated_cost))?;
        agg_stmt.next()?;

        Ok(())
    }

    /// Log a DALL-E (image generation) usage event
    pub async fn log_openai_dalle_usage(
        &self,
        bot_id: &str,
        image_size: &str,
        image_count: u32,
        estimated_cost: f64,
        user_id: &str,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

        // Insert into raw usage table
        let mut statement = conn.prepare(
            "INSERT INTO openai_usage
             (bot_id, user_id, guild_id, channel_id, service_type, model,
              image_count, image_size, estimated_cost_usd)
             VALUES (?, ?, ?, ?, 'dalle', 'dall-e-3', ?, ?, ?)",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, guild_id.unwrap_or("")))?;
        statement.bind((4, channel_id.unwrap_or("")))?;
        statement.bind((5, image_count as i64))?;
        statement.bind((6, image_size))?;
        statement.bind((7, estimated_cost))?;
        statement.next()?;

        // Update daily aggregate
        drop(statement);
        let mut agg_stmt = conn.prepare(
            "INSERT INTO openai_usage_daily
             (bot_id, date, guild_id, user_id, service_type, request_count, total_images, total_cost_usd)
             VALUES (?, ?, ?, ?, 'dalle', 1, ?, ?)
             ON CONFLICT(bot_id, date, guild_id, user_id, service_type) DO UPDATE SET
             request_count = request_count + 1,
             total_images = total_images + excluded.total_images,
             total_cost_usd = total_cost_usd + excluded.total_cost_usd",
        )?;
        agg_stmt.bind((1, bot_id))?;
        agg_stmt.bind((2, date.as_str()))?;
        agg_stmt.bind((3, guild_id.unwrap_or("")))?;
        agg_stmt.bind((4, user_id))?;
        agg_stmt.bind((5, image_count as i64))?;
        agg_stmt.bind((6, estimated_cost))?;
        agg_stmt.next()?;

        Ok(())
    }

    /// Get usage statistics for a user within a date range
    /// Returns (service_type, request_count, tokens, audio_seconds, images, cost)
    pub async fn get_user_usage_stats(
        &self,
        bot_id: &str,
        user_id: &str,
        days: i64,
    ) -> Result<Vec<(String, i64, i64, f64, i64, f64)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT service_type,
                    SUM(request_count) as requests,
                    SUM(total_tokens) as tokens,
                    SUM(total_audio_seconds) as audio_secs,
                    SUM(total_images) as images,
                    SUM(total_cost_usd) as cost
             FROM openai_usage_daily
             WHERE bot_id = ? AND user_id = ? AND date >= date('now', ? || ' days')
             GROUP BY service_type",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, format!("-{}", days).as_str()))?;

        let mut results = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let service_type = statement.read::<String, _>(0)?;
            let requests = statement.read::<i64, _>(1)?;
            let tokens = statement.read::<i64, _>(2)?;
            let audio_secs = statement.read::<f64, _>(3)?;
            let images = statement.read::<i64, _>(4)?;
            let cost = statement.read::<f64, _>(5)?;
            results.push((service_type, requests, tokens, audio_secs, images, cost));
        }
        Ok(results)
    }

    /// Get usage statistics for an entire guild within a date range
    /// Returns (service_type, request_count, tokens, audio_seconds, images, cost)
    pub async fn get_guild_usage_stats(
        &self,
        bot_id: &str,
        guild_id: &str,
        days: i64,
    ) -> Result<Vec<(String, i64, i64, f64, i64, f64)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT service_type,
                    SUM(request_count) as requests,
                    SUM(total_tokens) as tokens,
                    SUM(total_audio_seconds) as audio_secs,
                    SUM(total_images) as images,
                    SUM(total_cost_usd) as cost
             FROM openai_usage_daily
             WHERE bot_id = ? AND guild_id = ? AND date >= date('now', ? || ' days')
             GROUP BY service_type",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, format!("-{}", days).as_str()))?;

        let mut results = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let service_type = statement.read::<String, _>(0)?;
            let requests = statement.read::<i64, _>(1)?;
            let tokens = statement.read::<i64, _>(2)?;
            let audio_secs = statement.read::<f64, _>(3)?;
            let images = statement.read::<i64, _>(4)?;
            let cost = statement.read::<f64, _>(5)?;
            results.push((service_type, requests, tokens, audio_secs, images, cost));
        }
        Ok(results)
    }

    /// Get top users by cost for a guild
    /// Returns (user_id, request_count, total_cost)
    pub async fn get_guild_top_users_by_cost(
        &self,
        bot_id: &str,
        guild_id: &str,
        days: i64,
        limit: i64,
    ) -> Result<Vec<(String, i64, f64)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT user_id,
                    SUM(request_count) as requests,
                    SUM(total_cost_usd) as cost
             FROM openai_usage_daily
             WHERE bot_id = ? AND guild_id = ? AND user_id != '' AND date >= date('now', ? || ' days')
             GROUP BY user_id
             ORDER BY cost DESC
             LIMIT ?",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, format!("-{}", days).as_str()))?;
        statement.bind((4, limit))?;

        let mut results = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let user_id = statement.read::<String, _>(0)?;
            let requests = statement.read::<i64, _>(1)?;
            let cost = statement.read::<f64, _>(2)?;
            results.push((user_id, requests, cost));
        }
        Ok(results)
    }

    /// Cleanup old raw usage data (keep last N days)
    pub async fn cleanup_old_openai_usage(&self, bot_id: &str, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM openai_usage WHERE bot_id = ? AND timestamp < datetime('now', ? || ' days')",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, format!("-{}", days).as_str()))?;
        statement.next()?;
        info!("Cleaned up openai_usage older than {} days (bot: {bot_id})", days);
        Ok(())
    }

    /// Cleanup old daily aggregates (keep last N days)
    pub async fn cleanup_old_openai_usage_daily(&self, bot_id: &str, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM openai_usage_daily WHERE bot_id = ? AND date < date('now', ? || ' days')",
        )?;
        statement.bind((1, bot_id))?;
        statement.bind((2, format!("-{}", days).as_str()))?;
        statement.next()?;
        info!("Cleaned up openai_usage_daily older than {} days (bot: {bot_id})", days);
        Ok(())
    }

    // ========================================================================
    // Startup Notification Session Methods
    // ========================================================================

    /// Get a recent startup session (within N minutes)
    pub async fn get_recent_startup_session(&self, minutes: i64) -> Result<Option<String>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT session_id FROM bot_startup_session
             WHERE last_updated >= datetime('now', ? || ' minutes')
             ORDER BY last_updated DESC LIMIT 1",
        )?;
        statement.bind((1, format!("-{}", minutes).as_str()))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<String, _>(0)?))
        } else {
            Ok(None)
        }
    }

    /// Create a new startup session
    pub async fn create_startup_session(&self, session_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO bot_startup_session (session_id) VALUES (?)",
        )?;
        statement.bind((1, session_id))?;
        statement.next()?;
        info!("Created startup session: {}", session_id);
        Ok(())
    }

    /// Get session info
    pub async fn get_startup_session(&self, session_id: &str) -> Result<StartupSession> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT session_id, owner_message_id, channel_message_id, created_at, last_updated
             FROM bot_startup_session WHERE session_id = ?",
        )?;
        statement.bind((1, session_id))?;

        if let Ok(State::Row) = statement.next() {
            Ok(StartupSession {
                session_id: statement.read::<String, _>(0)?,
                owner_message_id: statement.read::<String, _>(1).ok(),
                channel_message_id: statement.read::<String, _>(2).ok(),
                created_at: statement.read::<String, _>(3)?,
                last_updated: statement.read::<String, _>(4)?,
            })
        } else {
            anyhow::bail!("Session not found: {}", session_id)
        }
    }

    /// Update owner message ID for session
    pub async fn update_session_owner_message(&self, session_id: &str, message_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE bot_startup_session
             SET owner_message_id = ?, last_updated = CURRENT_TIMESTAMP
             WHERE session_id = ?",
        )?;
        statement.bind((1, message_id))?;
        statement.bind((2, session_id))?;
        statement.next()?;
        Ok(())
    }

    /// Update channel message ID for session
    pub async fn update_session_channel_message(&self, session_id: &str, message_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE bot_startup_session
             SET channel_message_id = ?, last_updated = CURRENT_TIMESTAMP
             WHERE session_id = ?",
        )?;
        statement.bind((1, message_id))?;
        statement.bind((2, session_id))?;
        statement.next()?;
        Ok(())
    }

    /// Touch session to update last_updated timestamp
    pub async fn touch_startup_session(&self, session_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE bot_startup_session SET last_updated = CURRENT_TIMESTAMP WHERE session_id = ?",
        )?;
        statement.bind((1, session_id))?;
        statement.next()?;
        Ok(())
    }

    /// Add bot to startup session
    pub async fn add_bot_to_startup_session(
        &self,
        session_id: &str,
        bot_id: &str,
        bot_name: &str,
        version: &str,
        guilds_count: i64,
        shard_info: Option<String>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO bot_startup_bots
             (session_id, bot_id, bot_name, version, guilds_count, shard_info, started_at)
             VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )?;
        statement.bind((1, session_id))?;
        statement.bind((2, bot_id))?;
        statement.bind((3, bot_name))?;
        statement.bind((4, version))?;
        statement.bind((5, guilds_count))?;
        statement.bind((6, shard_info.as_deref().unwrap_or("")))?;
        statement.next()?;
        info!("Added bot {} to session {}", bot_name, session_id);
        Ok(())
    }

    /// Get all bots in a session
    pub async fn get_bots_in_session(&self, session_id: &str) -> Result<Vec<StartedBot>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT bot_id, bot_name, version, guilds_count, shard_info, started_at
             FROM bot_startup_bots
             WHERE session_id = ?
             ORDER BY started_at ASC",
        )?;
        statement.bind((1, session_id))?;

        let mut bots = Vec::new();
        while let Ok(State::Row) = statement.next() {
            let shard_info_str = statement.read::<String, _>(4)?;
            let started_at_str = statement.read::<String, _>(5)?;

            bots.push(StartedBot {
                bot_id: statement.read::<String, _>(0)?,
                bot_name: statement.read::<String, _>(1)?,
                version: statement.read::<String, _>(2)?,
                guilds_count: statement.read::<i64, _>(3)?,
                shard_info: if shard_info_str.is_empty() { None } else { Some(shard_info_str) },
                started_at: chrono::DateTime::parse_from_rfc3339(&started_at_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            });
        }
        Ok(bots)
    }

    /// Cleanup old startup sessions (older than N hours)
    pub async fn cleanup_old_startup_sessions(&self, hours: i64) -> Result<()> {
        let conn = self.connection.lock().await;

        // Delete bots first (to avoid foreign key issues if they existed)
        let mut stmt1 = conn.prepare(
            "DELETE FROM bot_startup_bots
             WHERE session_id IN (
                 SELECT session_id FROM bot_startup_session
                 WHERE created_at < datetime('now', ? || ' hours')
             )",
        )?;
        stmt1.bind((1, format!("-{}", hours).as_str()))?;
        stmt1.next()?;

        // Then delete sessions
        let mut stmt2 = conn.prepare(
            "DELETE FROM bot_startup_session WHERE created_at < datetime('now', ? || ' hours')",
        )?;
        stmt2.bind((1, format!("-{}", hours).as_str()))?;
        stmt2.next()?;

        info!("Cleaned up startup sessions older than {} hours", hours);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bot_data_isolation() {
        let db = Database::new(":memory:").await.unwrap();

        // Set persona for bot1
        db.set_user_persona("bot1", "user123", "muppet").await.unwrap();

        // Set different persona for same user on bot2
        db.set_user_persona("bot2", "user123", "chef").await.unwrap();

        // Verify isolation
        assert_eq!(
            db.get_user_persona("bot1", "user123").await.unwrap(),
            "muppet"
        );
        assert_eq!(
            db.get_user_persona("bot2", "user123").await.unwrap(),
            "chef"
        );
    }

    #[tokio::test]
    async fn test_conversation_history_isolation() {
        let db = Database::new(":memory:").await.unwrap();

        // Store message for bot1
        db.store_message("bot1", "user1", "channel1", "user", "Hello from bot1", None)
            .await
            .unwrap();

        // Store message for bot2 in same channel
        db.store_message("bot2", "user1", "channel1", "user", "Hello from bot2", None)
            .await
            .unwrap();

        // Verify isolation
        let bot1_history = db
            .get_conversation_history("bot1", "user1", "channel1", 10)
            .await
            .unwrap();
        let bot2_history = db
            .get_conversation_history("bot2", "user1", "channel1", 10)
            .await
            .unwrap();

        assert_eq!(bot1_history.len(), 1);
        assert_eq!(bot1_history[0].1, "Hello from bot1");

        assert_eq!(bot2_history.len(), 1);
        assert_eq!(bot2_history[0].1, "Hello from bot2");
    }

    #[tokio::test]
    async fn test_guild_settings_isolation() {
        let db = Database::new(":memory:").await.unwrap();

        // Set guild setting for bot1
        db.set_guild_setting("bot1", "guild123", "default_persona", "obi")
            .await
            .unwrap();

        // Set different setting for bot2
        db.set_guild_setting("bot2", "guild123", "default_persona", "chef")
            .await
            .unwrap();

        // Verify isolation
        assert_eq!(
            db.get_guild_setting("bot1", "guild123", "default_persona")
                .await
                .unwrap(),
            Some("obi".to_string())
        );
        assert_eq!(
            db.get_guild_setting("bot2", "guild123", "default_persona")
                .await
                .unwrap(),
            Some("chef".to_string())
        );
    }

    #[tokio::test]
    async fn test_reminders_isolation() {
        let db = Database::new(":memory:").await.unwrap();

        // Add reminder for bot1
        let id1 = db
            .add_reminder("bot1", "user1", "channel1", "Reminder for bot1", "2099-12-31 23:59:59")
            .await
            .unwrap();

        // Add reminder for bot2
        let id2 = db
            .add_reminder("bot2", "user1", "channel1", "Reminder for bot2", "2099-12-31 23:59:59")
            .await
            .unwrap();

        // Verify isolation - get_user_reminders returns reminders per bot
        let bot1_reminders = db.get_user_reminders("bot1", "user1").await.unwrap();
        let bot2_reminders = db.get_user_reminders("bot2", "user1").await.unwrap();

        assert_eq!(bot1_reminders.len(), 1);
        assert_eq!(bot1_reminders[0].0, id1);
        assert_eq!(bot1_reminders[0].2, "Reminder for bot1");

        assert_eq!(bot2_reminders.len(), 1);
        assert_eq!(bot2_reminders[0].0, id2);
        assert_eq!(bot2_reminders[0].2, "Reminder for bot2");
    }

    #[tokio::test]
    async fn test_default_bot_id() {
        // Verify that DEFAULT_BOT_ID constant is set correctly
        assert_eq!(DEFAULT_BOT_ID, "default");
    }
}
