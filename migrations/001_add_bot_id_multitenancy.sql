-- Migration: 001_add_bot_id_multitenancy.sql
-- Purpose: Add bot_id column to all tables for multi-bot support
--
-- IMPORTANT: Run a backup before executing this migration:
--   cp persona.db persona.db.backup-$(date +%Y%m%d-%H%M%S)
--
-- Execute with: sqlite3 persona.db < migrations/001_add_bot_id_multitenancy.sql
--
-- This migration uses a tiered approach:
-- - Tier 1: Tables requiring full recreation (PK/UNIQUE constraint changes)
-- - Tier 2: Tables with foreign key dependencies (order matters)
-- - Tier 3: Simple ALTER TABLE additions

PRAGMA foreign_keys = OFF;
BEGIN TRANSACTION;

-- ============================================================================
-- TIER 1: Full Table Recreation (PK or UNIQUE constraint changes)
-- ============================================================================

-- 1.1 user_preferences - PK changes from (user_id) to (bot_id, user_id)
CREATE TABLE user_preferences_new (
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    default_persona TEXT DEFAULT 'obi',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (bot_id, user_id)
);

INSERT INTO user_preferences_new (bot_id, user_id, default_persona, created_at, updated_at)
SELECT 'default', user_id, default_persona, created_at, updated_at FROM user_preferences;

DROP TABLE user_preferences;
ALTER TABLE user_preferences_new RENAME TO user_preferences;

-- 1.2 guild_settings - UNIQUE changes to include bot_id
CREATE TABLE guild_settings_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    guild_id TEXT NOT NULL,
    setting_key TEXT NOT NULL,
    setting_value TEXT,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(bot_id, guild_id, setting_key)
);

INSERT INTO guild_settings_new (id, bot_id, guild_id, setting_key, setting_value, updated_at)
SELECT id, 'default', guild_id, setting_key, setting_value, updated_at FROM guild_settings;

DROP TABLE guild_settings;
ALTER TABLE guild_settings_new RENAME TO guild_settings;

CREATE INDEX idx_guild_setting ON guild_settings(bot_id, guild_id, setting_key);

-- 1.3 channel_settings - UNIQUE changes to include bot_id
CREATE TABLE channel_settings_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    guild_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    verbosity TEXT DEFAULT 'concise',
    conflict_enabled BOOLEAN DEFAULT 1,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(bot_id, guild_id, channel_id)
);

INSERT INTO channel_settings_new (id, bot_id, guild_id, channel_id, verbosity, conflict_enabled, updated_at)
SELECT id, 'default', guild_id, channel_id, verbosity, conflict_enabled, updated_at FROM channel_settings;

DROP TABLE channel_settings;
ALTER TABLE channel_settings_new RENAME TO channel_settings;

CREATE INDEX idx_channel_settings_guild ON channel_settings(bot_id, guild_id);
CREATE INDEX idx_channel_settings_channel ON channel_settings(bot_id, channel_id);

-- 1.4 custom_commands - UNIQUE changes to include bot_id
CREATE TABLE custom_commands_new (
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
);

INSERT INTO custom_commands_new (id, bot_id, command_name, response_text, created_by_user_id, guild_id, is_global, created_at, updated_at)
SELECT id, 'default', command_name, response_text, created_by_user_id, guild_id, is_global, created_at, updated_at FROM custom_commands;

DROP TABLE custom_commands;
ALTER TABLE custom_commands_new RENAME TO custom_commands;

CREATE INDEX idx_custom_command ON custom_commands(bot_id, command_name, guild_id);

-- 1.5 feature_flags - UNIQUE changes to include bot_id
CREATE TABLE feature_flags_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    feature_name TEXT NOT NULL,
    enabled BOOLEAN DEFAULT 0,
    user_id TEXT,
    guild_id TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(bot_id, feature_name, user_id, guild_id)
);

INSERT INTO feature_flags_new (id, bot_id, feature_name, enabled, user_id, guild_id, created_at, updated_at)
SELECT id, 'default', feature_name, enabled, user_id, guild_id, created_at, updated_at FROM feature_flags;

