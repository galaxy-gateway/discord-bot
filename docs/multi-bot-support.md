# Multi-Discord-App Support Implementation Plan

## Executive Summary

This document outlines a comprehensive plan to enable the persona bot to support multiple Discord applications simultaneously. Currently, the bot is designed for a single Discord application with one token and one identity. This plan details the architectural changes needed to run multiple bots concurrently while sharing resources efficiently.

**Estimated Timeline**: 2.5-3 weeks
**Complexity**: Medium-High
**Risk Level**: Medium (primarily database migration)

---

## Current Architecture Analysis

### What Works Well (No Changes Needed)

1. **Persona System** ([src/personas.rs](../src/personas.rs))
   - Already stateless and shareable across multiple bots
   - Loads personas from `/prompt/*.md` files
   - No bot-specific state
   - ✅ Ready for multi-bot use

2. **OpenAI Integration**
   - Stateless API client
   - Can be shared across all bots
   - ✅ Ready for multi-bot use

3. **Modular Architecture**
   - Clean separation of concerns
   - Use of `Arc<>` for shared resources
   - Good foundation for multi-bot support

### Critical Blockers

#### 1. Database Schema ([src/database.rs](../src/database.rs))

**Problem**: No bot identity tracking

Current tables lack `bot_id` column. **All 27 tables** need migration:

**Core Tables**:
- `user_preferences`: Keyed only by `user_id` - conflicts across bots
- `user_profiles`: User profile data per bot
- `conversation_history`: Keyed by `user_id` + `channel_id` - can't distinguish bots
- `message_metadata`: Message context per bot
- `interaction_sessions`: Session tracking per bot

**Feature Tables**:
- `user_bookmarks`, `reminders`, `custom_commands`
- `daily_analytics`, `performance_metrics`, `error_logs`
- `feature_flags`, `feature_versions`, `bot_settings`, `extended_user_preferences`

**Conflict System**:
- `conflict_detection`, `mediation_history`, `user_interaction_patterns`, `channel_settings`

**Usage Tracking**:
- `openai_usage`, `openai_usage_daily`
- `dm_sessions`, `dm_session_metrics`, `dm_events`

**Guild Settings**:
- `guild_settings`: Keyed by `guild_id` + `setting_key` - same guild needs different settings per bot

**Impact**: HIGH - Requires schema migration for 27 tables and ~90+ method updates

#### 2. Configuration System ([src/config.rs](../src/config.rs))

**Problem**: Hardcoded single bot design

```rust
pub struct Config {
    pub discord_token: String,          // Only ONE token
    pub openai_api_key: String,
    pub database_path: String,
    pub log_level: String,
}
```

- Loads from `DISCORD_MUPPET_FRIEND` env variable
- No concept of multiple bot identities
- No structure for per-bot configuration

**Impact**: HIGH - Needs complete redesign

#### 3. Entry Point ([src/bin/bot.rs](../src/bin/bot.rs))

**Problem**: Single synchronous client

```rust
let mut client = Client::builder(&config.discord_token, intents)
    .event_handler(handler)
    .await?;

client.start().await?;  // Blocks forever - can't start another bot
```

**Impact**: MEDIUM - Needs async task spawning

#### 4. Command Handler ([src/commands.rs](../src/commands.rs))

**Problem**: No bot context awareness

- All database calls lack `bot_id` parameter
- Rate limiting per user, not per bot-user
- No way to distinguish which bot is handling a command

**Impact**: MEDIUM - Needs context propagation

---

## Implementation Plan

### Phase 1: Database Multi-Tenancy ⚠️ CRITICAL FIRST STEP

#### 1.1 Schema Migration

Add `bot_id TEXT NOT NULL` to all 27 tables. **Important**: SQLite does not support modifying primary keys with `ALTER TABLE`. Tables must be recreated.

```sql
-- migrations/001_add_bot_id.sql
-- SQLite requires table recreation to modify primary keys

PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;

-- Example: user_preferences table migration pattern
-- Apply this pattern to all 27 tables

-- 1. Create new table with bot_id in primary key
CREATE TABLE user_preferences_new (
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    persona TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (bot_id, user_id)
);

-- 2. Copy existing data with default bot_id
INSERT INTO user_preferences_new (bot_id, user_id, persona, created_at)
SELECT 'default', user_id, persona, created_at FROM user_preferences;

-- 3. Drop old table and rename new
DROP TABLE user_preferences;
ALTER TABLE user_preferences_new RENAME TO user_preferences;

-- Repeat for all other tables...
-- (See Appendix B for complete schema)

COMMIT;
PRAGMA foreign_keys=ON;

-- Enable WAL mode for better concurrent access
PRAGMA journal_mode=WAL;
PRAGMA busy_timeout=5000;
PRAGMA synchronous=NORMAL;
```

**Note**: The migration script generator should be created to handle all 27 tables automatically.

#### 1.2 Database Method Updates

Update all methods in `src/database.rs` to accept `bot_id` parameter:

**Before**:
```rust
pub async fn get_user_persona(&self, user_id: &str) -> Result<Option<String>>
```

