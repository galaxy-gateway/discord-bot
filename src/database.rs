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
}