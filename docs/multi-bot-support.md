# Multi-Discord-App Support Implementation Plan

## Executive Summary

This document outlines a comprehensive plan to enable the persona bot to support multiple Discord applications simultaneously. Currently, the bot is designed for a single Discord application with one token and one identity. This plan details the architectural changes needed to run multiple bots concurrently while sharing resources efficiently.

**Estimated Timeline**: 3-4 weeks
**Complexity**: Medium-High
**Risk Level**: Medium (primarily database migration)

> **Note (Updated 2025-11)**: This plan has been revised to reflect the current codebase state. The original estimate of 2 weeks was based on 4 tables; the actual scope includes 21 tables and 80+ database methods.

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

Current database has **21 tables** lacking `bot_id` column:

**Core User Tables:**
- `user_preferences`: Keyed only by `user_id` - conflicts across bots
- `extended_user_preferences`: Per-user key-value pairs
- `conversation_history`: Keyed by `user_id` + `channel_id`

**Guild & Channel Tables:**
- `guild_settings`: Keyed by `guild_id` + `setting_key`
- `channel_settings`: Per-channel verbosity and conflict settings

**Feature & Command Tables:**
- `custom_commands`: Guild-specific custom commands
- `feature_flags`: Per-guild feature toggles
- `feature_versions`: Feature audit trail

**Interaction Tables:**
- `reminders`: User reminders (needs per-bot isolation)
- `user_bookmarks`: Message bookmarks
- `message_metadata`: Message tracking
- `interaction_sessions`: Session tracking

**Conflict Detection Tables:**
- `conflict_detection`: Active conflict tracking
- `mediation_history`: Mediation records (FK to conflict_detection)
- `user_interaction_patterns`: User interaction analysis

**Analytics Tables:**
- `usage_stats`: Command usage statistics
- `daily_analytics`: Daily aggregated metrics
- `performance_metrics`: Performance tracking
- `error_logs`: Error tracking

**Global Tables (may remain shared):**
- `bot_settings`: Global bot configuration

**Impact**: HIGH - Requires schema migration and **~80 method updates**

#### 2. Configuration System ([src/config.rs](../src/config.rs))

**Problem**: Hardcoded single bot design

```rust
pub struct Config {
    pub discord_token: String,              // Only ONE token
    pub openai_api_key: String,
    pub database_path: String,
    pub log_level: String,
    pub discord_public_key: Option<String>, // Only ONE key
    pub discord_guild_id: Option<String>,   // Dev mode guild
    pub openai_model: String,               // AI model selection
    pub conflict_mediation_enabled: bool,   // Conflict feature toggle
    pub conflict_sensitivity: String,       // Sensitivity level
    pub mediation_cooldown_minutes: u64,    // Cooldown period
}
```

- Loads from `DISCORD_SPACE_CADET` env variable
- No concept of multiple bot identities
- No structure for per-bot configuration
- Some settings (e.g., `openai_model`) could be per-bot or shared

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

#### 4. Command Handler ([src/command_handler.rs](../src/command_handler.rs))

**Problem**: No bot context awareness

Current CommandHandler structure:
```rust
pub struct CommandHandler {
    persona_manager: PersonaManager,
    database: Database,
    rate_limiter: RateLimiter,
    audio_transcriber: AudioTranscriber,
    image_generator: ImageGenerator,        // Image generation support
    openai_model: String,
    conflict_detector: ConflictDetector,    // Conflict detection
    conflict_mediator: ConflictMediator,    // Mediation support
    conflict_enabled: bool,
    conflict_sensitivity_threshold: f32,
    start_time: std::time::Instant,         // Uptime tracking
}
```

Issues:
- No `bot_id` field - cannot identify which bot is handling requests
- All database calls lack `bot_id` parameter
- Rate limiting per user, not per bot-user
- ConflictDetector and ConflictMediator need bot context

**Impact**: MEDIUM - Needs context propagation through all components

---

## Implementation Plan

### Phase 1: Database Multi-Tenancy

> **Note**: Phase order has been revised. See Phase 2 (Config) which should be implemented first as it has no dependencies.

#### 1.1 Schema Migration Strategy

**Important**: SQLite cannot modify PRIMARY KEY or UNIQUE constraints with `ALTER TABLE`. Tables requiring constraint changes must be recreated.

**Tiered Migration Approach:**

