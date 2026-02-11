# Feature Organization & Versioning System

> **Status**: All Phases Complete
> **Version**: 0.3.1
> **Last Updated**: 2025-11-24

This document outlines the reorganization of bot features, command systems, and scripts for better maintainability and runtime control.

---

## Table of Contents

1. [Feature Versioning System](#feature-versioning-system)
2. [Command Organization](#command-organization)
3. [Scripts Organization](#scripts-organization)
4. [Runtime Feature Toggles](#runtime-feature-toggles)
5. [Implementation Checklist](#implementation-checklist)

---

## Feature Versioning System

### Overview

Each feature module will be versioned independently using semantic versioning, allowing:
- Clear tracking of feature changes
- Admin visibility into what's running
- Easier debugging and rollback decisions

### Feature Header Comments

Every feature module must include a header comment block:

```rust
//! # Feature: Reminders
//!
//! Scheduled reminder system with persona-aware delivery.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: true
//! - **Admin Only**: false
//!
//! ## Changelog
//! - 1.0.0: Initial release with basic reminders
```

### Feature Registry (`src/features.rs`)

Central registry for all bot features:

```rust
use std::collections::HashMap;

/// Describes a versioned bot feature
pub struct Feature {
    /// Feature identifier (snake_case)
    pub id: &'static str,
    /// Human-readable name
    pub name: &'static str,
    /// Current semantic version
    pub version: &'static str,
    /// Bot version when feature was added
    pub since: &'static str,
    /// Can be toggled at runtime by admins
    pub toggleable: bool,
    /// Brief description
    pub description: &'static str,
}

/// Returns all registered features
pub fn get_features() -> Vec<Feature> {
    vec![
        Feature {
            id: "personas",
            name: "Persona System",
            version: "1.0.0",
            since: "0.1.0",
            toggleable: false,
            description: "Multi-personality AI responses",
        },
        Feature {
            id: "reminders",
            name: "Reminders",
            version: "1.0.0",
            since: "0.1.0",
            toggleable: true,
            description: "Scheduled reminder system with persona delivery",
        },
        Feature {
            id: "conflict_detection",
            name: "Conflict Detection",
            version: "1.0.0",
            since: "0.1.0",
            toggleable: true,
            description: "Detects heated discussions and offers mediation",
        },
        Feature {
            id: "image_generation",
            name: "Image Generation",
            version: "1.0.0",
            since: "0.2.0",
            toggleable: true,
            description: "DALL-E powered image creation",
        },
        Feature {
            id: "audio_transcription",
            name: "Audio Transcription",
            version: "1.0.0",
            since: "0.1.0",
            toggleable: true,
            description: "Converts audio attachments to text",
        },
        Feature {
            id: "introspection",
            name: "Self-Introspection",
            version: "1.0.0",
            since: "0.1.0",
            toggleable: false,
            description: "Bot can explain its own internals",
        },
        Feature {
            id: "verbosity_control",
            name: "Verbosity Control",
            version: "1.0.0",
            since: "0.1.0",
            toggleable: false,
            description: "Per-channel response length settings",
        },
        Feature {
            id: "rate_limiting",
            name: "Rate Limiting",
            version: "1.0.0",
            since: "0.1.0",
            toggleable: false,
            description: "Prevents spam (10 req/60s per user)",
        },
    ]
}

/// Get a feature by ID
pub fn get_feature(id: &str) -> Option<&'static Feature> {
    get_features().into_iter().find(|f| f.id == id)
}
```

### Version Bumping Rules

| Change Type | Version Bump | Example |
|-------------|--------------|---------|
| Bug fix | Patch (x.x.+1) | Fix reminder timezone handling |
| New option/setting | Minor (x.+1.0) | Add reminder snooze option |
| Breaking change | Major (+1.0.0) | Change reminder time format |

---

## Command Organization

### Current Problem

- All slash commands defined in one large `slash_commands.rs` file
- Command handling logic mixed in `commands.rs` (2400+ lines)
- No separation between user-facing and admin commands
- No support for quick text-based commands

### Proposed Structure

```
src/
├── commands/
│   ├── mod.rs              # Re-exports, CommandHandler struct
│   ├── handler.rs          # Core message/interaction routing
│   │
│   ├── slash/              # Discord slash commands (/)
│   │   ├── mod.rs          # Slash command registration
│   │   ├── ask.rs          # /ask (unified AI chat)
│   │   ├── persona.rs      # /personas, /set_persona
│   │   ├── utility.rs      # /ping, /help, /forget
│   │   ├── remind.rs       # /remind, /reminders
│   │   ├── admin.rs        # /introspect, /settings, /set_*, /admin_role
│   │   └── imagine.rs      # /imagine (image generation)
│   │
│   └── bang/               # Text-based commands (!)
│       ├── mod.rs          # Bang command parser and registration
│       ├── info.rs         # !help, !status, !version, !uptime
│       ├── quick.rs        # !ping, !features
│       └── admin.rs        # !toggle, !reload, !sync
```

### Command Prefixes

| Prefix | Type | Audience | Registration |
|--------|------|----------|--------------|
| `/` | Slash commands | All users | Discord API |
| `!` | Bang commands | All users | Text parsing |

### Slash Commands (`/`)

Native Discord commands with autocomplete, validation, and discoverability.

**User Commands:**
- `/ping` - Test bot responsiveness
- `/help` - Show all available commands (stays as slash for discoverability)
- `/personas` - List available personas
- `/set_persona <persona>` - Set your default persona
- `/ask @persona <message>` - Chat with any persona (supports modifiers and history)
- `/imagine <prompt> [size] [style]` - Generate an image
- `/forget` - Clear conversation history
- `/remind <time> <message>` - Set a reminder
- `/reminders [action] [id]` - List or cancel reminders

**Admin Commands** (require MANAGE_GUILD):
- `/introspect <component>` - Explain bot internals
- `/settings` - View current guild configuration
- `/set_channel_verbosity <level> [channel]` - Set response verbosity
- `/set_guild_setting <setting> <value>` - Configure guild defaults
- `/admin_role <role>` - Grant admin access to a role

### Bang Commands (`!`)

Quick text-based commands for power users and admin operations.

**Info Commands:**
- `!help` - Quick command reference (text output)
- `!status` - Bot status and uptime
- `!version` - Show bot and feature versions
- `!uptime` - How long bot has been running

**Quick Commands:**
- `!ping` - Fast ping (no embed, just text)
- `!features` - List all features with versions and toggle status

**Admin Commands** (require MANAGE_GUILD or admin role):
- `!toggle <feature>` - Enable/disable a feature for this guild
- `!reload` - Reload guild settings from database
- `!sync` - Force sync slash commands to this guild

### Command Handler Refactor

```rust
// src/commands/mod.rs
pub mod handler;
pub mod slash;
pub mod bang;

pub use handler::CommandHandler;

// src/commands/handler.rs
impl CommandHandler {
    pub async fn handle_message(&self, ctx: &Context, msg: &Message) -> Result<()> {
        // Rate limit check
        if self.rate_limiter.check_limited(msg.author.id).await {
            return Ok(());
        }

        // Route based on prefix
        if let Some(cmd) = msg.content.strip_prefix('!') {
            return self.handle_bang_command(ctx, msg, cmd).await;
        }

        // Continue with existing logic for DMs, mentions, etc.
        // ...
    }

    async fn handle_bang_command(
        &self,
        ctx: &Context,
        msg: &Message,
        input: &str,
    ) -> Result<()> {
        let (cmd, args) = parse_bang_command(input);
        bang::dispatch(self, ctx, msg, cmd, args).await
    }
}
```

### Migration Strategy

1. Create `src/commands/` directory structure
2. Move slash command definitions to `src/commands/slash/mod.rs`
3. Split command handlers by category into separate files
4. Create bang command parser and handlers
5. Update `src/commands/mod.rs` to re-export everything
6. Update `src/lib.rs` to use new module path
7. Deprecate old `src/slash_commands.rs` and `src/commands.rs`

---

## Scripts Organization

### Current Problem

- 8 shell scripts in root directory cluttering workspace
- No organization by purpose
- No standardized way to run scripts with options

### Proposed Structure

```
scripts/
├── Makefile                    # Scoped make targets
├── README.md                   # Script documentation
│
├── commands/                   # Discord command management
│   ├── check.sh               # Check registered commands
│   └── cleanup.sh             # Remove duplicate registrations
│
├── service/                    # Systemd service management
│   ├── reload.sh              # Reload service
│   └── status.sh              # Check service status
│
├── tunnel/                     # ngrok tunnel management
│   ├── setup.sh               # Configure ngrok
│   └── start-http.sh          # Start bot with HTTP tunnel
│
└── test/                       # Testing utilities
    ├── env.sh                 # Validate environment config
    └── openai.sh              # Test OpenAI connectivity
```

### File Migration Map

| Current Location | New Location |
|------------------|--------------|
| `check-commands.sh` | `scripts/commands/check.sh` |
| `cleanup-commands.sh` | `scripts/commands/cleanup.sh` |
| `reload-service.sh` | `scripts/service/reload.sh` |
| `setup-ngrok.sh` | `scripts/tunnel/setup.sh` |
| `start-http-with-tunnel.sh` | `scripts/tunnel/start-http.sh` |
| `start-with-tunnel.sh` | `scripts/tunnel/start-gateway.sh` |
| `test_env_loading.sh` | `scripts/test/env.sh` |
| `test_openai_timeout.sh` | `scripts/test/openai.sh` |

### Scripts Makefile (`scripts/Makefile`)

```makefile
.PHONY: help commands/check commands/cleanup service/reload service/status \
        tunnel/setup tunnel/start-http tunnel/start-gateway test/env test/openai

# Default target
help:
	@echo "Available targets:"
	@echo "  commands/check    - Check registered Discord commands"
	@echo "  commands/cleanup  - Remove duplicate command registrations"
	@echo "  service/reload    - Reload systemd service"
	@echo "  service/status    - Check service status"
	@echo "  tunnel/setup      - Configure ngrok tunnel"
	@echo "  tunnel/start-http - Start bot with HTTP tunnel"
	@echo "  tunnel/start-gateway - Start bot with gateway tunnel"
	@echo "  test/env          - Validate environment configuration"
	@echo "  test/openai       - Test OpenAI API connectivity"

# Command management
commands/check:
	@./commands/check.sh $(ARGS)

commands/cleanup:
	@./commands/cleanup.sh $(ARGS)

# Service management
service/reload:
	@./service/reload.sh

service/status:
	@./service/status.sh

# Tunnel management
tunnel/setup:
	@./tunnel/setup.sh

tunnel/start-http:
	@./tunnel/start-http.sh

tunnel/start-gateway:
	@./tunnel/start-gateway.sh

# Testing
test/env:
	@./test/env.sh

test/openai:
	@./test/openai.sh $(TIMEOUT)

# Run all tests
test/all: test/env test/openai
```

### Root Makefile Updates

Add delegation to scripts Makefile:

```makefile
# Script delegation
scripts/%:
	@$(MAKE) -C scripts $*

# Aliases for common operations
check-commands: scripts/commands/check
cleanup-commands: scripts/commands/cleanup
```

### Usage Examples

```bash
# From project root
make scripts/commands/check
make scripts/service/reload
make scripts/test/all

# From scripts directory
cd scripts
make commands/check
make service/status ARGS="--verbose"
make test/openai TIMEOUT=30
```

---

## Runtime Feature Toggles

### Overview

Administrators should be able to enable/disable features without restarting the bot.

### Database Schema

Extend existing `feature_flags` table:

```sql
CREATE TABLE IF NOT EXISTS feature_flags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    feature_id TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    toggled_by TEXT,
    toggled_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(guild_id, feature_id)
);

CREATE TABLE IF NOT EXISTS feature_versions (
    feature_id TEXT PRIMARY KEY,
    version TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

### Admin Commands (Slash Commands)

**`/features`** - List all features with status (requires MANAGE_GUILD):

```
📦 Bot Features (v0.3.0)

Feature              Version  Status  Toggleable
─────────────────────────────────────────────────
Persona System       1.0.0    ✅ ON   No
Reminders            1.0.0    ✅ ON   Yes
Conflict Detection   1.0.0    ❌ OFF  Yes
Image Generation     1.0.0    ✅ ON   Yes
Audio Transcription  1.0.0    ✅ ON   Yes
Self-Introspection   1.0.0    ✅ ON   No
Verbosity Control    1.0.0    ✅ ON   No
Rate Limiting        1.0.0    ✅ ON   No

Use /toggle <feature> to enable/disable toggleable features.
```

**`/toggle <feature>`** - Toggle a feature (requires MANAGE_GUILD):

```
/toggle conflict_detection

Conflict Detection has been ✅ enabled.

Feature: conflict_detection v1.0.0
```

```
/toggle personas

❌ Persona System cannot be toggled. It's a core feature.
```

### Utility Commands (Slash Commands)

**`/status`** - Show bot status and uptime
**`/version`** - Show bot and all feature versions
**`/uptime`** - Show how long the bot has been running

### Feature Check Integration

```rust
impl CommandHandler {
    /// Check if a feature is enabled for a guild
    pub async fn is_feature_enabled(&self, guild_id: GuildId, feature_id: &str) -> bool {
        // Check database for guild-specific override
        if let Some(enabled) = self.db.get_feature_flag(guild_id, feature_id).await {
            return enabled;
        }

        // Fall back to default (enabled)
        true
    }
}

// Usage in command handlers
async fn handle_remind(&self, ctx: &Context, msg: &Message) -> Result<()> {
    if !self.is_feature_enabled(msg.guild_id.unwrap(), "reminders").await {
        msg.reply(ctx, "❌ Reminders are disabled on this server.").await?;
        return Ok(());
    }
    // ... handle reminder
}
```

---

## Implementation Checklist

### Phase 1: Feature Registry
- [x] Create `src/features.rs` with Feature struct and registry
- [x] Add feature header comments to existing modules:
  - [x] `src/personas.rs`
  - [x] `src/reminder_scheduler.rs`
  - [x] `src/conflict_detector.rs`
  - [x] `src/conflict_mediator.rs`
  - [x] `src/image_gen.rs`
  - [x] `src/audio.rs`
  - [x] `src/introspection.rs`
  - [x] `src/rate_limiter.rs`
- [x] Export features module in `src/lib.rs`

### Phase 2: Command Reorganization
- [x] Create `src/commands/` directory structure
- [x] Create `src/commands/mod.rs` with exports
- [x] Create `src/command_handler.rs` with CommandHandler
- [x] Migrate slash commands:
  - [x] `src/commands/slash/mod.rs` (registration)
  - [x] `src/commands/slash/chat.rs`
  - [x] `src/commands/slash/persona.rs`
  - [x] `src/commands/slash/utility.rs`
  - [x] `src/commands/slash/remind.rs`
  - [x] `src/commands/slash/admin.rs`
  - [x] `src/commands/slash/imagine.rs`
- [x] Create bang command system:
  - [x] `src/commands/bang/mod.rs` (parser)
  - [x] `src/commands/bang/info.rs`
  - [x] `src/commands/bang/quick.rs`
  - [x] `src/commands/bang/admin.rs`
- [x] Update `src/lib.rs` module exports
- [x] Remove old `src/slash_commands.rs`

### Phase 3: Scripts Organization
- [x] Create `scripts/` directory structure
- [x] Move and rename scripts:
  - [x] `scripts/commands/check.sh`
  - [x] `scripts/commands/cleanup.sh`
  - [x] `scripts/service/reload.sh`
  - [x] `scripts/service/status.sh`
  - [x] `scripts/tunnel/setup.sh`
  - [x] `scripts/tunnel/start-http.sh`
  - [x] `scripts/tunnel/start-gateway.sh`
  - [x] `scripts/test/env.sh`
  - [x] `scripts/test/openai.sh`
- [x] Create `scripts/Makefile`
- [x] Create `scripts/README.md`
- [x] Update root `Makefile` with delegation
- [x] Remove old scripts from root

### Phase 4: Runtime Toggles
- [x] Add `feature_versions` table migration
- [x] Add database methods for feature flags (`is_feature_enabled`, `set_feature_flag`, `get_guild_feature_flags`, `record_feature_toggle`)
- [x] Implement `/features` command (converted from bang to slash)
- [x] Implement `/toggle` command (converted from bang to slash, admin-only)
- [x] Add feature checks to toggleable handlers (`audio_transcription`, `conflict_mediation`, `reminders`, `image_generation`)
- [x] Toggle persistence verified (SQLite database handles persistence automatically)

### Phase 5: Documentation
- [x] Update `CLAUDE.md` with feature maintenance rules
- [x] Update `README.md` with new command prefixes
- [x] Update `docs/makefile-reference.md` with script targets
- [x] Migration guide: Feature toggles available via `/features` and `/toggle` commands; all features default to enabled

---

## CLAUDE.md Additions

Add the following section to `CLAUDE.md`:

```markdown
## Feature Version Maintenance

When modifying any feature module, follow these rules:

### Feature Header Requirements
Every feature module (`src/*.rs` that implements a distinct feature) must have a header comment:
- Feature name and brief description
- Current semantic version
- Bot version when feature was introduced
- Whether the feature is toggleable at runtime
- Changelog of version changes

### Version Update Rules
- **Patch** (x.x.+1): Bug fixes, internal refactoring
- **Minor** (x.+1.0): New options, settings, or non-breaking enhancements
- **Major** (+1.0.0): Breaking changes, API changes, major behavior changes

### When Adding Features
1. Create the feature module with proper header comment
2. Register the feature in `src/features.rs`
3. Add feature to this documentation's feature list
4. Update `README.md` if user-facing

### When Modifying Features
1. Update the feature header version
2. Add changelog entry in the header
3. Update `src/features.rs` version
4. Include version in commit message
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 0.2.0 | 2025-01-22 | Initial planning document |