**After**:
```rust
pub async fn get_user_persona(&self, bot_id: &str, user_id: &str) -> Result<Option<String>>
```

Affected methods (~90+ total across all features):

**Core Methods**:
- `get_user_persona`, `set_user_persona`
- `get_conversation_history`, `store_message`, `clear_conversation_history`
- `get_guild_setting`, `set_guild_setting`

**Usage Tracking**:
- `record_command_usage`, `get_usage_stats`
- OpenAI usage methods (raw and daily aggregates)

**DM Session Tracking**:
- `create_dm_session`, `end_dm_session`, `get_dm_stats`
- `record_dm_event`, `get_dm_session_metrics`

**Conflict System**:
- `store_conflict_detection`, `get_mediation_history`
- `get_user_interaction_patterns`, `get_channel_settings`

**Feature Flags & Analytics**:
- `get_feature_flag`, `set_feature_flag`
- `record_analytics`, `get_daily_analytics`

**And all other database operations...**

#### 1.3 Migration Strategy

**Option A**: Assign existing data to default bot
- Set `bot_id = 'default'` for all existing records
- New bots get unique IDs (e.g., 'muppet', 'chef', 'teacher')
- Pros: Preserves existing data
- Cons: Migration required for live databases

**Option B**: Fresh start
- Drop and recreate tables with new schema
- Pros: Clean implementation
- Cons: Lose all conversation history

**Recommendation**: Option A with SQL migration script

#### 1.4 Deliverables

- [ ] SQL migration script generator for all 27 tables: `migrations/001_add_bot_id.sql`
- [ ] Updated database schema in `database.rs`
- [ ] All ~90+ database methods accept `bot_id` parameter
- [ ] Integration tests for multi-bot data isolation
- [ ] Migration guide for production databases
- [ ] Rollback script for failed migrations

**Estimated Time**: 5-7 days (due to 27 tables, ~90+ methods)

---

### Phase 2: Configuration System Redesign

#### 2.1 New Configuration Structures

```rust
// src/config.rs

#[derive(Debug, Clone, Deserialize)]
pub struct BotConfig {
    /// Unique identifier for this bot instance (used in database)
    /// This is separate from `name` to allow friendly display names
    pub bot_id: String,

    /// Friendly name for logging and display
    pub name: String,

    /// Discord bot token
    pub discord_token: String,

    /// Optional: Default persona for this bot
    pub default_persona: Option<String>,

    /// Optional: Development guild ID for faster slash command registration
    pub discord_guild_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MultiConfig {
    /// List of bot configurations
    pub bots: Vec<BotConfig>,

    /// Shared OpenAI API key
    pub openai_api_key: String,

    /// Shared database path
    pub database_path: String,

    /// Default OpenAI model
    pub openai_model: String,

    /// Logging configuration
    pub log_level: String,

    /// Conflict mediation settings
    pub conflict_mediation_enabled: bool,
    pub conflict_sensitivity: String,
    pub mediation_cooldown_minutes: u64,

    /// Health check configuration (required for production)
    pub health_check_port: u16,
}

impl MultiConfig {
    /// Load from YAML/JSON file with environment variable interpolation
    pub fn from_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let interpolated = Self::interpolate_env_vars(&content);
        serde_yaml::from_str(&interpolated)
    }

    /// Load from environment variables (backward compatible)
    pub fn from_env_single_bot() -> Result<Self> {
        // Creates MultiConfig with single bot from DISCORD_MUPPET_FRIEND
    }

    /// Interpolate ${VAR_NAME} and ${VAR_NAME:-default} patterns
    fn interpolate_env_vars(content: &str) -> String {
        use regex::{Regex, Captures};
        let re = Regex::new(r"\$\{([^}:-]+)(?::-([^}]*))?\}").unwrap();
        re.replace_all(content, |caps: &Captures| {
            let var_name = &caps[1];
            let default = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            std::env::var(var_name).unwrap_or_else(|_| default.to_string())
        }).to_string()
    }
}
```

#### 2.2 Configuration File Format

**config.yaml**:
```yaml
bots:
  # bot_id is the database identifier (lowercase, no spaces)
  # name is the friendly display name for logging
  - bot_id: "muppet"
    name: "Muppet Friend"
    discord_token: "${DISCORD_MUPPET_TOKEN}"
    default_persona: "muppet"
    # discord_guild_id: "${DISCORD_GUILD_ID}"  # Optional: for dev mode

  - bot_id: "chef"
    name: "Chef Bot"
    discord_token: "${DISCORD_CHEF_TOKEN}"
    default_persona: "chef"

  - bot_id: "teacher"
    name: "Teacher Bot"
    discord_token: "${DISCORD_TEACHER_TOKEN}"
    default_persona: "teacher"

# Shared configuration
openai_api_key: "${OPENAI_API_KEY}"
openai_model: "${OPENAI_MODEL:-gpt-4o-mini}"
database_path: "${DATABASE_PATH:-./persona.db}"
log_level: "${LOG_LEVEL:-info}"

# Conflict mediation (shared settings)
conflict_mediation_enabled: true
conflict_sensitivity: "medium"
mediation_cooldown_minutes: 5

# Health check endpoint (required for production)
health_check_port: 8080
```

