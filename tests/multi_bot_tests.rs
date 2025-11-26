//! Multi-Bot Integration Tests
//!
//! These tests verify that the multi-bot architecture properly isolates data
//! between different bot instances while sharing resources efficiently.
//!
//! Run with: `cargo test --test multi_bot_tests`

use persona::config::{BotConfig, MultiConfig};
use persona::database::Database;
use persona::rate_limiter::RateLimiter;
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// Database Isolation Tests
// ============================================================================

/// Test that user personas are isolated between bots
#[tokio::test]
async fn test_user_persona_isolation() {
    let db = Database::new(":memory:").await.unwrap();

    // Set persona for bot1
    db.set_user_persona("bot1", "user123", "muppet")
        .await
        .unwrap();

    // Set different persona for same user on bot2
    db.set_user_persona("bot2", "user123", "chef")
        .await
        .unwrap();

    // Verify isolation - each bot sees its own value
    assert_eq!(
        db.get_user_persona("bot1", "user123").await.unwrap(),
        "muppet"
    );
    assert_eq!(
        db.get_user_persona("bot2", "user123").await.unwrap(),
        "chef"
    );

    // Verify default bot sees default persona
    assert_eq!(
        db.get_user_persona("bot3", "user123").await.unwrap(),
        "obi" // Default persona
    );
}

/// Test that conversation history is isolated between bots
#[tokio::test]
async fn test_conversation_history_isolation() {
    let db = Database::new(":memory:").await.unwrap();

    // Store messages for bot1
    db.store_message(
        "bot1",
        "user1",
        "channel1",
        "user",
        "Hello from bot1 context",
        Some("muppet"),
    )
    .await
    .unwrap();

    // Store messages for bot2 in same channel
    db.store_message(
        "bot2",
        "user1",
        "channel1",
        "user",
        "Hello from bot2 context",
        Some("chef"),
    )
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
    assert!(bot1_history[0].1.contains("bot1"));

    assert_eq!(bot2_history.len(), 1);
    assert!(bot2_history[0].1.contains("bot2"));
}