| Tier | Tables | Method | Reason |
|------|--------|--------|--------|
| **Tier 1** | 8 tables | Full recreation | PK or UNIQUE constraint changes |
| **Tier 2** | 2 tables | Ordered recreation | Foreign key dependencies |
| **Tier 3** | 10 tables | ALTER TABLE | Simple column addition |

**Tier 1 - Full Table Recreation (PK/UNIQUE changes):**
- `user_preferences` - PK: `(user_id)` → `(bot_id, user_id)`
- `guild_settings` - UNIQUE: `(guild_id, setting_key)` → `(bot_id, guild_id, setting_key)`
- `channel_settings` - UNIQUE: `(guild_id, channel_id)` → `(bot_id, guild_id, channel_id)`
- `custom_commands` - UNIQUE: `(command_name, guild_id)` → `(bot_id, command_name, guild_id)`
- `feature_flags` - UNIQUE: `(feature_name, user_id, guild_id)` → `(bot_id, feature_name, user_id, guild_id)`
- `extended_user_preferences` - UNIQUE: `(user_id, preference_key)` → `(bot_id, user_id, preference_key)`
- `user_interaction_patterns` - UNIQUE: `(user_id_a, user_id_b, channel_id)` → `(bot_id, ...)`
- `daily_analytics` - UNIQUE: `(date)` → `(bot_id, date)`

**Tier 2 - FK-Dependent Tables (order matters):**
1. `conflict_detection` - Must migrate first (parent table)
2. `mediation_history` - Has FK to conflict_detection

**Tier 3 - Simple ALTER TABLE:**
- `conversation_history`, `usage_stats`, `message_metadata`, `interaction_sessions`
- `user_bookmarks`, `reminders`, `performance_metrics`, `error_logs`, `feature_versions`

```sql
-- Migration: 001_add_bot_id_multitenancy.sql
PRAGMA foreign_keys = OFF;
BEGIN TRANSACTION;

-- TIER 1: Example - user_preferences (full recreation)
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

-- (Repeat pattern for other Tier 1 tables...)

-- TIER 2: conflict_detection first, then mediation_history
-- (Similar recreation pattern with FK preservation)

-- TIER 3: Simple ALTER TABLE
ALTER TABLE conversation_history ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE usage_stats ADD COLUMN bot_id TEXT NOT NULL DEFAULT 'default';
-- (Continue for remaining Tier 3 tables...)

-- Recreate indexes with bot_id
CREATE INDEX idx_conversation ON conversation_history(bot_id, user_id, channel_id, timestamp);

PRAGMA foreign_key_check;
COMMIT;
PRAGMA foreign_keys = ON;
VACUUM;
ANALYZE;
```

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

Affected methods (~80 total, grouped by feature):

**User Management:**
- `get_user_persona`, `get_user_persona_with_guild`, `set_user_persona`
- `set_user_preference`, `get_user_preference`

**Conversation History:**
- `store_message`, `get_conversation_history`, `clear_conversation_history`
- `cleanup_old_messages`, `get_recent_channel_messages`, `get_recent_channel_messages_since`

**Guild & Channel Settings:**
- `get_guild_setting`, `set_guild_setting`, `get_guild_feature_flags`
- `get_channel_verbosity`, `set_channel_verbosity`, `get_channel_settings`, `set_channel_conflict_enabled`

**Reminders & Bookmarks:**
- `add_reminder`, `get_pending_reminders`, `complete_reminder`, `get_user_reminders`, `delete_reminder`
- `add_bookmark`, `get_user_bookmarks`, `delete_bookmark`

**Custom Commands:**
- `add_custom_command`, `get_custom_command`, `delete_custom_command`

**Feature Flags:**
- `set_feature_flag`, `is_feature_enabled`, `record_feature_toggle`

**Conflict Detection:**
- `record_conflict_detection`, `mark_conflict_resolved`, `mark_mediation_triggered`
- `get_channel_active_conflict`, `record_mediation`, `get_last_mediation_timestamp`
- `update_user_interaction_pattern`

**Analytics & Metrics:**
- `log_usage`, `increment_daily_stat`, `add_performance_metric`, `store_system_metric`

**Sessions & Metadata:**
- `start_session`, `update_session_activity`, `end_session`
- `store_message_metadata`, `update_message_metadata_reactions`, `mark_message_deleted`, `mark_message_edited`

#### 1.3 Migration Strategy