#### 2.3 Backward Compatibility

Support both old and new configuration methods:

```rust
// Option 1: New multi-bot config file
let config = MultiConfig::from_file("config.yaml")?;

// Option 2: Legacy single-bot env vars
let config = MultiConfig::from_env_single_bot()?;
```

#### 2.4 Deliverables

- [ ] New config structures in `config.rs`
- [ ] YAML/JSON file parsing support
- [ ] Environment variable interpolation
- [ ] Backward compatibility layer
- [ ] Example `config.yaml` file
- [ ] Configuration validation

**Estimated Time**: 1-2 days

---

### Phase 3: Multi-Client Gateway Architecture

#### 3.1 Refactor Entry Point

**Current** ([src/bin/bot.rs](../src/bin/bot.rs)):
```rust
#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    // Single client only
    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(handler)
        .await?;

    client.start().await?;  // Blocks forever
}
```

**New**:
```rust
#[tokio::main]
async fn main() -> Result<()> {
    // Load multi-bot configuration
    let config = if Path::new("config.yaml").exists() {
        MultiConfig::from_file("config.yaml")?
    } else {
        MultiConfig::from_env_single_bot()?
    };

    // Shared resources
    let database = Arc::new(Database::new(&config.database_path).await?);
    let persona_manager = Arc::new(PersonaManager::new());
    let openai_api_key = config.openai_api_key.clone();

    // Spawn one task per bot
    let mut handles = vec![];

    for bot_config in config.bots {
        let db = Arc::clone(&database);
        let pm = Arc::clone(&persona_manager);
        let api_key = openai_api_key.clone();

        let handle = tokio::spawn(async move {
            run_bot(bot_config, db, pm, api_key).await
        });

        handles.push(handle);
    }

    // Wait for all bots (or first failure)
    let results = futures::future::join_all(handles).await;

    // Handle errors
    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(Ok(())) => info!("Bot {} exited successfully", i),
            Ok(Err(e)) => error!("Bot {} failed: {}", i, e),
            Err(e) => error!("Bot {} task panicked: {}", i, e),
        }
    }

    Ok(())
}

async fn run_bot(
    bot_config: BotConfig,
    database: Arc<Database>,
    persona_manager: Arc<PersonaManager>,
    openai_api_key: String,
) -> Result<()> {
    info!("Starting bot: {} ({})", bot_config.name, bot_config.bot_id);

    let command_handler = CommandHandler::new(
        bot_config.bot_id.clone(),  // NEW: Pass bot_id
        persona_manager,
        database,
        openai_api_key,
    );

    let handler = Handler {
        command_handler: Arc::new(command_handler),
    };

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_VOICE_STATES;

    let mut client = Client::builder(&bot_config.discord_token, intents)
        .event_handler(handler)
        .await?;

    client.start().await?;

    Ok(())
}
```

#### 3.2 Error Handling & Restart Logic

```rust
// Add retry logic for individual bot failures
async fn run_bot_with_retry(/* ... */) -> Result<()> {
    let mut retry_count = 0;
    const MAX_RETRIES: u32 = 5;

    loop {
        match run_bot(/* ... */).await {
            Ok(()) => break,
            Err(e) if retry_count < MAX_RETRIES => {
                error!("Bot {} failed: {}. Retrying ({}/{})",
                    bot_config.name, e, retry_count + 1, MAX_RETRIES);
                retry_count += 1;
                tokio::time::sleep(Duration::from_secs(5 * retry_count as u64)).await;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}
```

#### 3.3 Graceful Shutdown

Proper shutdown is critical for multi-bot deployments to avoid message loss and ensure clean disconnection.

```rust
use tokio::signal;
use std::time::Duration;

// Shutdown configuration
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

// In main()
let shutdown_token = CancellationToken::new();

// Clone for each bot task
for bot_config in config.bots {
    let token = shutdown_token.clone();
    let handle = tokio::spawn(async move {
        run_bot_with_shutdown(bot_config, db, pm, api_key, token).await
    });
    handles.push(handle);
}

// Wait for shutdown signal or bot completion
tokio::select! {
    _ = signal::ctrl_c() => {
        info!("Received Ctrl+C, initiating graceful shutdown...");
        shutdown_token.cancel();

        // Wait for bots to finish with timeout
        match tokio::time::timeout(SHUTDOWN_TIMEOUT, async {
            // 1. Stop accepting new messages
            info!("Stopping message processing...");

            // 2. Wait for in-flight requests to complete
            for (bot_id, shard_manager) in &shard_managers {
                info!("Disconnecting bot: {}", bot_id);
                shard_manager.shutdown_all().await;
            }

            // 3. Flush database writes
            info!("Flushing database...");
            database.flush().await;

            // 4. Wait for all bot tasks
            futures::future::join_all(handles).await
        }).await {
            Ok(_) => info!("Graceful shutdown completed"),
            Err(_) => warn!("Shutdown timed out after {:?}", SHUTDOWN_TIMEOUT),
        }
    }
    results = futures::future::join_all(handles) => {
        // Handle normal completion (all bots exited)
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(Ok(())) => info!("Bot {} exited successfully", i),
                Ok(Err(e)) => error!("Bot {} failed: {}", i, e),
                Err(e) => error!("Bot {} task panicked: {}", i, e),
            }
        }
    }
}
```