DROP TABLE feature_flags;
ALTER TABLE feature_flags_new RENAME TO feature_flags;

CREATE INDEX idx_feature_flag ON feature_flags(bot_id, feature_name, user_id, guild_id);

-- 1.6 extended_user_preferences - UNIQUE changes to include bot_id
CREATE TABLE extended_user_preferences_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    preference_key TEXT NOT NULL,
    preference_value TEXT,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(bot_id, user_id, preference_key)
);

INSERT INTO extended_user_preferences_new (id, bot_id, user_id, preference_key, preference_value, updated_at)
SELECT id, 'default', user_id, preference_key, preference_value, updated_at FROM extended_user_preferences;

DROP TABLE extended_user_preferences;
ALTER TABLE extended_user_preferences_new RENAME TO extended_user_preferences;

CREATE INDEX idx_user_pref ON extended_user_preferences(bot_id, user_id, preference_key);

-- 1.7 user_interaction_patterns - UNIQUE changes to include bot_id
CREATE TABLE user_interaction_patterns_new (
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
);

INSERT INTO user_interaction_patterns_new (id, bot_id, user_id_a, user_id_b, channel_id, guild_id, interaction_count, last_interaction, conflict_incidents, avg_response_time_ms, created_at)
SELECT id, 'default', user_id_a, user_id_b, channel_id, guild_id, interaction_count, last_interaction, conflict_incidents, avg_response_time_ms, created_at FROM user_interaction_patterns;

DROP TABLE user_interaction_patterns;
ALTER TABLE user_interaction_patterns_new RENAME TO user_interaction_patterns;

CREATE INDEX idx_interaction_users ON user_interaction_patterns(bot_id, user_id_a, user_id_b);

-- 1.8 daily_analytics - UNIQUE changes to include bot_id
CREATE TABLE daily_analytics_new (
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
);

INSERT INTO daily_analytics_new (id, bot_id, date, total_messages, unique_users, total_commands, total_errors, persona_usage, created_at)
SELECT id, 'default', date, total_messages, unique_users, total_commands, total_errors, persona_usage, created_at FROM daily_analytics;

DROP TABLE daily_analytics;
ALTER TABLE daily_analytics_new RENAME TO daily_analytics;

CREATE INDEX idx_analytics_date ON daily_analytics(bot_id, date);

-- 1.9 openai_usage_daily - UNIQUE changes to include bot_id
CREATE TABLE openai_usage_daily_new (
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
);

INSERT INTO openai_usage_daily_new (id, bot_id, date, guild_id, user_id, service_type, request_count, total_tokens, total_audio_seconds, total_images, total_cost_usd)
SELECT id, 'default', date, guild_id, user_id, service_type, request_count, total_tokens, total_audio_seconds, total_images, total_cost_usd FROM openai_usage_daily;

DROP TABLE openai_usage_daily;
ALTER TABLE openai_usage_daily_new RENAME TO openai_usage_daily;

CREATE INDEX idx_openai_daily_guild_date ON openai_usage_daily(bot_id, guild_id, date);
CREATE INDEX idx_openai_daily_user_date ON openai_usage_daily(bot_id, user_id, date);

-- ============================================================================
-- TIER 2: FK-Dependent Tables (order matters - parent first)
-- ============================================================================

-- 2.1 conflict_detection - Parent table, migrate first
CREATE TABLE conflict_detection_new (
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
);

INSERT INTO conflict_detection_new (id, bot_id, channel_id, guild_id, participants, detection_type, confidence_score, last_message_id, mediation_triggered, mediation_message_id, first_detected, last_detected, resolved_at)
SELECT id, 'default', channel_id, guild_id, participants, detection_type, confidence_score, last_message_id, mediation_triggered, mediation_message_id, first_detected, last_detected, resolved_at FROM conflict_detection;

DROP TABLE conflict_detection;
ALTER TABLE conflict_detection_new RENAME TO conflict_detection;

CREATE INDEX idx_conflict_channel ON conflict_detection(bot_id, channel_id, guild_id);
CREATE INDEX idx_conflict_timestamp ON conflict_detection(bot_id, first_detected);

