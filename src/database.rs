use anyhow::Result;
use log::info;
use sqlite::{Connection, State};
use std::sync::Arc;
use tokio::sync::Mutex;

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
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS user_preferences (
                user_id TEXT PRIMARY KEY,
                default_persona TEXT DEFAULT 'obi',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS usage_stats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                command TEXT NOT NULL,
                persona TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS conversation_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                persona TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_channel
             ON conversation_history(user_id, channel_id)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp
             ON conversation_history(timestamp)",
        )?;

        // Enhanced Interaction Tracking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS message_metadata (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
            "CREATE INDEX IF NOT EXISTS idx_message_id
             ON message_metadata(message_id)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS interaction_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
             ON interaction_sessions(user_id, session_start)",
        )?;

        // Feature-Specific Data
        conn.execute(
            "CREATE TABLE IF NOT EXISTS user_bookmarks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
             ON user_bookmarks(user_id)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS reminders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
             ON reminders(remind_at, completed)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS custom_commands (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                command_name TEXT NOT NULL,
                response_text TEXT NOT NULL,
                created_by_user_id TEXT NOT NULL,
                guild_id TEXT,
                is_global BOOLEAN DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(command_name, guild_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_custom_command
             ON custom_commands(command_name, guild_id)",
        )?;

        // Analytics & Metrics
        conn.execute(
            "CREATE TABLE IF NOT EXISTS daily_analytics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date DATE UNIQUE NOT NULL,
                total_messages INTEGER DEFAULT 0,
                unique_users INTEGER DEFAULT 0,
                total_commands INTEGER DEFAULT 0,
                total_errors INTEGER DEFAULT 0,
                persona_usage TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_analytics_date
             ON daily_analytics(date)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS performance_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                metric_type TEXT NOT NULL,
                value REAL NOT NULL,
                unit TEXT,
                metadata TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_metrics_type
             ON performance_metrics(metric_type, timestamp)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS error_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
             ON error_logs(error_type, timestamp)",
        )?;

        // Extended Configuration
        conn.execute(
            "CREATE TABLE IF NOT EXISTS feature_flags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                feature_name TEXT NOT NULL,
                enabled BOOLEAN DEFAULT 0,
                user_id TEXT,
                guild_id TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(feature_name, user_id, guild_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_feature_flag
             ON feature_flags(feature_name, user_id, guild_id)",
        )?;

        // Feature versions tracking for audit trail
        conn.execute(
            "CREATE TABLE IF NOT EXISTS feature_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
             ON feature_versions(feature_name, guild_id, changed_at)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS guild_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                guild_id TEXT NOT NULL,
                setting_key TEXT NOT NULL,
                setting_value TEXT,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(guild_id, setting_key)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_guild_setting
             ON guild_settings(guild_id, setting_key)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS extended_user_preferences (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                preference_key TEXT NOT NULL,
                preference_value TEXT,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(user_id, preference_key)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_pref
             ON extended_user_preferences(user_id, preference_key)",
        )?;

        // Conflict Detection & Mediation
        conn.execute(
            "CREATE TABLE IF NOT EXISTS conflict_detection (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
             ON conflict_detection(channel_id, guild_id)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conflict_timestamp
             ON conflict_detection(first_detected)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS mediation_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
             ON mediation_history(conflict_id)",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS user_interaction_patterns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id_a TEXT NOT NULL,
                user_id_b TEXT NOT NULL,
                channel_id TEXT,
                guild_id TEXT,
                interaction_count INTEGER DEFAULT 0,
                last_interaction DATETIME,
                conflict_incidents INTEGER DEFAULT 0,
                avg_response_time_ms INTEGER,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(user_id_a, user_id_b, channel_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_interaction_users
             ON user_interaction_patterns(user_id_a, user_id_b)",
        )?;

        // Channel Settings (for per-channel verbosity and other settings)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS channel_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                guild_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                verbosity TEXT DEFAULT 'concise',
                conflict_enabled BOOLEAN DEFAULT 1,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(guild_id, channel_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_channel_settings_guild
             ON channel_settings(guild_id)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_channel_settings_channel
             ON channel_settings(channel_id)",
        )?;

        // Bot Settings (for global bot configuration, not per-guild)
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
             ON openai_usage(user_id, timestamp)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_openai_usage_guild_ts
             ON openai_usage(guild_id, timestamp)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_openai_usage_timestamp
             ON openai_usage(timestamp)",
        )?;

        // Daily aggregates for fast queries (90-day retention)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS openai_usage_daily (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date DATE NOT NULL,
                guild_id TEXT,
                user_id TEXT,
                service_type TEXT NOT NULL,
                request_count INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                total_audio_seconds REAL DEFAULT 0,
                total_images INTEGER DEFAULT 0,
                total_cost_usd REAL DEFAULT 0,
                UNIQUE(date, guild_id, user_id, service_type)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_openai_daily_guild_date
             ON openai_usage_daily(guild_id, date)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_openai_daily_user_date
             ON openai_usage_daily(user_id, date)",
        )?;

        // DM Interaction Tracking Tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dm_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT UNIQUE NOT NULL,
                user_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                last_activity_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                ended_at DATETIME,
                end_reason TEXT,
                message_count INTEGER DEFAULT 0,
                user_message_count INTEGER DEFAULT 0,
                bot_message_count INTEGER DEFAULT 0,
                total_user_chars INTEGER DEFAULT 0,
                total_bot_chars INTEGER DEFAULT 0,
                avg_response_time_ms INTEGER
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_dm_sessions_user
             ON dm_sessions(user_id, started_at DESC)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_dm_sessions_active
             ON dm_sessions(session_id) WHERE ended_at IS NULL",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS dm_session_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT UNIQUE NOT NULL,
                total_api_calls INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                total_api_cost_usd REAL DEFAULT 0,
                chat_calls INTEGER DEFAULT 0,
                whisper_calls INTEGER DEFAULT 0,
                dalle_calls INTEGER DEFAULT 0,
                audio_transcriptions INTEGER DEFAULT 0,
                slash_commands_used INTEGER DEFAULT 0,
                conversation_depth INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES dm_sessions(session_id)
            )",
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS dm_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                user_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                event_data TEXT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES dm_sessions(session_id)
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_dm_events_session
             ON dm_events(session_id, timestamp)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_dm_events_type
             ON dm_events(event_type, timestamp)",
        )?;

        // Plugin Jobs Table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS plugin_jobs (
                id TEXT PRIMARY KEY,
                plugin_name TEXT NOT NULL,
                user_id TEXT NOT NULL,
                guild_id TEXT,
                channel_id TEXT NOT NULL,
                thread_id TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                params TEXT,
                result TEXT,
                error TEXT,
                started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                completed_at DATETIME
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_plugin_jobs_status
             ON plugin_jobs(status, started_at)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_plugin_jobs_user
             ON plugin_jobs(user_id, plugin_name, started_at)",
        )?;

        // Add parent_playlist_id column to plugin_jobs if it doesn't exist
        // SQLite doesn't support ADD COLUMN IF NOT EXISTS, so we check pragmatically
        let has_parent_playlist_id: bool = {
            let mut stmt = conn.prepare("PRAGMA table_info(plugin_jobs)")?;
            let mut found = false;
            while let Ok(State::Row) = stmt.next() {
                let col_name: String = stmt.read(1)?;
                if col_name == "parent_playlist_id" {
                    found = true;
                    break;
                }
            }
            found
        };

        if !has_parent_playlist_id {
            conn.execute("ALTER TABLE plugin_jobs ADD COLUMN parent_playlist_id TEXT")?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_plugin_jobs_parent
                 ON plugin_jobs(parent_playlist_id)",
            )?;
        }

        // Playlist Jobs Table (for multi-video transcription)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS playlist_jobs (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                guild_id TEXT,
                channel_id TEXT NOT NULL,
                thread_id TEXT,
                playlist_url TEXT NOT NULL,
                playlist_id TEXT NOT NULL,
                playlist_title TEXT,
                total_videos INTEGER NOT NULL,
                completed_videos INTEGER DEFAULT 0,
                failed_videos INTEGER DEFAULT 0,
                skipped_videos INTEGER DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending',
                max_videos INTEGER,
                current_video_job_id TEXT,
                error TEXT,
                started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                completed_at DATETIME,
                cancelled_at DATETIME,
                cancelled_by TEXT
            )",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_playlist_jobs_status
             ON playlist_jobs(status, started_at)",
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_playlist_jobs_user
             ON playlist_jobs(user_id, started_at)",
        )?;

        Ok(())
    }

    pub async fn get_user_persona(&self, user_id: &str) -> Result<String> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare("SELECT default_persona FROM user_preferences WHERE user_id = ?")?;
        statement.bind((1, user_id))?;

        if let Ok(State::Row) = statement.next() {
            Ok(statement.read::<String, _>("default_persona")?)
        } else {
            // Check for PERSONA environment variable, fallback to 'obi'
            Ok(std::env::var("PERSONA").unwrap_or_else(|_| "obi".to_string()))
        }
    }

    /// Get user persona with guild default fallback
    /// Cascade: user preference -> guild default -> env var -> "obi"
    pub async fn get_user_persona_with_guild(&self, user_id: &str, guild_id: Option<&str>) -> Result<String> {
        let conn = self.connection.lock().await;

        // First check user preference
        let mut statement = conn.prepare("SELECT default_persona FROM user_preferences WHERE user_id = ?")?;
        statement.bind((1, user_id))?;

        if let Ok(State::Row) = statement.next() {
            return Ok(statement.read::<String, _>("default_persona")?);
        }

        // Check guild default if guild_id is provided
        if let Some(gid) = guild_id {
            drop(statement);
            let mut guild_stmt = conn.prepare(
                "SELECT setting_value FROM guild_settings WHERE guild_id = ? AND setting_key = 'default_persona'"
            )?;
            guild_stmt.bind((1, gid))?;

            if let Ok(State::Row) = guild_stmt.next() {
                return Ok(guild_stmt.read::<String, _>(0)?);
            }
        }

        // Fall back to PERSONA environment variable, then 'obi'
        Ok(std::env::var("PERSONA").unwrap_or_else(|_| "obi".to_string()))
    }

    pub async fn set_user_persona(&self, user_id: &str, persona: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO user_preferences (user_id, default_persona, updated_at) 
             VALUES (?, ?, CURRENT_TIMESTAMP)",
        )?;
        
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO user_preferences (user_id, default_persona, updated_at) 
             VALUES (?, ?, CURRENT_TIMESTAMP)"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, persona))?;
        statement.next()?;
        
        info!("Updated persona for user {user_id} to {persona}");
        Ok(())
    }

    pub async fn log_usage(&self, user_id: &str, command: &str, persona: Option<&str>) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO usage_stats (user_id, command, persona) VALUES (?, ?, ?)"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, command))?;
        statement.bind((3, persona.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    pub async fn store_message(&self, user_id: &str, channel_id: &str, role: &str, content: &str, persona: Option<&str>) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO conversation_history (user_id, channel_id, role, content, persona) VALUES (?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, role))?;
        statement.bind((4, content))?;
        statement.bind((5, persona.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_conversation_history(&self, user_id: &str, channel_id: &str, limit: i64) -> Result<Vec<(String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT role, content FROM conversation_history
             WHERE user_id = ? AND channel_id = ?
             ORDER BY timestamp DESC
             LIMIT ?"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, limit))?;

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

    pub async fn clear_conversation_history(&self, user_id: &str, channel_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM conversation_history WHERE user_id = ? AND channel_id = ?"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, channel_id))?;
        statement.next()?;
        info!("Cleared conversation history for user {user_id} in channel {channel_id}");
        Ok(())
    }

    pub async fn cleanup_old_messages(&self, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM conversation_history WHERE timestamp < datetime('now', ? || ' days')"
        )?;
        statement.bind((1, format!("-{days}").as_str()))?;
        statement.next()?;
        info!("Cleaned up conversation history older than {days} days");
        Ok(())
    }

    // Message Metadata Methods
    pub async fn store_message_metadata(
        &self,
        message_id: &str,
        user_id: &str,
        channel_id: &str,
        attachment_urls: Option<&str>,
        embed_data: Option<&str>,
        reactions: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO message_metadata (message_id, user_id, channel_id, attachment_urls, embed_data, reactions)
             VALUES (?, ?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, message_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, channel_id))?;
        statement.bind((4, attachment_urls.unwrap_or("")))?;
        statement.bind((5, embed_data.unwrap_or("")))?;
        statement.bind((6, reactions.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    pub async fn update_message_metadata_reactions(&self, message_id: &str, reactions: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE message_metadata SET reactions = ? WHERE message_id = ?"
        )?;
        statement.bind((1, reactions))?;
        statement.bind((2, message_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn mark_message_deleted(&self, message_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE message_metadata SET deleted_at = CURRENT_TIMESTAMP WHERE message_id = ?"
        )?;
        statement.bind((1, message_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn mark_message_edited(&self, message_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE message_metadata SET edited_at = CURRENT_TIMESTAMP WHERE message_id = ?"
        )?;
        statement.bind((1, message_id))?;
        statement.next()?;
        Ok(())
    }

    // Interaction Session Methods
    pub async fn start_session(&self, user_id: &str, guild_id: Option<&str>) -> Result<i64> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO interaction_sessions (user_id, guild_id) VALUES (?, ?)"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, guild_id.unwrap_or("")))?;
        statement.next()?;

        // Get the last inserted row id
        let mut stmt = conn.prepare("SELECT last_insert_rowid()")?;
        stmt.next()?;
        let session_id = stmt.read::<i64, _>(0)?;
        Ok(session_id)
    }

    pub async fn update_session_activity(&self, session_id: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE interaction_sessions
             SET message_count = message_count + 1, last_activity = CURRENT_TIMESTAMP
             WHERE id = ?"
        )?;
        statement.bind((1, session_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn end_session(&self, session_id: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE interaction_sessions SET session_end = CURRENT_TIMESTAMP WHERE id = ?"
        )?;
        statement.bind((1, session_id))?;
        statement.next()?;
        Ok(())
    }

    // User Bookmark Methods
    pub async fn add_bookmark(
        &self,
        user_id: &str,
        channel_id: &str,
        message_id: &str,
        bookmark_name: Option<&str>,
        bookmark_note: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO user_bookmarks (user_id, channel_id, message_id, bookmark_name, bookmark_note)
             VALUES (?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, message_id))?;
        statement.bind((4, bookmark_name.unwrap_or("")))?;
        statement.bind((5, bookmark_note.unwrap_or("")))?;
        statement.next()?;
        info!("Added bookmark for user {user_id}");
        Ok(())
    }

    pub async fn get_user_bookmarks(&self, user_id: &str) -> Result<Vec<(String, String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT message_id, channel_id, bookmark_name, bookmark_note
             FROM user_bookmarks WHERE user_id = ?
             ORDER BY created_at DESC"
        )?;
        statement.bind((1, user_id))?;

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

    pub async fn delete_bookmark(&self, user_id: &str, message_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM user_bookmarks WHERE user_id = ? AND message_id = ?"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, message_id))?;
        statement.next()?;
        Ok(())
    }

    // Reminder Methods
    pub async fn add_reminder(
        &self,
        user_id: &str,
        channel_id: &str,
        reminder_text: &str,
        remind_at: &str,
    ) -> Result<i64> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO reminders (user_id, channel_id, reminder_text, remind_at)
             VALUES (?, ?, ?, ?)"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, reminder_text))?;
        statement.bind((4, remind_at))?;
        statement.next()?;

        let mut stmt = conn.prepare("SELECT last_insert_rowid()")?;
        stmt.next()?;
        let reminder_id = stmt.read::<i64, _>(0)?;
        info!("Added reminder {reminder_id} for user {user_id}");
        Ok(reminder_id)
    }

    pub async fn get_pending_reminders(&self) -> Result<Vec<(i64, String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT id, user_id, channel_id, reminder_text
             FROM reminders
             WHERE completed = 0 AND remind_at <= datetime('now')
             ORDER BY remind_at ASC"
        )?;

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

    pub async fn complete_reminder(&self, reminder_id: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE reminders SET completed = 1, completed_at = CURRENT_TIMESTAMP WHERE id = ?"
        )?;
        statement.bind((1, reminder_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_user_reminders(&self, user_id: &str) -> Result<Vec<(i64, String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT id, channel_id, reminder_text, remind_at
             FROM reminders
             WHERE user_id = ? AND completed = 0
             ORDER BY remind_at ASC"
        )?;
        statement.bind((1, user_id))?;

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

    pub async fn delete_reminder(&self, reminder_id: i64, user_id: &str) -> Result<bool> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM reminders WHERE id = ? AND user_id = ?"
        )?;
        statement.bind((1, reminder_id))?;
        statement.bind((2, user_id))?;
        statement.next()?;

        // Check if a row was actually deleted
        let mut check = conn.prepare("SELECT changes()")?;
        check.next()?;
        let changes = check.read::<i64, _>(0)?;

        if changes > 0 {
            info!("Deleted reminder {reminder_id} for user {user_id}");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    // Custom Command Methods
    pub async fn add_custom_command(
        &self,
        command_name: &str,
        response_text: &str,
        created_by_user_id: &str,
        guild_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let is_global = guild_id.is_none();
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO custom_commands (command_name, response_text, created_by_user_id, guild_id, is_global, updated_at)
             VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)"
        )?;
        statement.bind((1, command_name))?;
        statement.bind((2, response_text))?;
        statement.bind((3, created_by_user_id))?;
        statement.bind((4, guild_id.unwrap_or("")))?;
        statement.bind((5, if is_global { 1i64 } else { 0i64 }))?;
        statement.next()?;
        info!("Added custom command: {command_name}");
        Ok(())
    }

    pub async fn get_custom_command(&self, command_name: &str, guild_id: Option<&str>) -> Result<Option<String>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT response_text FROM custom_commands
             WHERE command_name = ? AND (guild_id = ? OR is_global = 1)
             ORDER BY is_global ASC
             LIMIT 1"
        )?;
        statement.bind((1, command_name))?;
        statement.bind((2, guild_id.unwrap_or("")))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<String, _>(0)?))
        } else {
            Ok(None)
        }
    }

    pub async fn delete_custom_command(&self, command_name: &str, guild_id: Option<&str>) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM custom_commands WHERE command_name = ? AND guild_id = ?"
        )?;
        statement.bind((1, command_name))?;
        statement.bind((2, guild_id.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    // Analytics Methods
    pub async fn increment_daily_stat(&self, stat_type: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

        match stat_type {
            "message" => {
                conn.execute(
                    "INSERT INTO daily_analytics (date, total_messages) VALUES (?, 1)
                     ON CONFLICT(date) DO UPDATE SET total_messages = total_messages + 1"
                )?;
            }
            "command" => {
                conn.execute(
                    "INSERT INTO daily_analytics (date, total_commands) VALUES (?, 1)
                     ON CONFLICT(date) DO UPDATE SET total_commands = total_commands + 1"
                )?;
            }
            "error" => {
                conn.execute(
                    "INSERT INTO daily_analytics (date, total_errors) VALUES (?, 1)
                     ON CONFLICT(date) DO UPDATE SET total_errors = total_errors + 1"
                )?;
            }
            _ => {}
        }

        let mut statement = conn.prepare(
            "INSERT INTO daily_analytics (date, total_messages) VALUES (?, 0)
             ON CONFLICT(date) DO NOTHING"
        )?;
        statement.bind((1, date.as_str()))?;
        statement.next()?;
        Ok(())
    }

    pub async fn add_performance_metric(&self, metric_type: &str, value: f64, unit: Option<&str>, metadata: Option<&str>) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO performance_metrics (metric_type, value, unit, metadata) VALUES (?, ?, ?, ?)"
        )?;
        statement.bind((1, metric_type))?;
        statement.bind((2, value))?;
        statement.bind((3, unit.unwrap_or("")))?;
        statement.bind((4, metadata.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    // System Metrics Methods (for /sysinfo command)

    /// Store a system metric snapshot (uses performance_metrics table)
    pub async fn store_system_metric(&self, metric_type: &str, value: f64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO performance_metrics (metric_type, value, unit, metadata) VALUES (?, ?, 'system', '')"
        )?;
        statement.bind((1, metric_type))?;
        statement.bind((2, value))?;
        statement.next()?;
        Ok(())
    }

    /// Get historical metrics data for a specific metric type
    /// Returns (unix_timestamp, value) pairs ordered by time ascending
    pub async fn get_metrics_history(&self, metric_type: &str, hours: i64) -> Result<Vec<(i64, f64)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT strftime('%s', timestamp) as unix_time, value
             FROM performance_metrics
             WHERE metric_type = ? AND timestamp >= datetime('now', ? || ' hours')
             ORDER BY timestamp ASC"
        )?;
        statement.bind((1, metric_type))?;
        statement.bind((2, format!("-{}", hours).as_str()))?;

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
    pub async fn cleanup_old_metrics(&self, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM performance_metrics WHERE unit = 'system' AND timestamp < datetime('now', ? || ' days')"
        )?;
        statement.bind((1, format!("-{}", days).as_str()))?;
        statement.next()?;
        info!("Cleaned up system metrics older than {} days", days);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log_error(
        &self,
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
            "INSERT INTO error_logs (error_type, error_message, stack_trace, user_id, channel_id, command, metadata)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, error_type))?;
        statement.bind((2, error_message))?;
        statement.bind((3, stack_trace.unwrap_or("")))?;
        statement.bind((4, user_id.unwrap_or("")))?;
        statement.bind((5, channel_id.unwrap_or("")))?;
        statement.bind((6, command.unwrap_or("")))?;
        statement.bind((7, metadata.unwrap_or("")))?;
        statement.next()?;

        // Also increment daily error count
        self.increment_daily_stat("error").await?;
        Ok(())
    }

    // Feature Flag Methods
    pub async fn set_feature_flag(
        &self,
        feature_name: &str,
        enabled: bool,
        user_id: Option<&str>,
        guild_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO feature_flags (feature_name, enabled, user_id, guild_id, updated_at)
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)"
        )?;
        statement.bind((1, feature_name))?;
        statement.bind((2, if enabled { 1i64 } else { 0i64 }))?;
        statement.bind((3, user_id.unwrap_or("")))?;
        statement.bind((4, guild_id.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    /// Check if a feature is enabled for a guild
    /// Returns true by default if no record exists (features are enabled unless explicitly disabled)
    pub async fn is_feature_enabled(&self, feature_name: &str, user_id: Option<&str>, guild_id: Option<&str>) -> Result<bool> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT enabled FROM feature_flags
             WHERE feature_name = ? AND user_id = ? AND guild_id = ?
             LIMIT 1"
        )?;
        statement.bind((1, feature_name))?;
        statement.bind((2, user_id.unwrap_or("")))?;
        statement.bind((3, guild_id.unwrap_or("")))?;

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
    pub async fn get_guild_feature_flags(&self, guild_id: &str) -> Result<std::collections::HashMap<String, bool>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT feature_name, enabled FROM feature_flags
             WHERE guild_id = ? AND user_id = ''"
        )?;
        statement.bind((1, guild_id))?;

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
        feature_name: &str,
        version: &str,
        guild_id: Option<&str>,
        toggled_by: &str,
        enabled: bool,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO feature_versions (feature_name, version, guild_id, toggled_by, enabled)
             VALUES (?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, feature_name))?;
        statement.bind((2, version))?;
        statement.bind((3, guild_id.unwrap_or("")))?;
        statement.bind((4, toggled_by))?;
        statement.bind((5, if enabled { 1i64 } else { 0i64 }))?;
        statement.next()?;
        info!("Recorded feature toggle: {feature_name} -> {enabled} by {toggled_by}");
        Ok(())
    }

    // Guild Settings Methods
    pub async fn set_guild_setting(&self, guild_id: &str, setting_key: &str, setting_value: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO guild_settings (guild_id, setting_key, setting_value, updated_at)
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)"
        )?;
        statement.bind((1, guild_id))?;
        statement.bind((2, setting_key))?;
        statement.bind((3, setting_value))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_guild_setting(&self, guild_id: &str, setting_key: &str) -> Result<Option<String>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT setting_value FROM guild_settings WHERE guild_id = ? AND setting_key = ?"
        )?;
        statement.bind((1, guild_id))?;
        statement.bind((2, setting_key))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<String, _>(0)?))
        } else {
            Ok(None)
        }
    }

    // Bot Settings Methods (global, not per-guild)
    pub async fn set_bot_setting(&self, setting_key: &str, setting_value: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO bot_settings (setting_key, setting_value, updated_at)
             VALUES (?, ?, CURRENT_TIMESTAMP)"
        )?;
        statement.bind((1, setting_key))?;
        statement.bind((2, setting_value))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_bot_setting(&self, setting_key: &str) -> Result<Option<String>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT setting_value FROM bot_settings WHERE setting_key = ?"
        )?;
        statement.bind((1, setting_key))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<String, _>(0)?))
        } else {
            Ok(None)
        }
    }

    // Extended User Preferences Methods
    pub async fn set_user_preference(&self, user_id: &str, preference_key: &str, preference_value: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT OR REPLACE INTO extended_user_preferences (user_id, preference_key, preference_value, updated_at)
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, preference_key))?;
        statement.bind((3, preference_value))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_user_preference(&self, user_id: &str, preference_key: &str) -> Result<Option<String>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT preference_value FROM extended_user_preferences WHERE user_id = ? AND preference_key = ?"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, preference_key))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<String, _>(0)?))
        } else {
            Ok(None)
        }
    }

    // Conflict Detection & Mediation Methods

    pub async fn record_conflict_detection(
        &self,
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
             (channel_id, guild_id, participants, detection_type, confidence_score, last_message_id)
             VALUES (?, ?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, channel_id))?;
        statement.bind((2, guild_id.unwrap_or("")))?;
        statement.bind((3, participants))?;
        statement.bind((4, detection_type))?;
        statement.bind((5, confidence as f64))?;
        statement.bind((6, last_message_id))?;
        statement.next()?;

        // Get the ID of the inserted row
        let mut id_statement = conn.prepare("SELECT last_insert_rowid()")?;
        id_statement.next()?;
        let conflict_id = id_statement.read::<i64, _>(0)?;

        info!("Recorded conflict detection in channel {channel_id} with confidence {confidence}");
        Ok(conflict_id)
    }

    pub async fn mark_conflict_resolved(&self, conflict_id: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE conflict_detection SET resolved_at = CURRENT_TIMESTAMP WHERE id = ?"
        )?;
        statement.bind((1, conflict_id))?;
        statement.next()?;
        info!("Marked conflict {conflict_id} as resolved");
        Ok(())
    }

    pub async fn mark_mediation_triggered(&self, conflict_id: i64, message_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE conflict_detection
             SET mediation_triggered = 1, mediation_message_id = ?
             WHERE id = ?"
        )?;
        statement.bind((1, message_id))?;
        statement.bind((2, conflict_id))?;
        statement.next()?;
        Ok(())
    }

    pub async fn get_channel_active_conflict(&self, channel_id: &str) -> Result<Option<i64>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT id FROM conflict_detection
             WHERE channel_id = ? AND resolved_at IS NULL
             ORDER BY last_detected DESC LIMIT 1"
        )?;
        statement.bind((1, channel_id))?;

        if let Ok(State::Row) = statement.next() {
            Ok(Some(statement.read::<i64, _>(0)?))
        } else {
            Ok(None)
        }
    }

    pub async fn record_mediation(
        &self,
        conflict_id: i64,
        channel_id: &str,
        message_text: &str,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO mediation_history (conflict_id, channel_id, mediation_message)
             VALUES (?, ?, ?)"
        )?;
        statement.bind((1, conflict_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, message_text))?;
        statement.next()?;
        info!("Recorded mediation for conflict {conflict_id}");
        Ok(())
    }

    /// Get the timestamp of the last mediation in a channel
    pub async fn get_last_mediation_timestamp(&self, channel_id: &str) -> Result<Option<i64>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT strftime('%s', mh.created_at) as unix_time
             FROM mediation_history mh
             WHERE mh.channel_id = ?
             ORDER BY mh.created_at DESC
             LIMIT 1"
        )?;
        statement.bind((1, channel_id))?;

        if let Ok(State::Row) = statement.next() {
            let timestamp_str = statement.read::<String, _>(0)?;
            Ok(Some(timestamp_str.parse::<i64>()?))
        } else {
            Ok(None)
        }
    }

    pub async fn get_recent_channel_messages(
        &self,
        channel_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT user_id, content, strftime('%s', timestamp) as unix_time
             FROM conversation_history
             WHERE channel_id = ?
             ORDER BY timestamp DESC
             LIMIT ?"
        )?;
        statement.bind((1, channel_id))?;
        statement.bind((2, limit as i64))?;

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
        channel_id: &str,
        since_timestamp: i64,
        limit: usize,
    ) -> Result<Vec<(String, String, String)>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT user_id, content, strftime('%s', timestamp) as unix_time
             FROM conversation_history
             WHERE channel_id = ?
               AND CAST(strftime('%s', timestamp) AS INTEGER) > ?
             ORDER BY timestamp DESC
             LIMIT ?"
        )?;
        statement.bind((1, channel_id))?;
        statement.bind((2, since_timestamp))?;
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

    pub async fn update_user_interaction_pattern(
        &self,
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
             (user_id_a, user_id_b, channel_id, interaction_count, conflict_incidents, last_interaction)
             VALUES (?, ?, ?, 1, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(user_id_a, user_id_b, channel_id) DO UPDATE SET
             interaction_count = interaction_count + 1,
             conflict_incidents = conflict_incidents + ?,
             last_interaction = CURRENT_TIMESTAMP"
        )?;
        statement.bind((1, user_a))?;
        statement.bind((2, user_b))?;
        statement.bind((3, channel_id))?;
        statement.bind((4, conflict_increment))?;
        statement.bind((5, conflict_increment))?;
        statement.next()?;
        Ok(())
    }

    // Channel Settings Methods

    /// Get verbosity for a channel, falling back to guild default, then "concise"
    pub async fn get_channel_verbosity(&self, guild_id: &str, channel_id: &str) -> Result<String> {
        let conn = self.connection.lock().await;

        // First try channel-specific setting
        let mut statement = conn.prepare(
            "SELECT verbosity FROM channel_settings WHERE guild_id = ? AND channel_id = ?"
        )?;
        statement.bind((1, guild_id))?;
        statement.bind((2, channel_id))?;

        if let Ok(State::Row) = statement.next() {
            return Ok(statement.read::<String, _>(0)?);
        }

        // Fall back to guild default
        drop(statement);
        let mut guild_stmt = conn.prepare(
            "SELECT setting_value FROM guild_settings WHERE guild_id = ? AND setting_key = 'default_verbosity'"
        )?;
        guild_stmt.bind((1, guild_id))?;

        if let Ok(State::Row) = guild_stmt.next() {
            return Ok(guild_stmt.read::<String, _>(0)?);
        }

        // Default to concise
        Ok("concise".to_string())
    }

    /// Set verbosity for a specific channel
    pub async fn set_channel_verbosity(&self, guild_id: &str, channel_id: &str, verbosity: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO channel_settings (guild_id, channel_id, verbosity, updated_at)
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(guild_id, channel_id) DO UPDATE SET
             verbosity = excluded.verbosity,
             updated_at = CURRENT_TIMESTAMP"
        )?;
        statement.bind((1, guild_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, verbosity))?;
        statement.next()?;
        info!("Set verbosity for channel {channel_id} to {verbosity}");
        Ok(())
    }

    /// Get all settings for a channel
    pub async fn get_channel_settings(&self, guild_id: &str, channel_id: &str) -> Result<(String, bool)> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT verbosity, conflict_enabled FROM channel_settings WHERE guild_id = ? AND channel_id = ?"
        )?;
        statement.bind((1, guild_id))?;
        statement.bind((2, channel_id))?;

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
    pub async fn set_channel_conflict_enabled(&self, guild_id: &str, channel_id: &str, enabled: bool) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO channel_settings (guild_id, channel_id, conflict_enabled, updated_at)
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(guild_id, channel_id) DO UPDATE SET
             conflict_enabled = excluded.conflict_enabled,
             updated_at = CURRENT_TIMESTAMP"
        )?;
        statement.bind((1, guild_id))?;
        statement.bind((2, channel_id))?;
        statement.bind((3, if enabled { 1i64 } else { 0i64 }))?;
        statement.next()?;
        info!("Set conflict_enabled for channel {channel_id} to {enabled}");
        Ok(())
    }

    /// Check if a user has the bot admin role for a guild
    pub async fn has_bot_admin_role(&self, guild_id: &str, user_roles: &[String]) -> Result<bool> {
        // Get the bot admin role ID from guild settings
        let admin_role = self.get_guild_setting(guild_id, "bot_admin_role").await?;

        if let Some(role_id) = admin_role {
            Ok(user_roles.iter().any(|r| r == &role_id))
        } else {
            // No bot admin role set - only Discord admins can manage
            Ok(false)
        }
    }

    // OpenAI Usage Tracking Methods

    /// Log a ChatCompletion (GPT) usage event
    #[allow(clippy::too_many_arguments)]
    pub async fn log_openai_chat_usage(
        &self,
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
             (request_id, user_id, guild_id, channel_id, service_type, model,
              input_tokens, output_tokens, total_tokens, estimated_cost_usd)
             VALUES (?, ?, ?, ?, 'chat', ?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, request_id.unwrap_or("")))?;
        statement.bind((2, user_id))?;
        statement.bind((3, guild_id.unwrap_or("")))?;
        statement.bind((4, channel_id.unwrap_or("")))?;
        statement.bind((5, model))?;
        statement.bind((6, input_tokens as i64))?;
        statement.bind((7, output_tokens as i64))?;
        statement.bind((8, total_tokens as i64))?;
        statement.bind((9, estimated_cost))?;
        statement.next()?;

        // Update daily aggregate
        drop(statement);
        let mut agg_stmt = conn.prepare(
            "INSERT INTO openai_usage_daily
             (date, guild_id, user_id, service_type, request_count, total_tokens, total_cost_usd)
             VALUES (?, ?, ?, 'chat', 1, ?, ?)
             ON CONFLICT(date, guild_id, user_id, service_type) DO UPDATE SET
             request_count = request_count + 1,
             total_tokens = total_tokens + excluded.total_tokens,
             total_cost_usd = total_cost_usd + excluded.total_cost_usd"
        )?;
        agg_stmt.bind((1, date.as_str()))?;
        agg_stmt.bind((2, guild_id.unwrap_or("")))?;
        agg_stmt.bind((3, user_id))?;
        agg_stmt.bind((4, total_tokens as i64))?;
        agg_stmt.bind((5, estimated_cost))?;
        agg_stmt.next()?;

        Ok(())
    }

    /// Log a Whisper (audio transcription) usage event
    pub async fn log_openai_whisper_usage(
        &self,
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
             (user_id, guild_id, channel_id, service_type, model,
              audio_duration_seconds, estimated_cost_usd)
             VALUES (?, ?, ?, 'whisper', 'whisper-1', ?, ?)"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, guild_id.unwrap_or("")))?;
        statement.bind((3, channel_id.unwrap_or("")))?;
        statement.bind((4, audio_duration_seconds))?;
        statement.bind((5, estimated_cost))?;
        statement.next()?;

        // Update daily aggregate
        drop(statement);
        let mut agg_stmt = conn.prepare(
            "INSERT INTO openai_usage_daily
             (date, guild_id, user_id, service_type, request_count, total_audio_seconds, total_cost_usd)
             VALUES (?, ?, ?, 'whisper', 1, ?, ?)
             ON CONFLICT(date, guild_id, user_id, service_type) DO UPDATE SET
             request_count = request_count + 1,
             total_audio_seconds = total_audio_seconds + excluded.total_audio_seconds,
             total_cost_usd = total_cost_usd + excluded.total_cost_usd"
        )?;
        agg_stmt.bind((1, date.as_str()))?;
        agg_stmt.bind((2, guild_id.unwrap_or("")))?;
        agg_stmt.bind((3, user_id))?;
        agg_stmt.bind((4, audio_duration_seconds))?;
        agg_stmt.bind((5, estimated_cost))?;
        agg_stmt.next()?;

        Ok(())
    }

    /// Log a DALL-E (image generation) usage event
    pub async fn log_openai_dalle_usage(
        &self,
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
             (user_id, guild_id, channel_id, service_type, model,
              image_count, image_size, estimated_cost_usd)
             VALUES (?, ?, ?, 'dalle', 'dall-e-3', ?, ?, ?)"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, guild_id.unwrap_or("")))?;
        statement.bind((3, channel_id.unwrap_or("")))?;
        statement.bind((4, image_count as i64))?;
        statement.bind((5, image_size))?;
        statement.bind((6, estimated_cost))?;
        statement.next()?;

        // Update daily aggregate
        drop(statement);
        let mut agg_stmt = conn.prepare(
            "INSERT INTO openai_usage_daily
             (date, guild_id, user_id, service_type, request_count, total_images, total_cost_usd)
             VALUES (?, ?, ?, 'dalle', 1, ?, ?)
             ON CONFLICT(date, guild_id, user_id, service_type) DO UPDATE SET
             request_count = request_count + 1,
             total_images = total_images + excluded.total_images,
             total_cost_usd = total_cost_usd + excluded.total_cost_usd"
        )?;
        agg_stmt.bind((1, date.as_str()))?;
        agg_stmt.bind((2, guild_id.unwrap_or("")))?;
        agg_stmt.bind((3, user_id))?;
        agg_stmt.bind((4, image_count as i64))?;
        agg_stmt.bind((5, estimated_cost))?;
        agg_stmt.next()?;

        Ok(())
    }

    /// Get usage statistics for a user within a date range
    /// Returns (service_type, request_count, tokens, audio_seconds, images, cost)
    pub async fn get_user_usage_stats(
        &self,
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
             WHERE user_id = ? AND date >= date('now', ? || ' days')
             GROUP BY service_type"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, format!("-{}", days).as_str()))?;

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
    /// Includes DM usage from users who have interacted in this guild
    /// Returns (service_type, request_count, tokens, audio_seconds, images, cost)
    pub async fn get_guild_usage_stats(
        &self,
        guild_id: &str,
        days: i64,
    ) -> Result<Vec<(String, i64, i64, f64, i64, f64)>> {
        let conn = self.connection.lock().await;
        let days_str = format!("-{}", days);
        let mut statement = conn.prepare(
            "SELECT service_type,
                    SUM(request_count) as requests,
                    SUM(total_tokens) as tokens,
                    SUM(total_audio_seconds) as audio_secs,
                    SUM(total_images) as images,
                    SUM(total_cost_usd) as cost
             FROM openai_usage_daily
             WHERE (guild_id = ? OR (guild_id = '' AND user_id IN (
                 SELECT DISTINCT user_id FROM openai_usage_daily WHERE guild_id = ?
             )))
             AND date >= date('now', ? || ' days')
             GROUP BY service_type"
        )?;
        statement.bind((1, guild_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, days_str.as_str()))?;

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
    /// Includes DM usage from users who have interacted in this guild
    /// Returns (user_id, request_count, total_cost)
    pub async fn get_guild_top_users_by_cost(
        &self,
        guild_id: &str,
        days: i64,
        limit: i64,
    ) -> Result<Vec<(String, i64, f64)>> {
        let conn = self.connection.lock().await;
        let days_str = format!("-{}", days);
        let mut statement = conn.prepare(
            "SELECT user_id,
                    SUM(request_count) as requests,
                    SUM(total_cost_usd) as cost
             FROM openai_usage_daily
             WHERE (guild_id = ? OR (guild_id = '' AND user_id IN (
                 SELECT DISTINCT user_id FROM openai_usage_daily WHERE guild_id = ?
             )))
             AND user_id != ''
             AND date >= date('now', ? || ' days')
             GROUP BY user_id
             ORDER BY cost DESC
             LIMIT ?"
        )?;
        statement.bind((1, guild_id))?;
        statement.bind((2, guild_id))?;
        statement.bind((3, days_str.as_str()))?;
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
    pub async fn cleanup_old_openai_usage(&self, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM openai_usage WHERE timestamp < datetime('now', ? || ' days')"
        )?;
        statement.bind((1, format!("-{}", days).as_str()))?;
        statement.next()?;
        info!("Cleaned up openai_usage older than {} days", days);
        Ok(())
    }

    /// Cleanup old daily aggregates (keep last N days)
    pub async fn cleanup_old_openai_usage_daily(&self, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM openai_usage_daily WHERE date < date('now', ? || ' days')"
        )?;
        statement.bind((1, format!("-{}", days).as_str()))?;
        statement.next()?;
        info!("Cleaned up openai_usage_daily older than {} days", days);
        Ok(())
    }

    // DM Interaction Tracking Methods

    /// Create a new DM session
    pub async fn create_dm_session(&self, session_id: &str, user_id: &str, channel_id: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO dm_sessions (session_id, user_id, channel_id) VALUES (?, ?, ?)"
        )?;
        statement.bind((1, session_id))?;
        statement.bind((2, user_id))?;
        statement.bind((3, channel_id))?;
        statement.next()?;

        // Also create metrics row
        let mut metrics_stmt = conn.prepare(
            "INSERT INTO dm_session_metrics (session_id) VALUES (?)"
        )?;
        metrics_stmt.bind((1, session_id))?;
        metrics_stmt.next()?;

        Ok(())
    }

    /// End a DM session
    pub async fn end_dm_session(&self, session_id: &str, reason: &str) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE dm_sessions SET ended_at = CURRENT_TIMESTAMP, end_reason = ? WHERE session_id = ?"
        )?;
        statement.bind((1, reason))?;
        statement.bind((2, session_id))?;
        statement.next()?;
        Ok(())
    }

    /// Update DM session activity
    pub async fn update_dm_session_activity(
        &self,
        session_id: &str,
        msg_count: i32,
        user_chars: i32,
        bot_chars: i32,
        avg_response_time: i32,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "UPDATE dm_sessions
             SET message_count = ?,
                 total_user_chars = ?,
                 total_bot_chars = ?,
                 avg_response_time_ms = ?,
                 last_activity_at = CURRENT_TIMESTAMP
             WHERE session_id = ?"
        )?;
        statement.bind((1, msg_count as i64))?;
        statement.bind((2, user_chars as i64))?;
        statement.bind((3, bot_chars as i64))?;
        statement.bind((4, avg_response_time as i64))?;
        statement.bind((5, session_id))?;
        statement.next()?;
        Ok(())
    }

    /// Log a DM event
    pub async fn log_dm_event(
        &self,
        session_id: &str,
        event_type: &str,
        user_id: &str,
        channel_id: &str,
        event_data: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "INSERT INTO dm_events (session_id, event_type, user_id, channel_id, event_data)
             VALUES (?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, session_id))?;
        statement.bind((2, event_type))?;
        statement.bind((3, user_id))?;
        statement.bind((4, channel_id))?;
        statement.bind((5, event_data.unwrap_or("")))?;
        statement.next()?;
        Ok(())
    }

    /// Update DM session metrics
    pub async fn update_dm_session_metrics(
        &self,
        session_id: &str,
        api_type: &str,
        tokens: u32,
        cost: f64,
    ) -> Result<()> {
        let conn = self.connection.lock().await;

        let (api_field, tokens_update) = match api_type {
            "chat" => ("chat_calls = chat_calls + 1", format!("total_tokens = total_tokens + {}", tokens)),
            "whisper" => ("whisper_calls = whisper_calls + 1", String::new()),
            "dalle" => ("dalle_calls = dalle_calls + 1", String::new()),
            _ => return Ok(()),
        };

        let sql = if tokens_update.is_empty() {
            format!(
                "UPDATE dm_session_metrics
                 SET {},
                     total_api_calls = total_api_calls + 1,
                     total_api_cost_usd = total_api_cost_usd + ?,
                     updated_at = CURRENT_TIMESTAMP
                 WHERE session_id = ?",
                api_field
            )
        } else {
            format!(
                "UPDATE dm_session_metrics
                 SET {},
                     {},
                     total_api_calls = total_api_calls + 1,
                     total_api_cost_usd = total_api_cost_usd + ?,
                     updated_at = CURRENT_TIMESTAMP
                 WHERE session_id = ?",
                api_field, tokens_update
            )
        };

        let mut statement = conn.prepare(&sql)?;
        statement.bind((1, cost))?;
        statement.bind((2, session_id))?;
        statement.next()?;
        Ok(())
    }

    /// Increment DM session feature counter
    pub async fn increment_dm_session_feature(&self, session_id: &str, feature: &str) -> Result<()> {
        let conn = self.connection.lock().await;

        let field = match feature {
            "audio" => "audio_transcriptions",
            "slash_command" => "slash_commands_used",
            _ => return Ok(()),
        };

        let sql = format!(
            "UPDATE dm_session_metrics
             SET {} = {} + 1, updated_at = CURRENT_TIMESTAMP
             WHERE session_id = ?",
            field, field
        );

        let mut statement = conn.prepare(&sql)?;
        statement.bind((1, session_id))?;
        statement.next()?;
        Ok(())
    }

    /// Get user DM stats for the last N days
    pub async fn get_user_dm_stats(&self, user_id: &str, days: i64) -> Result<DmStats> {
        let conn = self.connection.lock().await;

        // Get session counts and averages
        let mut stmt = conn.prepare(
            "SELECT
                COUNT(*) as session_count,
                SUM(message_count) as total_messages,
                SUM(user_message_count) as user_messages,
                SUM(bot_message_count) as bot_messages,
                AVG(avg_response_time_ms) as avg_response_time,
                AVG((julianday(ended_at) - julianday(started_at)) * 24 * 60) as avg_duration_min
             FROM dm_sessions
             WHERE user_id = ?
             AND started_at >= datetime('now', ? || ' days')
             AND ended_at IS NOT NULL"
        )?;
        stmt.bind((1, user_id))?;
        stmt.bind((2, format!("-{}", days).as_str()))?;

        let (session_count, total_messages, user_messages, bot_messages, avg_response_time, avg_duration) =
            if let Ok(State::Row) = stmt.next() {
                (
                    stmt.read::<i64, _>(0).unwrap_or(0),
                    stmt.read::<i64, _>(1).unwrap_or(0),
                    stmt.read::<i64, _>(2).unwrap_or(0),
                    stmt.read::<i64, _>(3).unwrap_or(0),
                    stmt.read::<i64, _>(4).unwrap_or(0),
                    stmt.read::<f64, _>(5).unwrap_or(0.0),
                )
            } else {
                (0, 0, 0, 0, 0, 0.0)
            };

        // Get API metrics
        let mut api_stmt = conn.prepare(
            "SELECT
                SUM(sm.total_api_calls) as api_calls,
                SUM(sm.total_tokens) as tokens,
                SUM(sm.total_api_cost_usd) as cost,
                SUM(sm.chat_calls) as chat_calls,
                SUM(sm.whisper_calls) as whisper_calls,
                SUM(sm.dalle_calls) as dalle_calls,
                SUM(sm.audio_transcriptions) as audio_count,
                SUM(sm.slash_commands_used) as slash_count
             FROM dm_session_metrics sm
             JOIN dm_sessions s ON sm.session_id = s.session_id
             WHERE s.user_id = ?
             AND s.started_at >= datetime('now', ? || ' days')"
        )?;
        api_stmt.bind((1, user_id))?;
        api_stmt.bind((2, format!("-{}", days).as_str()))?;

        let (api_calls, tokens, cost, chat_calls, whisper_calls, dalle_calls, audio_count, slash_count) =
            if let Ok(State::Row) = api_stmt.next() {
                (
                    api_stmt.read::<i64, _>(0).unwrap_or(0),
                    api_stmt.read::<i64, _>(1).unwrap_or(0),
                    api_stmt.read::<f64, _>(2).unwrap_or(0.0),
                    api_stmt.read::<i64, _>(3).unwrap_or(0),
                    api_stmt.read::<i64, _>(4).unwrap_or(0),
                    api_stmt.read::<i64, _>(5).unwrap_or(0),
                    api_stmt.read::<i64, _>(6).unwrap_or(0),
                    api_stmt.read::<i64, _>(7).unwrap_or(0),
                )
            } else {
                (0, 0, 0.0, 0, 0, 0, 0, 0)
            };

        Ok(DmStats {
            session_count,
            total_messages,
            user_messages,
            bot_messages,
            avg_response_time_ms: avg_response_time,
            avg_session_duration_min: avg_duration,
            api_calls,
            total_tokens: tokens,
            total_cost_usd: cost,
            chat_calls,
            whisper_calls,
            dalle_calls,
            audio_transcriptions: audio_count,
            slash_commands_used: slash_count,
        })
    }

    /// Get user's recent DM sessions
    pub async fn get_user_recent_sessions(&self, user_id: &str, limit: i64) -> Result<Vec<SessionInfo>> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "SELECT session_id, started_at, ended_at, message_count, avg_response_time_ms
             FROM dm_sessions
             WHERE user_id = ?
             ORDER BY started_at DESC
             LIMIT ?"
        )?;
        statement.bind((1, user_id))?;
        statement.bind((2, limit))?;

        let mut sessions = Vec::new();
        while let Ok(State::Row) = statement.next() {
            sessions.push(SessionInfo {
                session_id: statement.read::<String, _>(0)?,
                started_at: statement.read::<String, _>(1)?,
                ended_at: statement.read::<Option<String>, _>(2)?,
                message_count: statement.read::<i64, _>(3)?,
                avg_response_time_ms: statement.read::<i64, _>(4).unwrap_or(0),
            });
        }

        Ok(sessions)
    }

    /// Cleanup old DM events (keep last N days)
    pub async fn cleanup_old_dm_events(&self, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM dm_events WHERE timestamp < datetime('now', ? || ' days')"
        )?;
        statement.bind((1, format!("-{}", days).as_str()))?;
        statement.next()?;
        info!("Cleaned up dm_events older than {} days", days);
        Ok(())
    }

    // Plugin Job Methods

    /// Create a new plugin job record
    pub async fn create_plugin_job(&self, job: &crate::features::plugins::job::Job) -> Result<()> {
        let conn = self.connection.lock().await;
        let params_json = serde_json::to_string(&job.params)?;

        let mut statement = conn.prepare(
            "INSERT INTO plugin_jobs (id, plugin_name, user_id, guild_id, channel_id, status, params, started_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, job.id.as_str()))?;
        statement.bind((2, job.plugin_name.as_str()))?;
        statement.bind((3, job.user_id.as_str()))?;
        statement.bind((4, job.guild_id.as_deref().unwrap_or("")))?;
        statement.bind((5, job.channel_id.as_str()))?;
        statement.bind((6, job.status.to_string().as_str()))?;
        statement.bind((7, params_json.as_str()))?;
        statement.bind((8, job.started_at.to_rfc3339().as_str()))?;
        statement.next()?;

        Ok(())
    }

    /// Update a plugin job record
    pub async fn update_plugin_job(&self, job: &crate::features::plugins::job::Job) -> Result<()> {
        let conn = self.connection.lock().await;

        let completed_at = job.completed_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_default();

        let result_preview = job.result.as_ref()
            .map(|r| r.chars().take(1000).collect::<String>())
            .unwrap_or_default();

        let error = job.error.as_ref()
            .cloned()
            .unwrap_or_default();

        let thread_id = job.thread_id.as_deref().unwrap_or("");

        let mut statement = conn.prepare(
            "UPDATE plugin_jobs SET
                status = ?,
                thread_id = ?,
                result = ?,
                error = ?,
                completed_at = CASE WHEN ? = '' THEN NULL ELSE ? END
             WHERE id = ?"
        )?;
        statement.bind((1, job.status.to_string().as_str()))?;
        statement.bind((2, thread_id))?;
        statement.bind((3, result_preview.as_str()))?;
        statement.bind((4, error.as_str()))?;
        statement.bind((5, completed_at.as_str()))?;
        statement.bind((6, completed_at.as_str()))?;
        statement.bind((7, job.id.as_str()))?;
        statement.next()?;

        Ok(())
    }

    /// Get incomplete plugin jobs (for crash recovery)
    pub async fn get_incomplete_plugin_jobs(&self) -> Result<Vec<crate::features::plugins::job::Job>> {
        use crate::features::plugins::job::{Job, JobStatus};
        use std::collections::HashMap;

        let conn = self.connection.lock().await;
        let mut jobs = Vec::new();

        let mut statement = conn.prepare(
            "SELECT id, plugin_name, user_id, guild_id, channel_id, thread_id, status, params, started_at
             FROM plugin_jobs
             WHERE status IN ('pending', 'running')
             ORDER BY started_at ASC"
        )?;

        while let Ok(State::Row) = statement.next() {
            let params_json: String = statement.read::<String, _>(7)?;
            let params: HashMap<String, String> = serde_json::from_str(&params_json).unwrap_or_default();
            let guild_id: String = statement.read(3)?;
            let thread_id: String = statement.read(5)?;
            let status_str: String = statement.read(6)?;
            let started_at_str: String = statement.read(8)?;

            let status = status_str.parse::<JobStatus>().unwrap_or(JobStatus::Pending);
            let started_at = chrono::DateTime::parse_from_rfc3339(&started_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            jobs.push(Job {
                id: statement.read(0)?,
                plugin_name: statement.read(1)?,
                user_id: statement.read(2)?,
                guild_id: if guild_id.is_empty() { None } else { Some(guild_id) },
                channel_id: statement.read(4)?,
                thread_id: if thread_id.is_empty() { None } else { Some(thread_id) },
                status,
                params,
                started_at,
                completed_at: None,
                result: None,
                error: None,
                parent_playlist_id: None, // Recovery doesn't load parent - handled separately
            });
        }

        Ok(jobs)
    }

    /// Cleanup old completed plugin jobs (keep last N days)
    pub async fn cleanup_old_plugin_jobs(&self, days: i64) -> Result<()> {
        let conn = self.connection.lock().await;
        let mut statement = conn.prepare(
            "DELETE FROM plugin_jobs WHERE completed_at < datetime('now', ? || ' days')"
        )?;
        statement.bind((1, format!("-{}", days).as_str()))?;
        statement.next()?;
        info!("Cleaned up plugin_jobs older than {} days", days);
        Ok(())
    }

    // Playlist Job Methods

    /// Create a new playlist job record
    pub async fn create_playlist_job(&self, job: &crate::features::plugins::job::PlaylistJob) -> Result<()> {
        let conn = self.connection.lock().await;

        let mut statement = conn.prepare(
            "INSERT INTO playlist_jobs (
                id, user_id, guild_id, channel_id, playlist_url, playlist_id,
                playlist_title, total_videos, status, max_videos, started_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )?;
        statement.bind((1, job.id.as_str()))?;
        statement.bind((2, job.user_id.as_str()))?;
        statement.bind((3, job.guild_id.as_deref().unwrap_or("")))?;
        statement.bind((4, job.channel_id.as_str()))?;
        statement.bind((5, job.playlist_url.as_str()))?;
        statement.bind((6, job.playlist_id.as_str()))?;
        statement.bind((7, job.playlist_title.as_deref().unwrap_or("")))?;
        statement.bind((8, job.total_videos as i64))?;
        statement.bind((9, job.status.to_string().as_str()))?;
        statement.bind((10, job.max_videos.map(|v| v as i64).unwrap_or(-1)))?;
        statement.bind((11, job.started_at.to_rfc3339().as_str()))?;
        statement.next()?;

        Ok(())
    }

    /// Update a playlist job record
    pub async fn update_playlist_job(&self, job: &crate::features::plugins::job::PlaylistJob) -> Result<()> {
        let conn = self.connection.lock().await;

        let completed_at = job.completed_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_default();

        let cancelled_at = job.cancelled_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_default();

        let mut statement = conn.prepare(
            "UPDATE playlist_jobs SET
                thread_id = ?,
                playlist_title = ?,
                completed_videos = ?,
                failed_videos = ?,
                skipped_videos = ?,
                status = ?,
                current_video_job_id = ?,
                error = ?,
                completed_at = CASE WHEN ? = '' THEN NULL ELSE ? END,
                cancelled_at = CASE WHEN ? = '' THEN NULL ELSE ? END,
                cancelled_by = ?
             WHERE id = ?"
        )?;
        statement.bind((1, job.thread_id.as_deref().unwrap_or("")))?;
        statement.bind((2, job.playlist_title.as_deref().unwrap_or("")))?;
        statement.bind((3, job.completed_videos as i64))?;
        statement.bind((4, job.failed_videos as i64))?;
        statement.bind((5, job.skipped_videos as i64))?;
        statement.bind((6, job.status.to_string().as_str()))?;
        statement.bind((7, job.current_video_job_id.as_deref().unwrap_or("")))?;
        statement.bind((8, job.error.as_deref().unwrap_or("")))?;
        statement.bind((9, completed_at.as_str()))?;
        statement.bind((10, completed_at.as_str()))?;
        statement.bind((11, cancelled_at.as_str()))?;
        statement.bind((12, cancelled_at.as_str()))?;
        statement.bind((13, job.cancelled_by.as_deref().unwrap_or("")))?;
        statement.bind((14, job.id.as_str()))?;
        statement.next()?;

        Ok(())
    }

    /// Get incomplete playlist jobs (for crash recovery / auto-resume)
    pub async fn get_incomplete_playlist_jobs(&self) -> Result<Vec<crate::features::plugins::job::PlaylistJob>> {
        use crate::features::plugins::job::{PlaylistJob, PlaylistJobStatus};

        let conn = self.connection.lock().await;
        let mut jobs = Vec::new();

        let mut statement = conn.prepare(
            "SELECT id, user_id, guild_id, channel_id, thread_id, playlist_url, playlist_id,
                    playlist_title, total_videos, completed_videos, failed_videos, skipped_videos,
                    status, max_videos, current_video_job_id, error, started_at
             FROM playlist_jobs
             WHERE status IN ('pending', 'running', 'paused')
             ORDER BY started_at ASC"
        )?;

        while let Ok(State::Row) = statement.next() {
            let guild_id: String = statement.read(2)?;
            let thread_id: String = statement.read(4)?;
            let playlist_title: String = statement.read(7)?;
            let status_str: String = statement.read(12)?;
            let max_videos: i64 = statement.read(13)?;
            let current_video_job_id: String = statement.read(14)?;
            let error: String = statement.read(15)?;
            let started_at_str: String = statement.read(16)?;

            let status = status_str.parse::<PlaylistJobStatus>().unwrap_or(PlaylistJobStatus::Pending);
            let started_at = chrono::DateTime::parse_from_rfc3339(&started_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            jobs.push(PlaylistJob {
                id: statement.read(0)?,
                user_id: statement.read(1)?,
                guild_id: if guild_id.is_empty() { None } else { Some(guild_id) },
                channel_id: statement.read(3)?,
                thread_id: if thread_id.is_empty() { None } else { Some(thread_id) },
                playlist_url: statement.read(5)?,
                playlist_id: statement.read(6)?,
                playlist_title: if playlist_title.is_empty() { None } else { Some(playlist_title) },
                total_videos: statement.read::<i64, _>(8)? as u32,
                completed_videos: statement.read::<i64, _>(9)? as u32,
                failed_videos: statement.read::<i64, _>(10)? as u32,
                skipped_videos: statement.read::<i64, _>(11)? as u32,
                status,
                max_videos: if max_videos < 0 { None } else { Some(max_videos as u32) },
                current_video_job_id: if current_video_job_id.is_empty() { None } else { Some(current_video_job_id) },
                error: if error.is_empty() { None } else { Some(error) },
                started_at,
                completed_at: None,
                cancelled_at: None,
                cancelled_by: None,
            });
        }

        Ok(jobs)
    }

    /// Get active playlist jobs for a user
    pub async fn get_user_active_playlist_jobs(&self, user_id: &str) -> Result<Vec<crate::features::plugins::job::PlaylistJob>> {
        use crate::features::plugins::job::{PlaylistJob, PlaylistJobStatus};

        let conn = self.connection.lock().await;
        let mut jobs = Vec::new();

        let mut statement = conn.prepare(
            "SELECT id, user_id, guild_id, channel_id, thread_id, playlist_url, playlist_id,
                    playlist_title, total_videos, completed_videos, failed_videos, skipped_videos,
                    status, max_videos, current_video_job_id, error, started_at
             FROM playlist_jobs
             WHERE user_id = ? AND status IN ('pending', 'running', 'paused')
             ORDER BY started_at DESC"
        )?;
        statement.bind((1, user_id))?;

        while let Ok(State::Row) = statement.next() {
            let guild_id: String = statement.read(2)?;
            let thread_id: String = statement.read(4)?;
            let playlist_title: String = statement.read(7)?;
            let status_str: String = statement.read(12)?;
            let max_videos: i64 = statement.read(13)?;
            let current_video_job_id: String = statement.read(14)?;
            let error: String = statement.read(15)?;
            let started_at_str: String = statement.read(16)?;

            let status = status_str.parse::<PlaylistJobStatus>().unwrap_or(PlaylistJobStatus::Pending);
            let started_at = chrono::DateTime::parse_from_rfc3339(&started_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            jobs.push(PlaylistJob {
                id: statement.read(0)?,
                user_id: statement.read(1)?,
                guild_id: if guild_id.is_empty() { None } else { Some(guild_id) },
                channel_id: statement.read(3)?,
                thread_id: if thread_id.is_empty() { None } else { Some(thread_id) },
                playlist_url: statement.read(5)?,
                playlist_id: statement.read(6)?,
                playlist_title: if playlist_title.is_empty() { None } else { Some(playlist_title) },
                total_videos: statement.read::<i64, _>(8)? as u32,
                completed_videos: statement.read::<i64, _>(9)? as u32,
                failed_videos: statement.read::<i64, _>(10)? as u32,
                skipped_videos: statement.read::<i64, _>(11)? as u32,
                status,
                max_videos: if max_videos < 0 { None } else { Some(max_videos as u32) },
                current_video_job_id: if current_video_job_id.is_empty() { None } else { Some(current_video_job_id) },
                error: if error.is_empty() { None } else { Some(error) },
                started_at,
                completed_at: None,
                cancelled_at: None,
                cancelled_by: None,
            });
        }

        Ok(jobs)
    }

    /// Get completed video job IDs for a playlist (for resume functionality)
    pub async fn get_completed_video_job_ids(&self, playlist_job_id: &str) -> Result<Vec<String>> {
        let conn = self.connection.lock().await;
        let mut ids = Vec::new();

        let mut statement = conn.prepare(
            "SELECT params FROM plugin_jobs
             WHERE parent_playlist_id = ? AND status = 'completed'"
        )?;
        statement.bind((1, playlist_job_id))?;

        while let Ok(State::Row) = statement.next() {
            let params_json: String = statement.read(0)?;
            if let Ok(params) = serde_json::from_str::<std::collections::HashMap<String, String>>(&params_json) {
                if let Some(url) = params.get("url") {
                    // Extract video ID from URL
                    if let Some(vid) = url.split("v=").nth(1).and_then(|s| s.split('&').next()) {
                        ids.push(vid.to_string());
                    }
                }
            }
        }

        Ok(ids)
    }
}

/// DM statistics for a user
#[derive(Debug, Clone)]
pub struct DmStats {
    pub session_count: i64,
    pub total_messages: i64,
    pub user_messages: i64,
    pub bot_messages: i64,
    pub avg_response_time_ms: i64,
    pub avg_session_duration_min: f64,
    pub api_calls: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub chat_calls: i64,
    pub whisper_calls: i64,
    pub dalle_calls: i64,
    pub audio_transcriptions: i64,
    pub slash_commands_used: i64,
}

/// Session information
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub message_count: i64,
    pub avg_response_time_ms: i64,
}