**Shutdown Sequence**:
1. Signal all bots to stop accepting new messages
2. Wait for in-flight requests to complete (with timeout)
3. Disconnect from Discord Gateway cleanly
4. Flush pending database writes
5. Exit process

#### 3.4 Deliverables

- [ ] Refactored `bin/bot.rs` with multi-client spawning
- [ ] Shared resource management (Arc-wrapped)
- [ ] Per-bot error handling and logging
- [ ] Graceful shutdown mechanism
- [ ] Bot restart logic for transient failures
- [ ] Structured logging with bot_id context

**Estimated Time**: 2-4 days

---

### Phase 4: Context Propagation

#### 4.1 Update CommandHandler

**Current**:
```rust
pub struct CommandHandler {
    persona_manager: PersonaManager,
    database: Database,
    rate_limiter: RateLimiter,
    audio_transcriber: AudioTranscriber,
    openai_api_key: String,
}
```

**New**:
```rust
pub struct CommandHandler {
    bot_id: String,  // NEW: Bot identity
    persona_manager: PersonaManager,
    database: Database,
    rate_limiter: RateLimiter,
    audio_transcriber: AudioTranscriber,
    openai_api_key: String,
}

impl CommandHandler {
    pub fn new(
        bot_id: String,  // NEW parameter
        persona_manager: Arc<PersonaManager>,
        database: Arc<Database>,
        openai_api_key: String,
    ) -> Self {
        Self {
            bot_id,
            persona_manager: (*persona_manager).clone(),
            database: (*database).clone(),
            rate_limiter: RateLimiter::new(),
            audio_transcriber: AudioTranscriber::new(),
            openai_api_key,
        }
    }
}
```

#### 4.2 Update All Command Methods

**Example - handle_chat**:
```rust
// Before
pub async fn handle_chat(&self, ctx: &Context, msg: &Message) -> Result<()> {
    let persona = self.database.get_user_persona(&msg.author.id.to_string()).await?;
    // ...
}

// After
pub async fn handle_chat(&self, ctx: &Context, msg: &Message) -> Result<()> {
    let persona = self.database
        .get_user_persona(&self.bot_id, &msg.author.id.to_string())
        .await?;
    // ...
}
```

Apply this pattern to all methods:
- `handle_chat`
- `handle_persona_command`
- `handle_clear_command`
- `handle_help_command`
- `handle_stats_command`
- All other command handlers

#### 4.3 Update Rate Limiter

The rate limiter already uses `DashMap` for thread-safe concurrent access. Only the key type needs updating.

**Current** (in `src/features/rate_limiting/limiter.rs`):
```rust
// Rate limiter keyed by user_id only - already thread-safe with DashMap
pub struct RateLimiter {
    requests: DashMap<String, Vec<Instant>>,  // Key: user_id
    max_requests: usize,
    time_window: Duration,
}
```

**New**:
```rust
// Rate limiter keyed by "bot_id:user_id" composite key
pub struct RateLimiter {
    requests: DashMap<String, Vec<Instant>>,  // Key: "bot_id:user_id"
    max_requests: usize,
    time_window: Duration,
}

impl RateLimiter {
    pub fn check_rate_limit(&self, bot_id: &str, user_id: &str) -> bool {
        let key = format!("{}:{}", bot_id, user_id);
        // ... rest of logic unchanged
    }

    pub async fn wait_for_rate_limit(&self, bot_id: &str, user_id: &str) {
        let key = format!("{}:{}", bot_id, user_id);
        // ... rest of logic unchanged
    }
}
```

**Note**: Using a composite string key (`"bot_id:user_id"`) is simpler than a tuple key and maintains DashMap compatibility.

#### 4.4 Update All Database Calls

Systematically update every database call to include `bot_id`:

```rust
// Pattern: Add &self.bot_id as first parameter
self.database.method_name(&self.bot_id, /* other params */).await?;
```

#### 4.5 Deliverables

- [ ] Add `bot_id` field to `CommandHandler`
- [ ] Update all command handler methods
- [ ] Update rate limiter to use composite keys
- [ ] Update all database calls with bot_id
- [ ] Add integration tests for context isolation
- [ ] Verify no conversation bleeding between bots

**Estimated Time**: 3-4 days (more features to update)

---

### Phase 5: Feature-Specific Updates

Several existing features require bot_id context propagation beyond the core command handler.

#### 5.1 Conflict Detection & Mediation

Location: `src/features/conflict/`