/// Test that guild settings are isolated between bots
#[tokio::test]
async fn test_guild_settings_isolation() {
    let db = Database::new(":memory:").await.unwrap();

    // Set guild settings for bot1
    db.set_guild_setting("bot1", "guild123", "default_persona", "obi")
        .await
        .unwrap();
    db.set_guild_setting("bot1", "guild123", "default_verbosity", "detailed")
        .await
        .unwrap();

    // Set different settings for bot2 in same guild
    db.set_guild_setting("bot2", "guild123", "default_persona", "chef")
        .await
        .unwrap();
    db.set_guild_setting("bot2", "guild123", "default_verbosity", "concise")
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

/// Test that reminders are isolated between bots
#[tokio::test]
async fn test_reminder_isolation() {
    let db = Database::new(":memory:").await.unwrap();

    // Add reminder for bot1
    let id1 = db
        .add_reminder("bot1", "user1", "channel1", "Bot1 reminder", "2099-01-01T00:00:00")
        .await
        .unwrap();

    // Add reminder for bot2
    let _id2 = db
        .add_reminder("bot2", "user1", "channel1", "Bot2 reminder", "2099-01-01T00:00:00")
        .await
        .unwrap();

    // Get reminders for each bot
    let bot1_reminders = db.get_user_reminders("bot1", "user1").await.unwrap();
    let bot2_reminders = db.get_user_reminders("bot2", "user1").await.unwrap();

    assert_eq!(bot1_reminders.len(), 1);
    assert!(bot1_reminders[0].2.contains("Bot1"));

    assert_eq!(bot2_reminders.len(), 1);
    assert!(bot2_reminders[0].2.contains("Bot2"));

    // Verify cross-bot deletion doesn't work
    let deleted = db.delete_reminder("bot2", id1, "user1").await.unwrap();
    assert!(!deleted, "Bot2 should not be able to delete Bot1's reminder");

    // Verify same-bot deletion works
    let deleted = db.delete_reminder("bot1", id1, "user1").await.unwrap();
    assert!(deleted, "Bot1 should be able to delete its own reminder");
}

/// Test that feature flags are isolated between bots
#[tokio::test]
async fn test_feature_flag_isolation() {
    let db = Database::new(":memory:").await.unwrap();

    // Enable feature for bot1
    db.set_feature_flag("bot1", "test_feature", true, None, Some("guild1"))
        .await
        .unwrap();

    // Disable same feature for bot2
    db.set_feature_flag("bot2", "test_feature", false, None, Some("guild1"))
        .await
        .unwrap();

    // Verify isolation
    assert!(
        db.is_feature_enabled("bot1", "test_feature", None, Some("guild1"))
            .await
            .unwrap()
    );
    assert!(
        !db.is_feature_enabled("bot2", "test_feature", None, Some("guild1"))
            .await
            .unwrap()
    );
}

/// Test that basic usage logging works per bot
#[tokio::test]
async fn test_usage_logging_isolation() {
    let db = Database::new(":memory:").await.unwrap();

    // Log usage for bot1
    db.log_usage("bot1", "user1", "chat", Some("muppet"))
        .await
        .unwrap();
    db.log_usage("bot1", "user1", "chat", Some("muppet"))
        .await
        .unwrap();

    // Log usage for bot2
    db.log_usage("bot2", "user1", "imagine", None)
        .await
        .unwrap();

    // Basic verification that logging doesn't fail
    // Note: get_user_usage_stats returns Vec of OpenAI usage aggregates, not command counts
    // The log_usage function writes to usage_stats table, separate from OpenAI tracking
}

// ============================================================================
// Rate Limiter Isolation Tests
// ============================================================================

/// Test that rate limits are independent per bot
#[tokio::test]
async fn test_rate_limiter_bot_isolation() {
    let limiter = RateLimiter::new(2, Duration::from_secs(60));

    // Fill up limit for bot1/user1
    assert!(limiter.check_rate_limit("bot1", "user1").await);
    assert!(limiter.check_rate_limit("bot1", "user1").await);
    assert!(!limiter.check_rate_limit("bot1", "user1").await); // blocked

    // Same user on bot2 should NOT be blocked
    assert!(limiter.check_rate_limit("bot2", "user1").await);
    assert!(limiter.check_rate_limit("bot2", "user1").await);
    assert!(!limiter.check_rate_limit("bot2", "user1").await); // now blocked

    // Different user on bot1 should NOT be blocked
    assert!(limiter.check_rate_limit("bot1", "user2").await);
}

// ============================================================================
// Configuration Tests
// ============================================================================

/// Test that MultiConfig correctly parses multiple bots
#[test]
fn test_multi_config_multiple_bots() {
    let config = MultiConfig {
        bots: vec![
            BotConfig {
                application_id: Some("app1".to_string()),
                name: "Bot One".to_string(),
                discord_token: "token1".to_string(),
                default_persona: Some("muppet".to_string()),
                discord_guild_id: None,
                openai_model: Some("gpt-4".to_string()),
                conflict_mediation_enabled: Some(true),
                conflict_sensitivity: Some("high".to_string()),
                mediation_cooldown_minutes: Some(10),
                commands: None,
                startup_notification_enabled: Some(true),
            },
            BotConfig {
                application_id: Some("app2".to_string()),
                name: "Bot Two".to_string(),
                discord_token: "token2".to_string(),
                default_persona: Some("chef".to_string()),
                discord_guild_id: None,
                openai_model: None, // Uses global default
                conflict_mediation_enabled: None,
                conflict_sensitivity: None,
                mediation_cooldown_minutes: None,
                commands: None,
                startup_notification_enabled: Some(true),
            },
        ],
        openai_api_key: "sk-test".to_string(),
        database_path: ":memory:".to_string(),
        log_level: "info".to_string(),
        openai_model: "gpt-4o-mini".to_string(),
        conflict_mediation_enabled: true,
        conflict_sensitivity: "medium".to_string(),
        mediation_cooldown_minutes: 5,
        shared_commands: vec![],
    };

    assert!(config.validate().is_ok());
    assert_eq!(config.bots.len(), 2);

    // Test effective values with overrides
    let bot1 = &config.bots[0];
    assert_eq!(bot1.effective_model(&config.openai_model), "gpt-4");
    assert_eq!(bot1.effective_conflict_sensitivity(&config.conflict_sensitivity), "high");

    // Test effective values with defaults
    let bot2 = &config.bots[1];
    assert_eq!(bot2.effective_model(&config.openai_model), "gpt-4o-mini");
    assert_eq!(bot2.effective_conflict_sensitivity(&config.conflict_sensitivity), "medium");
}

/// Test bot_id accessor
#[test]
fn test_bot_id_accessor() {
    // With application_id set
    let bot_with_id = BotConfig {
        application_id: Some("123456789".to_string()),
        name: "Test Bot".to_string(),
        discord_token: "token".to_string(),
        default_persona: None,
        discord_guild_id: None,
        openai_model: None,
        conflict_mediation_enabled: None,
        conflict_sensitivity: None,
        mediation_cooldown_minutes: None,
        commands: None,
        startup_notification_enabled: Some(true),
    };
    assert_eq!(bot_with_id.bot_id(), "123456789");

    // Without application_id (falls back to name)
    let bot_without_id = BotConfig {
        application_id: None,
        name: "Fallback Bot".to_string(),
        discord_token: "token".to_string(),
        default_persona: None,
        discord_guild_id: None,
        openai_model: None,
        conflict_mediation_enabled: None,
        conflict_sensitivity: None,
        mediation_cooldown_minutes: None,
        commands: None,
        startup_notification_enabled: Some(true),
    };
    assert_eq!(bot_without_id.bot_id(), "Fallback Bot");
}

// ============================================================================
// Shared Resource Tests
// ============================================================================

/// Test that database can be shared across multiple bot contexts
#[tokio::test]
async fn test_shared_database_concurrent_access() {
    let db = Arc::new(Database::new(":memory:").await.unwrap());

    let db1 = db.clone();
    let db2 = db.clone();

    // Simulate concurrent access from two bots
    let handle1 = tokio::spawn(async move {
        for i in 0..10 {
            db1.log_usage("bot1", "user1", &format!("cmd{}", i), None)
                .await
                .unwrap();
        }
    });

    let handle2 = tokio::spawn(async move {
        for i in 0..10 {
            db2.log_usage("bot2", "user1", &format!("cmd{}", i), None)
                .await
                .unwrap();
        }
    });

    handle1.await.unwrap();
    handle2.await.unwrap();

    // If we got here without panics, concurrent access works
}

/// Test channel settings isolation between bots
#[tokio::test]
async fn test_channel_settings_isolation() {
    let db = Database::new(":memory:").await.unwrap();

    // Set channel verbosity for bot1
    db.set_channel_verbosity("bot1", "guild1", "channel1", "detailed")
        .await
        .unwrap();

    // Set different verbosity for bot2 in same channel
    db.set_channel_verbosity("bot2", "guild1", "channel1", "concise")
        .await
        .unwrap();

    // Verify isolation - get_channel_verbosity returns String (default "concise" if not set)
    let bot1_verbosity = db
        .get_channel_verbosity("bot1", "guild1", "channel1")
        .await
        .unwrap();
    let bot2_verbosity = db
        .get_channel_verbosity("bot2", "guild1", "channel1")
        .await
        .unwrap();

    assert_eq!(bot1_verbosity, "detailed");
    assert_eq!(bot2_verbosity, "concise");
}

/// Test conflict detection isolation between bots
#[tokio::test]
async fn test_conflict_detection_isolation() {
    let db = Database::new(":memory:").await.unwrap();

    // Record conflict for bot1 - participants is a JSON string
    let conflict1_id = db
        .record_conflict_detection(
            "bot1",
            "channel1",
            Some("guild1"),
            "[\"user1\", \"user2\"]", // JSON array as string
            "hostile_language",
            0.8,
            "msg123",
        )
        .await
        .unwrap();

    // Record different conflict for bot2 in same channel
    let conflict2_id = db
        .record_conflict_detection(
            "bot2",
            "channel1",
            Some("guild1"),
            "[\"user3\", \"user4\"]", // JSON array as string
            "rapid_exchange",
            0.6,
            "msg456",
        )
        .await
        .unwrap();

    // Get active conflicts for each bot
    let bot1_conflict = db
        .get_channel_active_conflict("bot1", "channel1")
        .await
        .unwrap();
    let bot2_conflict = db
        .get_channel_active_conflict("bot2", "channel1")
        .await
        .unwrap();

    assert_eq!(bot1_conflict, Some(conflict1_id));
    assert_eq!(bot2_conflict, Some(conflict2_id));

    // Mark bot1's conflict resolved - shouldn't affect bot2
    db.mark_conflict_resolved("bot1", conflict1_id).await.unwrap();

    let bot1_conflict_after = db
        .get_channel_active_conflict("bot1", "channel1")
        .await
        .unwrap();
    let bot2_conflict_after = db
        .get_channel_active_conflict("bot2", "channel1")
        .await
        .unwrap();

    assert_eq!(bot1_conflict_after, None); // Resolved
    assert_eq!(bot2_conflict_after, Some(conflict2_id)); // Still active
}

/// Test that clearing conversation history is isolated per bot
#[tokio::test]
async fn test_clear_conversation_history_isolation() {
    let db = Database::new(":memory:").await.unwrap();

    // Store messages for bot1
    db.store_message("bot1", "user1", "channel1", "user", "Bot1 message 1", None)
        .await.unwrap();
    db.store_message("bot1", "user1", "channel1", "assistant", "Bot1 response", None)
        .await.unwrap();

    // Store messages for bot2
    db.store_message("bot2", "user1", "channel1", "user", "Bot2 message 1", None)
        .await.unwrap();
    db.store_message("bot2", "user1", "channel1", "assistant", "Bot2 response", None)
        .await.unwrap();

    // Verify both have messages
    assert_eq!(
        db.get_conversation_history("bot1", "user1", "channel1", 10).await.unwrap().len(),
        2
    );
    assert_eq!(
        db.get_conversation_history("bot2", "user1", "channel1", 10).await.unwrap().len(),
        2
    );

    // Clear bot1's history only
    db.clear_conversation_history("bot1", "user1", "channel1").await.unwrap();

    // Bot1 should be empty, bot2 should be unchanged
    assert_eq!(
        db.get_conversation_history("bot1", "user1", "channel1", 10).await.unwrap().len(),
        0
    );
    assert_eq!(
        db.get_conversation_history("bot2", "user1", "channel1", 10).await.unwrap().len(),
        2
    );
}