-- 2.2 mediation_history - Has FK to conflict_detection
CREATE TABLE mediation_history_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    conflict_id INTEGER NOT NULL,
    channel_id TEXT NOT NULL,
    mediation_message TEXT,
    effectiveness_rating INTEGER,
    follow_up_messages INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(conflict_id) REFERENCES conflict_detection(id)
);

INSERT INTO mediation_history_new (id, bot_id, conflict_id, channel_id, mediation_message, effectiveness_rating, follow_up_messages, created_at)
SELECT id, 'default', conflict_id, channel_id, mediation_message, effectiveness_rating, follow_up_messages, created_at FROM mediation_history;

DROP TABLE mediation_history;
ALTER TABLE mediation_history_new RENAME TO mediation_history;

CREATE INDEX idx_mediation_conflict ON mediation_history(bot_id, conflict_id);

-- ============================================================================
-- TIER 3: Simple ALTER TABLE additions
-- ============================================================================

-- 3.1 conversation_history
ALTER TABLE conversation_history ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';

-- Drop old indexes and create new ones with bot_id
DROP INDEX IF EXISTS idx_user_channel;
DROP INDEX IF EXISTS idx_timestamp;
CREATE INDEX idx_conversation ON conversation_history(bot_id, user_id, channel_id, timestamp);
CREATE INDEX idx_conversation_channel ON conversation_history(bot_id, channel_id, timestamp);

-- 3.2 usage_stats
ALTER TABLE usage_stats ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
CREATE INDEX idx_usage ON usage_stats(bot_id, timestamp);

-- 3.3 message_metadata
ALTER TABLE message_metadata ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
DROP INDEX IF EXISTS idx_message_id;
CREATE INDEX idx_message_metadata ON message_metadata(bot_id, message_id);

-- 3.4 interaction_sessions
ALTER TABLE interaction_sessions ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
DROP INDEX IF EXISTS idx_session_user;
CREATE INDEX idx_session_user ON interaction_sessions(bot_id, user_id, session_start);

-- 3.5 user_bookmarks
ALTER TABLE user_bookmarks ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
DROP INDEX IF EXISTS idx_bookmark_user;
CREATE INDEX idx_bookmark_user ON user_bookmarks(bot_id, user_id);

-- 3.6 reminders
ALTER TABLE reminders ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
DROP INDEX IF EXISTS idx_reminder_time;
CREATE INDEX idx_reminder_time ON reminders(bot_id, remind_at, completed);

-- 3.7 performance_metrics
ALTER TABLE performance_metrics ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
DROP INDEX IF EXISTS idx_metrics_type;
CREATE INDEX idx_metrics_type ON performance_metrics(bot_id, metric_type, timestamp);

-- 3.8 error_logs
ALTER TABLE error_logs ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
DROP INDEX IF EXISTS idx_error_type;
CREATE INDEX idx_error_type ON error_logs(bot_id, error_type, timestamp);

-- 3.9 feature_versions
ALTER TABLE feature_versions ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
DROP INDEX IF EXISTS idx_feature_versions;
CREATE INDEX idx_feature_versions ON feature_versions(bot_id, feature_name, guild_id, changed_at);

-- 3.10 openai_usage
ALTER TABLE openai_usage ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
DROP INDEX IF EXISTS idx_openai_usage_user_ts;
DROP INDEX IF EXISTS idx_openai_usage_guild_ts;
DROP INDEX IF EXISTS idx_openai_usage_timestamp;
CREATE INDEX idx_openai_usage_user_ts ON openai_usage(bot_id, user_id, timestamp);
CREATE INDEX idx_openai_usage_guild_ts ON openai_usage(bot_id, guild_id, timestamp);
CREATE INDEX idx_openai_usage_timestamp ON openai_usage(bot_id, timestamp);

-- ============================================================================
-- Final validation and cleanup
-- ============================================================================

PRAGMA foreign_key_check;

COMMIT;

-- Re-enable foreign keys
PRAGMA foreign_keys = ON;

-- Optimize database
VACUUM;
ANALYZE;

-- Verify migration was successful
SELECT 'Migration completed successfully. Tables updated:' AS status;
SELECT name FROM sqlite_master WHERE type='table' ORDER BY name;