```rust
// detector.rs - Add bot_id to conflict tracking
impl ConflictDetector {
    pub async fn detect_conflict(
        &self,
        bot_id: &str,  // NEW
        channel_id: &str,
        user_id: &str,
        message: &str,
    ) -> Result<Option<ConflictEvent>> {
        // Store conflict with bot context
        self.database.store_conflict_detection(bot_id, channel_id, user_id, ...).await?;
    }
}

// mediator.rs - Add bot_id to mediation history
impl ConflictMediator {
    pub async fn mediate(
        &self,
        bot_id: &str,  // NEW
        conflict: &ConflictEvent,
    ) -> Result<MediationResponse> {
        // Track mediation per bot
        self.database.record_mediation(bot_id, ...).await?;
    }
}
```

#### 5.2 Reminders Scheduler

Location: `src/features/reminders/scheduler.rs`

The background scheduler needs bot context to send reminders from the correct bot.

```rust
pub struct ReminderScheduler {
    bot_id: String,  // NEW: Which bot owns these reminders
    database: Arc<Database>,
    http: Arc<Http>,  // Bot-specific HTTP client
}

impl ReminderScheduler {
    pub fn new(bot_id: String, database: Arc<Database>, http: Arc<Http>) -> Self {
        Self { bot_id, database, http }
    }

    pub async fn check_and_send_reminders(&self) -> Result<()> {
        // Only fetch reminders for this bot
        let reminders = self.database
            .get_pending_reminders(&self.bot_id)
            .await?;
        // ...
    }
}
```

#### 5.3 DM Session Tracking

Location: `src/database.rs` (DM-related methods)

DM sessions must be tracked per-bot since users may DM multiple bots.

```rust
// All DM methods need bot_id
pub async fn create_dm_session(
    &self,
    bot_id: &str,  // NEW
    user_id: &str,
    channel_id: &str,
) -> Result<i64>;

pub async fn get_dm_stats(
    &self,
    bot_id: &str,  // NEW
    user_id: Option<&str>,
) -> Result<DmStats>;
```

#### 5.4 Analytics & Usage Tracking

Location: `src/features/analytics/`

```rust
// interaction_tracker.rs
impl InteractionTracker {
    pub async fn record_interaction(
        &self,
        bot_id: &str,  // NEW
        user_id: &str,
        command: &str,
    ) -> Result<()>;
}

// usage_tracker.rs
impl UsageTracker {
    pub async fn record_openai_usage(
        &self,
        bot_id: &str,  // NEW
        tokens: u32,
        cost: f64,
    ) -> Result<()>;
}
```

#### 5.5 Feature Flags

Location: `src/database.rs`

Feature flags can be per-bot for gradual rollouts.

```rust
pub async fn get_feature_flag(
    &self,
    bot_id: &str,  // NEW
    flag_name: &str,
) -> Result<Option<bool>>;

pub async fn set_feature_flag(
    &self,
    bot_id: &str,  // NEW
    flag_name: &str,
    enabled: bool,
) -> Result<()>;
```

#### 5.6 Deliverables

- [ ] Update ConflictDetector with bot_id context
- [ ] Update ConflictMediator with bot_id context
- [ ] Update ReminderScheduler to be bot-aware
- [ ] Update all DM session methods
- [ ] Update InteractionTracker and UsageTracker
- [ ] Update feature flag methods
- [ ] Add tests for feature isolation

**Estimated Time**: 2-3 days

---

### Phase 6: Sharding Considerations

For bots in 5000+ guilds, Discord requires sharding. Each shard handles a subset of guilds.

#### 6.1 Sharding Architecture

```rust
use serenity::client::bridge::gateway::ShardManager;

async fn run_bot(
    bot_config: BotConfig,
    database: Arc<Database>,
    // ...
) -> Result<()> {
    let mut client = Client::builder(&bot_config.discord_token, intents)
        .event_handler(handler)
        .await?;

    // For large bots, access shard manager
    let shard_manager = client.shard_manager.clone();

    // Store for graceful shutdown
    SHARD_MANAGERS.lock().await.insert(
        bot_config.bot_id.clone(),
        shard_manager,
    );

    client.start_autosharded().await?;  // Auto-shard based on guild count
    Ok(())
}
```

#### 6.2 Shard-Aware Logging

```rust
use tracing::{info, Span};

// Include shard ID in logs
info!(
    bot_id = %self.bot_id,
    shard_id = %ctx.shard_id,
    "Processing message"
);
```

#### 6.3 When to Consider Sharding

| Guilds | Recommendation |
|--------|---------------|
| < 2,500 | No sharding needed |
| 2,500 - 5,000 | Optional, may improve responsiveness |
| > 5,000 | Required by Discord |

**Note**: Each bot in a multi-bot setup shards independently based on its own guild count.

---

## Testing Strategy

### Unit Tests

```rust
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
            Some("muppet".to_string())
        );
        assert_eq!(
            db.get_user_persona("bot2", "user123").await.unwrap(),
            Some("chef".to_string())
        );
    }

    #[tokio::test]
    async fn test_conversation_history_isolation() {
        // Similar test for conversation history
    }

    #[test]
    fn test_rate_limiter_per_bot() {
        let mut limiter = RateLimiter::new();

        // User should be rate limited per bot, not globally
        assert!(limiter.check_rate_limit("bot1", "user123"));
        assert!(limiter.check_rate_limit("bot2", "user123"));  // Different bot, should pass
    }
}
```