**Decision**: Assign existing data to default bot (preserves data)
- Set `bot_id = 'default'` for all existing records
- New bots use Discord `application_id` as their `bot_id`
- Supports global data sharing with per-bot overrides

#### 1.4 Deliverables

- [x] SQL migration script: `migrations/001_add_bot_id.sql` ✅ (Completed 2025-01)
- [x] Updated database schema in `database.rs` ✅ (287 occurrences of bot_id)
- [x] All ~80 database methods accept `bot_id` parameter ✅ (~55 methods updated)
- [x] Integration tests for multi-bot data isolation ✅ (in `database.rs` tests)
- [ ] Migration guide for production databases

**Estimated Time**: 6-8 days
**Actual**: Completed

---

### Phase 2: Configuration System Redesign

> **Note**: This phase should be implemented FIRST as it has no dependencies and establishes the `bot_id` structure needed for database methods.

#### 2.1 New Configuration Structures

```rust
// src/config.rs

#[derive(Debug, Clone, Deserialize)]
pub struct BotConfig {
    /// Discord application ID (used as bot_id in database)
    /// Retrieved from Discord API on startup, not configured manually
    #[serde(skip)]
    pub application_id: Option<String>,

    /// Friendly name for logging
    pub name: String,

    /// Discord bot token
    pub discord_token: String,

    /// Discord public key for interaction verification (HTTP mode)
    pub discord_public_key: Option<String>,

    /// Optional: Default persona for this bot
    pub default_persona: Option<String>,

    /// Optional: Dev mode guild ID for this bot
    pub discord_guild_id: Option<String>,

    /// Optional: Per-bot OpenAI model override
    pub openai_model: Option<String>,

    /// Optional: Per-bot conflict mediation settings
    pub conflict_mediation_enabled: Option<bool>,
    pub conflict_sensitivity: Option<String>,
    pub mediation_cooldown_minutes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MultiConfig {
    /// List of bot configurations
    pub bots: Vec<BotConfig>,

    /// Shared OpenAI API key
    pub openai_api_key: String,

    /// Shared database path
    pub database_path: String,

    /// Logging configuration
    pub log_level: String,

    /// Default OpenAI model (can be overridden per-bot)
    pub openai_model: String,

    /// Default conflict settings (can be overridden per-bot)
    pub conflict_mediation_enabled: bool,
    pub conflict_sensitivity: String,
    pub mediation_cooldown_minutes: u64,
}

impl MultiConfig {
    /// Load from YAML/JSON file
    pub fn from_file(path: &str) -> Result<Self> {
        // Implementation
    }

    /// Load from environment variables (backward compatible)
    pub fn from_env_single_bot() -> Result<Self> {
        // Creates MultiConfig with single bot from DISCORD_SPACE_CADET
    }
}
```

#### 2.2 Configuration File Format

**config.yaml**:
```yaml
bots:
  - bot_id: "muppet"
    name: "Muppet Friend"
    discord_token: "${DISCORD_MUPPET_TOKEN}"
    discord_public_key: "${DISCORD_MUPPET_PUBLIC_KEY}"
    default_persona: "muppet"

  - bot_id: "chef"
    name: "Chef Bot"
    discord_token: "${DISCORD_CHEF_TOKEN}"
    discord_public_key: "${DISCORD_CHEF_PUBLIC_KEY}"
    default_persona: "chef"

  - bot_id: "teacher"
    name: "Teacher Bot"
    discord_token: "${DISCORD_TEACHER_TOKEN}"
    default_persona: "teacher"

openai_api_key: "${OPENAI_API_KEY}"
database_path: "./persona.db"
log_level: "info"
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

- [x] New config structures in `config.rs` (`BotConfig`, `MultiConfig`) ✅
- [x] YAML file parsing support (required for multi-bot) ✅
- [x] Environment variable interpolation in YAML ✅ (`${VAR:-default}` syntax)
- [x] Backward compatibility layer for single-bot env vars ✅ (`from_env_single_bot()`)
- [x] Example `config.yaml` file ✅ (`config.yaml.example`)
- [x] Configuration validation ✅ (`validate()` method)
- [x] Application ID fetching from Discord API on startup ✅ (`fetch_application_id()`)

**Estimated Time**: 2-3 days
**Actual**: Completed (config.rs v2.0.0)

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

```rust
use tokio::signal;