### Integration Tests

1. **Multi-Bot Startup**: Verify all bots connect successfully
2. **Data Isolation**: Send commands to different bots, verify no data bleeding
3. **Concurrent Operations**: Stress test with simultaneous requests to all bots
4. **Bot Failure Recovery**: Kill one bot, verify others continue
5. **Configuration Loading**: Test both YAML and env var configurations

### Test Mocking Strategy

Running integration tests without real Discord tokens requires mocking. Here's the recommended approach:

#### Test Fixtures

```rust
#[cfg(test)]
mod test_fixtures {
    use super::*;

    /// Create an in-memory database for testing
    pub async fn test_database() -> Database {
        Database::new(":memory:").await.expect("Failed to create test database")
    }

    /// Create test bot configurations
    pub fn test_bot_configs() -> (BotConfig, BotConfig) {
        let bot1 = BotConfig {
            bot_id: "test_bot_1".to_string(),
            name: "Test Bot 1".to_string(),
            discord_token: "fake_token_1".to_string(),
            default_persona: Some("muppet".to_string()),
            discord_guild_id: None,
        };
        let bot2 = BotConfig {
            bot_id: "test_bot_2".to_string(),
            name: "Test Bot 2".to_string(),
            discord_token: "fake_token_2".to_string(),
            default_persona: Some("chef".to_string()),
            discord_guild_id: None,
        };
        (bot1, bot2)
    }

    /// Setup multi-bot test environment
    pub async fn setup_multi_bot_test() -> (Database, BotConfig, BotConfig) {
        let db = test_database().await;
        let (bot1, bot2) = test_bot_configs();
        (db, bot1, bot2)
    }
}
```

#### Mocking Discord API (Optional)

For tests that need Discord API responses, use `mockito` or similar:

```rust
#[cfg(test)]
mod discord_mock_tests {
    use mockito::{Server, Mock};

    #[tokio::test]
    async fn test_bot_connection() {
        let mut server = Server::new();

        // Mock Discord gateway
        let _m = server.mock("GET", "/gateway")
            .with_status(200)
            .with_body(r#"{"url": "wss://gateway.discord.gg"}"#)
            .create();

        // Test connection logic...
    }
}
```

#### Running Tests Without Discord

```bash
# Run database-only tests (no Discord connection)
cargo test --lib database::

# Run all unit tests
cargo test --lib

# Run with test logging
RUST_LOG=debug cargo test -- --nocapture
```

### Manual Testing Checklist

- [ ] Start 2+ bots with different tokens
- [ ] Send DM to each bot, verify separate conversation histories
- [ ] Set different personas on same user across bots
- [ ] Verify guild settings are per-bot
- [ ] Check usage stats tracked separately
- [ ] Test rate limiting per bot
- [ ] Verify graceful shutdown
- [ ] Test bot restart after crash

---

## Migration Guide

### For Existing Deployments

#### Step 1: Backup Database
```bash
cp persona.db persona.db.backup
```

#### Step 2: Run Migration Script
```bash
sqlite3 persona.db < migrations/001_add_bot_id.sql
```

#### Step 3: Update Configuration

Create `config.yaml`:
```yaml
bots:
  - bot_id: "default"  # Match migration default
    name: "Main Bot"
    discord_token: "${DISCORD_MUPPET_FRIEND}"
    default_persona: "muppet"

openai_api_key: "${OPENAI_API_KEY}"
database_path: "./persona.db"
log_level: "info"
```

#### Step 4: Deploy New Version

```bash
cargo build --release
./target/release/bot  # Will auto-detect config.yaml
```

#### Step 5: Add Additional Bots

Edit `config.yaml` to add more bot configurations, then restart.

---

## Monitoring & Observability

### Structured Logging

```rust
use tracing::{info, error, warn};

// Log with bot context
info!(
    bot_id = %self.bot_id,
    user_id = %user_id,
    "Processing chat command"
);
```

### Metrics to Track

Per bot:
- Active connections
- Messages processed
- Commands executed
- Rate limit hits
- Errors encountered
- API latency (OpenAI, Discord)

### Health Checks (Required)

A health check endpoint is **required** for production deployments. This enables load balancers, container orchestrators, and monitoring systems to verify bot health.

```rust
use axum::{Router, routing::get, Json, extract::State};
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct HealthStatus {
    pub status: &'static str,
    pub bots: Vec<BotHealth>,
    pub database_connected: bool,
    pub uptime_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct BotHealth {
    pub bot_id: String,
    pub name: String,
    pub connected: bool,
    pub guilds: usize,
    pub latency_ms: Option<u64>,
}

// Health check endpoint - spawned alongside bots
async fn health_check(State(registry): State<Arc<BotRegistry>>) -> Json<HealthStatus> {
    let bots = registry.bots.iter().map(|(id, bot)| {
        BotHealth {
            bot_id: id.clone(),
            name: bot.name.clone(),
            connected: bot.is_connected(),
            guilds: bot.guild_count(),
            latency_ms: bot.latency(),
        }
    }).collect();

    Json(HealthStatus {
        status: "ok",
        bots,
        database_connected: registry.database.is_connected().await,
        uptime_seconds: registry.uptime().as_secs(),
    })
}

// In main() - spawn health check server
let health_app = Router::new()
    .route("/health", get(health_check))
    .route("/ready", get(readiness_check))
    .with_state(Arc::clone(&registry));

tokio::spawn(async move {
    let addr = format!("0.0.0.0:{}", config.health_check_port);
    info!("Health check endpoint starting on {}", addr);
    axum::Server::bind(&addr.parse().unwrap())
        .serve(health_app.into_make_service())
        .await
        .expect("Health check server failed");
});
```

**Endpoints**:
- `GET /health` - Overall system health (for monitoring)
- `GET /ready` - Readiness probe (for Kubernetes/load balancers)

---

## Performance Considerations

### Resource Usage

**Per Bot**:
- 1 WebSocket connection to Discord Gateway
- ~10-50 MB memory (depending on cache size)
- Minimal CPU (event-driven)

**Shared**:
- SQLite database (single file, thread-safe)
- OpenAI HTTP client (connection pool)
- Persona manager (lightweight, in-memory)

**Scaling**: Should easily support 5-10 bots on modest hardware (2 CPU, 4GB RAM)

### Rate Limits

Discord API limits (per bot):
- 50 requests/second global
- 5 requests/second per channel
- 1 gateway connection per shard (5000 guilds)

**Mitigation**: Each bot has independent rate limits since they're separate applications.

### Database Contention

SQLite handles concurrent reads well but serializes writes. With multiple bots:
- Use WAL mode: `PRAGMA journal_mode=WAL;`
- Keep transactions short
- Consider connection pool if needed

---

## Risk Assessment

### High Risk

1. **Database Migration Failure**
   - Mitigation: Mandatory backup, rollback script, test on copy first

2. **Data Leakage Between Bots**
   - Mitigation: Extensive integration tests, code review on all database calls

### Medium Risk

1. **Bot Crash Affecting Others**
   - Mitigation: Isolated async tasks, error boundaries, restart logic

2. **Configuration Errors**
   - Mitigation: Validation on load, clear error messages, schema validation

### Low Risk

1. **Performance Degradation**
   - Mitigation: Monitoring, load testing before production

2. **Discord API Changes**
   - Mitigation: Pin serenity version, gradual upgrades

---

## Rollback Plan

If multi-bot deployment fails:

1. **Stop New Version**
   ```bash
   killall bot
   ```

2. **Restore Database Backup** (if migration was run)
   ```bash
   mv persona.db.backup persona.db
   ```

3. **Deploy Previous Version**
   ```bash
   git checkout <previous-tag>
   cargo build --release
   ./target/release/bot
   ```

4. **Revert to Env Var Configuration**
   ```bash
   export DISCORD_MUPPET_FRIEND=<token>
   ```

---

## Future Enhancements

### Phase 6+: Advanced Features

1. **Dynamic Bot Management**
   - Add/remove bots without restart
   - Hot-reload configuration
   - Admin API for bot management

2. **Per-Bot Customization**
   - Custom personas per bot
   - Different OpenAI models per bot
   - Bot-specific rate limits

3. **Cross-Bot Features**
   - User preferences that follow across bots
   - Shared conversation context (opt-in)
   - Bot-to-bot communication

4. **Scaling**
   - PostgreSQL for high-concurrency deployments
   - Redis for distributed rate limiting
   - Separate processes for Gateway vs HTTP bots

5. **Monitoring Dashboard**
   - Real-time bot status
   - Usage analytics per bot
   - Cost tracking per bot (OpenAI API)

---

## Open Questions - Decisions Made

The following decisions have been made for implementation:

| Question | Decision | Rationale |
|----------|----------|-----------|
| **Data Sharing** | Per-bot by default, no sharing | Prevents accidental data leakage; simpler mental model |
| **Bot Identification** | Use custom `bot_id` field | More flexible than Discord app_id; human-readable |
| **Config Management** | Support both file and env vars | Backward compatible; env vars for simple deployments |
| **Deployment Model** | Single process for all bots | Simpler operations; shared resources; easier debugging |
| **Health Check** | Required (not optional) | Essential for production monitoring and orchestration |

### Detailed Decisions

1. **Data Sharing Philosophy**
   - **Decision**: All data is per-bot by default
   - User preferences, conversation history, and settings are completely isolated
   - No cross-bot data sharing in initial implementation
   - Future enhancement could add opt-in sharing

2. **Bot Identification**
   - **Decision**: Use custom `bot_id` string (e.g., "muppet", "chef")
   - Separate from display `name` field for flexibility
   - Used as database key, log identifier, and metrics label
   - Must be lowercase, alphanumeric, no spaces

3. **Configuration Management**
   - **Decision**: Support both methods with file taking precedence
   - If `config.yaml` exists, use it (with env var interpolation)
   - Otherwise, fall back to `DISCORD_MUPPET_FRIEND` for single-bot mode
   - Remote config (S3, HTTP) deferred to future enhancement