// In main()
tokio::select! {
    _ = signal::ctrl_c() => {
        info!("Received Ctrl+C, shutting down all bots...");
        // Cancel all bot tasks
    }
    results = futures::future::join_all(handles) => {
        // Handle normal completion
    }
}
```

#### 3.4 Hidden Blockers (Background Tasks)

The following background tasks currently assume single-bot operation and need refactoring:

**1. ReminderScheduler** ([src/reminder_scheduler.rs](../src/reminder_scheduler.rs))
- Currently spawned once in `main()`, assumes single bot
- Queries `get_pending_reminders()` without `bot_id`
- Calls `get_user_persona(user_id)` without `bot_id`
- **Solution**: Spawn one ReminderScheduler per bot, pass `bot_id` to constructor

```rust
// Per-bot reminder scheduler
for bot_config in config.bots {
    let scheduler = ReminderScheduler::new(
        bot_config.application_id.clone(),  // NEW
        database.clone(),
        config.openai_model.clone(),
    );
    tokio::spawn(scheduler.run(http.clone()));
}
```

**2. StartupNotifier** ([src/startup_notification.rs](../src/startup_notification.rs))
- Uses static `FIRST_READY` flag - won't work with multiple bots in same process
- Reads from `bot_settings` table which has no `bot_id`
- **Solution**: Per-bot notifier instance, or shared notifier with bot tracking

```rust
// Change static flag to per-bot tracking
static NOTIFIED_BOTS: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
```

**3. Metrics Collection Loop**
- Single instance spawned in `main()`
- No `bot_id` in `performance_metrics` table
- **Decision needed**: Per-bot metrics or global metrics?
- **Recommendation**: Keep global for now, add `bot_id` to track source

#### 3.5 Deliverables

- [x] Refactored `bin/bot.rs` with multi-client spawning ✅ (`tokio::spawn` per bot)
- [x] Shared resource management (Arc-wrapped) ✅ (`Arc<Database>`, `Arc<PersonaManager>`)
- [x] Per-bot error handling and logging ✅ (`[bot_name]` log prefixes)
- [x] Graceful shutdown mechanism ✅ (Ctrl+C handler with `SHUTDOWN_REQUESTED`)
- [x] Bot restart logic for transient failures ✅ (5 retries with exponential backoff)
- [x] Structured logging with bot_id context ✅ (All logs prefixed with bot name)
- [x] Per-bot ReminderScheduler instances ✅ (`ReminderScheduler::with_bot_id()`)
- [x] Fix StartupNotifier static flag for multi-bot ✅ (`NOTIFIED_BOTS` HashSet)
- [x] Metrics collection with bot_id tracking ✅ (`metrics_collection_loop_with_bot_id()`)

**Estimated Time**: 3-4 days
**Actual**: Completed (bot.rs v0.6.0)

---

### Phase 4: Context Propagation

#### 4.1 Update CommandHandler

**Current** (actual structure):
```rust
pub struct CommandHandler {
    persona_manager: PersonaManager,
    database: Database,
    rate_limiter: RateLimiter,
    audio_transcriber: AudioTranscriber,
    image_generator: ImageGenerator,
    openai_model: String,
    conflict_detector: ConflictDetector,
    conflict_mediator: ConflictMediator,
    conflict_enabled: bool,
    conflict_sensitivity_threshold: f32,
    start_time: std::time::Instant,
}
```

**New**:
```rust
pub struct CommandHandler {
    bot_id: String,  // NEW: Bot identity (Discord application_id)
    persona_manager: PersonaManager,
    database: Database,
    rate_limiter: RateLimiter,
    audio_transcriber: AudioTranscriber,
    image_generator: ImageGenerator,
    openai_model: String,
    conflict_detector: ConflictDetector,
    conflict_mediator: ConflictMediator,
    conflict_enabled: bool,
    conflict_sensitivity_threshold: f32,
    start_time: std::time::Instant,
}