4. **Deployment Model**
   - **Decision**: Single monolithic process
   - All bots run in one process with shared database connection
   - Simpler Docker setup (one container)
   - Per-bot processes can be added later if needed

---

## Appendix A: File Change Summary

### Major Changes Required

| File | Changes | Estimated Lines | Complexity |
|------|---------|-----------------|------------|
| `src/core/config.rs` | Add MultiConfig, YAML parsing, env interpolation | ~150 | High |
| `src/database.rs` | Add bot_id to all ~90 methods, 27 table migrations | ~500 | High |
| `src/bin/bot.rs` | Multi-client spawning, health endpoint, shutdown | ~250 | High |
| `src/command_handler.rs` | Add bot_id field, update all methods | ~200 | Medium |
| `src/features/rate_limiting/limiter.rs` | Composite string keys | ~30 | Low |
| `src/features/conflict/detector.rs` | Add bot_id to detection | ~50 | Medium |
| `src/features/conflict/mediator.rs` | Add bot_id to mediation | ~50 | Medium |
| `src/features/reminders/scheduler.rs` | Bot-aware scheduling | ~80 | Medium |
| `src/features/analytics/*.rs` | Bot-aware tracking | ~100 | Medium |

### New Files Needed

- `migrations/001_add_bot_id.sql` - Database migration (all 27 tables)
- `config.yaml.example` - Example multi-bot configuration
- `src/health.rs` - Health check endpoint (required)
- `docs/multi-bot-setup.md` - User-facing setup guide
- `tests/integration/multi_bot_tests.rs` - Integration tests
- `tests/fixtures/mod.rs` - Test fixtures for multi-bot scenarios

### No Changes Required

- `src/features/personas/` - Already stateless and multi-bot compatible ✅
- `src/features/audio/transcriber.rs` - Stateless, shareable ✅
- `src/features/image_gen/` - Stateless, shareable ✅
- `src/commands/slash/` - Command definitions are stateless ✅

### Minor Updates Only

- `src/handler.rs` - Pass bot_id to command handler
- `src/features/message_components/` - Minor context updates

---

## Appendix B: Database Schema (After Migration)

```sql
-- User preferences (after migration)
CREATE TABLE user_preferences (
    bot_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    persona TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (bot_id, user_id)
);

-- Conversation history (after migration)
CREATE TABLE conversation_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);
CREATE INDEX idx_conversation ON conversation_history(bot_id, user_id, channel_id, timestamp);

-- Guild settings (after migration)
CREATE TABLE guild_settings (
    bot_id TEXT NOT NULL,
    guild_id TEXT NOT NULL,
    setting_key TEXT NOT NULL,
    setting_value TEXT NOT NULL,
    PRIMARY KEY (bot_id, guild_id, setting_key)
);

-- Usage statistics (after migration)
CREATE TABLE usage_stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    command TEXT NOT NULL,
    timestamp INTEGER NOT NULL
);
CREATE INDEX idx_usage ON usage_stats(bot_id, timestamp);
```

---

## Appendix C: Example Multi-Bot Config

```yaml
# config.yaml - Full example with 4 bots

bots:
  # Muppet personality bot
  - bot_id: "muppet"
    name: "Muppet Friend"
    discord_token: "${DISCORD_MUPPET_TOKEN}"
    default_persona: "muppet"

  # Chef personality bot
  - bot_id: "chef"
    name: "Chef Bot"
    discord_token: "${DISCORD_CHEF_TOKEN}"
    default_persona: "chef"

  # Teacher personality bot
  - bot_id: "teacher"
    name: "Teacher Bot"
    discord_token: "${DISCORD_TEACHER_TOKEN}"
    default_persona: "teacher"

  # Analyst personality bot
  - bot_id: "analyst"
    name: "Analyst Bot"
    discord_token: "${DISCORD_ANALYST_TOKEN}"
    default_persona: "analyst"

# Shared configuration
openai_api_key: "${OPENAI_API_KEY}"
database_path: "./persona.db"
log_level: "info"
```

---

## Conclusion

This implementation plan provides a comprehensive roadmap to enable multi-Discord-app support. The phased approach minimizes risk while delivering incremental value. The architecture maintains the existing persona system's elegance while adding the flexibility to run multiple bot identities simultaneously.

**Key Success Factors**:
- Careful database migration with rollback plan (27 tables require recreation)
- Comprehensive testing at each phase with proper test fixtures
- Backward compatibility during transition (env vars still work)
- Clear separation of shared vs. per-bot resources
- Required health check endpoint for production monitoring
- Proper graceful shutdown handling

**Estimated Total Effort**:

| Phase | Description | Time |
|-------|-------------|------|
| Phase 1 | Database Multi-Tenancy | 5-7 days |
| Phase 2 | Configuration System | 1-2 days |
| Phase 3 | Multi-Client Architecture | 3-4 days |
| Phase 4 | Context Propagation | 3-4 days |
| Phase 5 | Feature-Specific Updates | 2-3 days |
| Phase 6 | Sharding (if needed) | 1-2 days |
| **Total** | | **~2.5-3 weeks** |

**Ready to Implement**: All open questions have been decided. See "Open Questions - Decisions Made" section for details.