impl CommandHandler {
    pub fn new(
        bot_id: String,  // NEW parameter
        persona_manager: Arc<PersonaManager>,
        database: Arc<Database>,
        openai_model: String,
        conflict_config: ConflictConfig,
    ) -> Self {
        Self {
            bot_id,
            persona_manager: (*persona_manager).clone(),
            database: (*database).clone(),
            rate_limiter: RateLimiter::new(),
            audio_transcriber: AudioTranscriber::new(),
            image_generator: ImageGenerator::new(),
            openai_model,
            conflict_detector: ConflictDetector::new(conflict_config.sensitivity),
            conflict_mediator: ConflictMediator::new(conflict_config.cooldown),
            conflict_enabled: conflict_config.enabled,
            conflict_sensitivity_threshold: conflict_config.sensitivity,
            start_time: std::time::Instant::now(),
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

**Current**:
```rust
// Rate limiter keyed by user_id only
pub struct RateLimiter {
    last_interaction: HashMap<String, Instant>,
}
```

**New**:
```rust
// Rate limiter keyed by (bot_id, user_id)
pub struct RateLimiter {
    last_interaction: HashMap<(String, String), Instant>,
}

impl RateLimiter {
    pub fn check_rate_limit(&mut self, bot_id: &str, user_id: &str) -> bool {
        let key = (bot_id.to_string(), user_id.to_string());
        // ... rest of logic
    }
}
```

#### 4.4 Update All Database Calls

Systematically update every database call to include `bot_id`:

```rust
// Pattern: Add &self.bot_id as first parameter
self.database.method_name(&self.bot_id, /* other params */).await?;
```

#### 4.5 Additional Components Needing bot_id

Beyond CommandHandler, these components also need bot_id context:

- **ConflictDetector** - Conflict tracking is per-bot
- **ConflictMediator** - Mediation cooldowns are per-bot
- **MessageComponentHandler** - Persona selection needs bot context
- **ImageGenerator** - Stateless, no changes needed

#### 4.6 Deliverables

- [x] Add `bot_id` field to `CommandHandler` ✅ (`command_handler.rs:25`)
- [x] Update all command handler methods (~50 methods) ✅ (73 `self.bot_id` usages)
- [x] Update rate limiter to use composite keys ✅ (`rate_limiter.rs` v1.1.0)
- [x] Update all database calls with bot_id (~80 calls) ✅ (All calls updated)
- [x] Pass bot_id to ConflictDetector/ConflictMediator ✅ (Stateless - per-bot instances)
- [x] Update MessageComponentHandler with bot_id ✅ (`with_bot_id()` constructor)
- [x] Add integration tests for context isolation ✅ (`database.rs` tests)
- [x] Verify no conversation bleeding between bots ✅ (Tests confirm isolation)

**Estimated Time**: 4-5 days
**Actual**: Completed

---

### Phase 5: HTTP Mode Multi-Bot Support (Deferred)

> **Status**: This phase is **deferred** - focus on Gateway mode first. HTTP multi-bot support is optional/future enhancement.

If supporting HTTP interaction mode for multiple bots:

#### 5.1 Update HTTP Server

**Current** ([src/bin/http_bot.rs](../src/bin/http_bot.rs)):
```rust
// Single bot, single public key
let config = Config::from_env()?;
verify_discord_signature(&signature, &timestamp, &body, &public_key)?;
```

**New**:
```rust
// Map application_id -> (bot_id, public_key)
struct BotRegistry {
    bots: HashMap<String, (String, String)>,  // app_id -> (bot_id, public_key)
}

async fn handle_interaction(
    body: String,
    signature: String,
    timestamp: String,
    registry: Arc<BotRegistry>,
) -> Result<Response> {
    // Parse interaction to get application_id
    let interaction: Interaction = serde_json::from_str(&body)?;
    let app_id = &interaction.application_id;

    // Look up which bot this is for
    let (bot_id, public_key) = registry.bots.get(app_id)
        .ok_or("Unknown application")?;

    // Verify signature with correct public key
    verify_discord_signature(&signature, &timestamp, &body, public_key)?;

    // Route to correct bot handler with bot_id context
    handle_command(bot_id, interaction).await
}
```

#### 5.2 Single Server, Multiple Bots

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let config = MultiConfig::from_file("config.yaml")?;

    // Build registry
    let mut registry = BotRegistry { bots: HashMap::new() };
    for bot in &config.bots {
        if let Some(public_key) = &bot.discord_public_key {
            // Need to get application_id from Discord API or config
            let app_id = get_application_id(&bot.discord_token).await?;
            registry.bots.insert(app_id, (bot.bot_id.clone(), public_key.clone()));
        }
    }

    let registry = Arc::new(registry);

    // Single HTTP server on port 6666
    let app = Router::new()
        .route("/interactions", post(handle_interaction))
        .layer(Extension(registry));

    // Start server
    axum::Server::bind(&"0.0.0.0:6666".parse()?)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
```

#### 5.3 Deliverables

- [ ] Bot registry structure
- [ ] Application ID lookup/configuration
- [ ] Multi-bot signature verification
- [ ] Routing by application_id
- [ ] Update http_bot.rs entry point
- [ ] Integration tests for multiple bots

**Estimated Time**: 1-2 days

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
    discord_token: "${DISCORD_SPACE_CADET}"
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

### Health Checks

```rust
// Optional: Add health check endpoint
async fn health_check(registry: Arc<BotRegistry>) -> Json<HealthStatus> {
    let status = registry.bots.iter().map(|(id, bot)| {
        (id.clone(), bot.is_connected())
    }).collect();

    Json(HealthStatus { bots: status })
}
```

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
   export DISCORD_SPACE_CADET=<token>
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

## Design Decisions (Resolved)

The following decisions have been made for this implementation:

### 1. Data Sharing Philosophy
**Decision**: **Global with per-bot overrides**
- User preferences and settings default to global (shared across bots)
- Can be overridden per-bot when needed
- Conversation history is per-bot (no sharing)

### 2. Bot Identification
**Decision**: **Discord application_id**
- Use the Discord application ID as the `bot_id` in database records
- Provides consistency with Discord's identity system
- Retrieved from Discord API on startup, not configured manually

### 3. Configuration Management
**Decision**: **YAML file required for multi-bot**
- Multi-bot configuration requires `config.yaml`
- Single-bot can still use environment variables (backward compatible)
- No remote config support initially

### 4. Deployment Model
**Decision**: **Either/both supported**
- Single process by default (tokio::spawn for each bot)
- Architecture supports running bots as separate processes if desired
- Docker: single container recommended, but per-bot containers possible

### 5. HTTP Mode Priority
**Decision**: **Deferred**
- Focus on Gateway mode first (Phases 1-4)
- HTTP multi-bot support (Phase 5) is optional/future enhancement
- Gateway-only for initial release

---

## Appendix A: File Change Summary

### Major Changes Required

| File | Current Lines | Changes | Estimated New Lines | Complexity |
|------|---------------|---------|---------------------|------------|
| `src/config.rs` | 73 | Add MultiConfig, BotConfig, YAML parsing | ~200 | High |
| `src/database.rs` | 1,777 | Add bot_id to all 80+ methods | ~2,200 | High |
| `src/bin/bot.rs` | 377 | Multi-client spawning, per-bot tasks | ~500 | Medium |
| `src/command_handler.rs` | 3,095 | Add bot_id field, propagate to all calls | ~3,200 | Medium |
| `src/rate_limiter.rs` | 109 | Composite keys (bot_id, user_id) | ~130 | Low |
| `src/reminder_scheduler.rs` | 199 | Add bot_id, per-bot instances | ~250 | Medium |
| `src/startup_notification.rs` | 212 | Fix static flag for multi-bot | ~250 | Medium |
| `src/bin/http_bot.rs` | 60 | Bot registry (Phase 5, deferred) | ~150 | Medium |

### New Files Needed

- `migrations/001_add_bot_id.sql` - Database migration script (~200 lines)
- `config.yaml.example` - Example multi-bot configuration
- `docs/multi-bot-setup.md` - User-facing setup guide
- `tests/integration/multi_bot_tests.rs` - Integration tests

### No Changes Required

- `src/personas.rs` - Already multi-bot compatible ✅
- `src/audio.rs` - Stateless ✅
- `src/message_components.rs` - Minor context updates only

---

## Appendix B: Database Schema (After Migration)

All 21 tables with `bot_id` column (where applicable):

### Core User Tables

```sql
-- User preferences (Tier 1 - recreation required)
CREATE TABLE user_preferences (
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    default_persona TEXT DEFAULT 'obi',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (bot_id, user_id)
);

-- Extended user preferences (Tier 1 - recreation required)
CREATE TABLE extended_user_preferences (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    preference_key TEXT NOT NULL,
    preference_value TEXT,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(bot_id, user_id, preference_key)
);

-- Conversation history (Tier 3 - ALTER TABLE)
CREATE TABLE conversation_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    persona TEXT,
    timestamp INTEGER NOT NULL
);
CREATE INDEX idx_conversation ON conversation_history(bot_id, user_id, channel_id, timestamp);
```

### Guild & Channel Tables

```sql
-- Guild settings (Tier 1 - recreation required)
CREATE TABLE guild_settings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    guild_id TEXT NOT NULL,
    setting_key TEXT NOT NULL,
    setting_value TEXT,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(bot_id, guild_id, setting_key)
);

-- Channel settings (Tier 1 - recreation required)
CREATE TABLE channel_settings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    guild_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    verbosity TEXT DEFAULT 'concise',
    conflict_enabled BOOLEAN DEFAULT 1,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(bot_id, guild_id, channel_id)
);
```

### Feature & Command Tables

```sql
-- Custom commands (Tier 1 - recreation required)
CREATE TABLE custom_commands (
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

-- Feature flags (Tier 1 - recreation required)
CREATE TABLE feature_flags (
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

-- Feature versions (Tier 3 - ALTER TABLE)
CREATE TABLE feature_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    feature_name TEXT NOT NULL,
    version TEXT NOT NULL,
    guild_id TEXT,
    toggled_by TEXT,
    enabled BOOLEAN,
    changed_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

### Interaction Tables

```sql
-- Reminders (Tier 3 - ALTER TABLE)
CREATE TABLE reminders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    reminder_text TEXT NOT NULL,
    remind_at TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed BOOLEAN DEFAULT 0,
    completed_at DATETIME
);
CREATE INDEX idx_reminder_time ON reminders(bot_id, remind_at, completed);

-- User bookmarks (Tier 3 - ALTER TABLE)
CREATE TABLE user_bookmarks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    bookmark_name TEXT,
    bookmark_note TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Message metadata (Tier 3 - ALTER TABLE)
CREATE TABLE message_metadata (
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
);

-- Interaction sessions (Tier 3 - ALTER TABLE)
CREATE TABLE interaction_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    guild_id TEXT,
    session_start DATETIME DEFAULT CURRENT_TIMESTAMP,
    session_end DATETIME,
    message_count INTEGER DEFAULT 0,
    last_activity DATETIME
);
```

### Conflict Detection Tables

```sql
-- Conflict detection (Tier 2 - parent table, migrate first)
CREATE TABLE conflict_detection (
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

-- Mediation history (Tier 2 - has FK to conflict_detection)
CREATE TABLE mediation_history (
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

-- User interaction patterns (Tier 1 - recreation required)
CREATE TABLE user_interaction_patterns (
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
```

### Analytics Tables

```sql
-- Usage stats (Tier 3 - ALTER TABLE)
CREATE TABLE usage_stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    command TEXT NOT NULL,
    persona TEXT,
    timestamp INTEGER NOT NULL
);
CREATE INDEX idx_usage ON usage_stats(bot_id, timestamp);

-- Daily analytics (Tier 1 - recreation required)
CREATE TABLE daily_analytics (
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

-- Performance metrics (Tier 3 - ALTER TABLE)
CREATE TABLE performance_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_id TEXT NOT NULL DEFAULT 'default',
    metric_type TEXT NOT NULL,
    value REAL NOT NULL,
    unit TEXT,
    metadata TEXT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Error logs (Tier 3 - ALTER TABLE)
CREATE TABLE error_logs (
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
);
```

### Global Tables (No bot_id)

```sql
-- Bot settings (remains global, shared across all bots)
CREATE TABLE bot_settings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    setting_key TEXT NOT NULL UNIQUE,
    setting_value TEXT,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
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
    discord_public_key: "${DISCORD_MUPPET_PUBLIC_KEY}"
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
- Careful database migration with rollback plan (tiered approach for SQLite)
- Comprehensive testing at each phase
- Backward compatibility during transition
- Clear separation of shared vs. per-bot resources
- Discord application_id as canonical bot identifier

**Estimated Total Effort**: ~3-4 weeks for core implementation (Phases 1-4)

| Phase | Duration | Dependencies | Status |
|-------|----------|--------------|--------|
| Phase 2: Config | 2-3 days | None (implement first) | ✅ Complete |
| Phase 1: Database | 6-8 days | Phase 2 | ✅ Complete |
| Phase 3: Gateway | 3-4 days | Phases 1, 2 | ✅ Complete |
| Phase 4: Context | 4-5 days | Phases 1, 2, 3 | ✅ Complete |
| Phase 5: HTTP (deferred) | 1-2 days | All above | ⏸️ Deferred |

**Implementation Status**: All core phases (1-4) completed as of 2025-01.

**Questions?** All design decisions have been resolved - see the "Design Decisions" section above.
