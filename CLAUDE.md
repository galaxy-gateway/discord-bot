# Persona Discord Bot

AI-powered Discord bot with multiple personalities, conflict mediation, and a real-time TUI dashboard.

## Quick Reference

| Command | Description |
|---------|-------------|
| `make build` | Build debug binary |
| `make build-release` | Build release binary |
| `make run` | Run bot in development mode |
| `make tui` | Run TUI dashboard (requires running bot) |
| `make test` | Run tests |
| `make fmt` | Format code |
| `make lint` | Run clippy |
| `make start` | Start systemd service |
| `make stop` | Stop systemd service |
| `make logs-follow` | Follow service logs |

## Architecture Overview

```
src/
├── bin/
│   ├── bot.rs          # Bot binary entry point
│   └── tui.rs          # TUI binary entry point
├── core/
│   ├── mod.rs          # Core module exports
│   └── config.rs       # Configuration loading from env
├── features/           # Feature modules (versioned)
│   ├── analytics/      # Usage tracking, metrics, system info
│   ├── audio/          # Whisper transcription
│   ├── conflict/       # Detection and mediation
│   ├── image_gen/      # DALL-E integration
│   ├── introspection/  # Self-documentation
│   ├── personas/       # Multi-personality system
│   ├── plugins/        # CLI command plugins
│   ├── rate_limiting/  # Request throttling
│   ├── reminders/      # Scheduled reminders
│   ├── startup/        # Startup notifications
│   └── mod.rs          # Feature registry
├── commands/
│   ├── slash/          # Discord slash commands
│   └── mod.rs          # Command registration
├── ipc/                # Bot <-> TUI communication
│   ├── protocol.rs     # Message types (BotEvent, TuiCommand)
│   ├── server.rs       # Unix socket server (bot side)
│   └── client.rs       # Unix socket client (TUI side)
├── tui/                # Terminal UI (optional feature)
│   ├── app.rs          # Main TUI application
│   ├── event.rs        # Event handling
│   ├── state/          # Application state
│   └── ui/             # Screen renderers
├── lib.rs              # Library crate root
├── database.rs         # SQLite operations
├── command_handler.rs  # Slash command dispatcher
└── message_components.rs # Discord message formatting
```

### Key Dependencies

- **serenity** - Discord API client
- **tokio** - Async runtime
- **openai** - OpenAI API client
- **sqlite** - Database
- **ratatui/crossterm** - TUI (optional)
- **serde/serde_json** - Serialization
- **anyhow** - Error handling
- **log/env_logger** - Logging

## Coding Patterns

### Error Handling

Use `anyhow::Result` for fallible functions:

```rust
use anyhow::{Result, anyhow};

pub fn do_thing() -> Result<String> {
    let value = some_call()
        .map_err(|_| anyhow!("Failed to do thing"))?;
    Ok(value)
}
```

### Async Patterns

- Use `tokio::spawn` for background tasks
- Use `Arc<RwLock<T>>` for shared state
- Use channels (`tokio::sync::mpsc`) for message passing

```rust
let shared = Arc::new(RwLock::new(State::new()));
let shared_clone = Arc::clone(&shared);

tokio::spawn(async move {
    let mut state = shared_clone.write().await;
    state.update();
});
```

### Configuration

Use `from_env()` pattern for loading config:

```rust
impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Config {
            token: env::var("TOKEN")
                .map_err(|_| anyhow!("TOKEN not set"))?,
            optional: env::var("OPTIONAL").unwrap_or_else(|_| "default".to_string()),
        })
    }
}
```

### Logging

Use the `log` crate with descriptive prefixes:

```rust
use log::{info, warn, error, debug};

info!("Bot started successfully");
warn!("Rate limit approaching");
error!("Failed to connect: {}", err);
debug!("Processing message: {:?}", msg);
```

### Serialization

Use serde with tagged enums for IPC:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    MessageCreate { channel_id: u64, content: String },
    Ready { guilds: Vec<GuildInfo> },
}
```

### Testing

Use `#[tokio::test]` for async tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_function() {
        assert_eq!(add(2, 2), 4);
    }

    #[tokio::test]
    async fn test_async_function() {
        let result = fetch_data().await;
        assert!(result.is_ok());
    }
}
```

### Feature Flags

Use `#[cfg(feature = "...")]` for optional features:

```rust
#[cfg(feature = "tui")]
pub mod tui;
```

## Common Development Tasks

### Adding a New Slash Command

1. Create command file in `src/commands/slash/` (e.g., `mycommand.rs`)
2. Add `pub fn create_commands() -> Vec<CreateApplicationCommand>`
3. Add `pub mod mycommand;` to `src/commands/slash/mod.rs`
4. Add `commands.extend(mycommand::create_commands());` in `create_slash_commands_with_plugins()`
5. Handle command in `src/command_handler.rs`

### Adding IPC Message Types

1. Add variant to `BotEvent` or `TuiCommand` in `src/ipc/protocol.rs`
2. Add handler in `src/ipc/server.rs` (for TuiCommand)
3. Add handler in TUI state (for BotEvent)

### Adding Database Tables

1. Add table creation in `src/database.rs` `ensure_tables_exist()`
2. Add query functions in same file
3. Call from appropriate feature module

### Modifying TUI Screens

1. UI rendering: `src/tui/ui/<screen>.rs`
2. State management: `src/tui/state/<screen>.rs`
3. Event handling: `src/tui/app.rs` in `handle_key_event()`

## Key Files by Change Type

| Change Type | Primary Files |
|-------------|---------------|
| Slash commands | `src/commands/slash/`, `src/command_handler.rs` |
| Bot features | `src/features/`, `src/features/mod.rs` |
| IPC protocol | `src/ipc/protocol.rs` |
| TUI screens | `src/tui/ui/`, `src/tui/state/` |
| Configuration | `src/core/config.rs`, `.env.example` |
| Database | `src/database.rs` |
| Discord events | `src/bin/bot.rs` (EventHandler impl) |

## Feature Version Maintenance

When modifying any feature module, follow these rules:

### Feature Header Requirements

Every feature module (`src/features/<name>/mod.rs`) must have a header comment:

```rust
//! # Feature: Feature Name
//!
//! Brief description of the feature.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: true/false
//!
//! ## Changelog
//! - 1.0.0: Initial release
```

### Version Update Rules

- **Patch** (x.x.+1): Bug fixes, internal refactoring
- **Minor** (x.+1.0): New options, settings, or non-breaking enhancements
- **Major** (+1.0.0): Breaking changes, API changes, major behavior changes

### When Adding Features

1. Create the feature module with proper header comment
2. Register the feature in `src/features/mod.rs` (FEATURES array)
3. Update `docs/feature-organization.md` implementation checklist
4. Update `README.md` if user-facing

### When Modifying Features

1. Update the feature header version
2. Add changelog entry in the header
3. Update `src/features/mod.rs` version in FEATURES array
4. Include version in commit message

See `docs/feature-organization.md` for complete feature organization specification.

## Git Conventions

- Use commitizen format for commit messages
- Never use double quotes (`"`) in commit messages
- Include package version numbers in commit messages
- After completing work:
  1. Recommend the commit message (commitizen format)
  2. Update `Cargo.toml` version number
  3. Create and push a git tag
  4. Update `updateMessage.txt` with changes (Visionary persona)

### Commit Message Format

```
<type>(<scope>): <description> (<version>)

<body>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

## Environment Variables

### Required

| Variable | Description |
|----------|-------------|
| `DISCORD_MUPPET_FRIEND` | Discord bot token |
| `OPENAI_API_KEY` | OpenAI API key |

### Optional

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_PATH` | `persona.db` | SQLite database path |
| `LOG_LEVEL` | `info` | Logging level (error/warn/info/debug/trace) |
| `OPENAI_MODEL` | `gpt-5.1` | OpenAI model to use |
| `DISCORD_GUILD_ID` | - | Guild ID for instant command registration |
| `CONFLICT_MEDIATION_ENABLED` | `true` | Enable conflict detection |
| `CONFLICT_SENSITIVITY` | `medium` | Detection sensitivity (low/medium/high/ultra) |
| `MEDIATION_COOLDOWN_MINUTES` | `5` | Cooldown between mediations |

See `.env.example` for full documentation.

## GitHub Issue Creation

Create issues programmatically using `.github/scripts/`:

**Bug Reports:**
```bash
echo '{"title": "Bug title", "description": "Details", "steps": "1. Do X", "expected": "Y", "actual": "Z"}' | .github/scripts/new-bug.sh --json --silent
```

**Feature Requests:**
```bash
echo '{"title": "Feature", "summary": "Brief", "problem": "Why", "solution": "How", "priority": "medium"}' | .github/scripts/new-feature.sh --json --silent
```

**Quick Ideas:**
```bash
echo '{"title": "Idea title"}' | .github/scripts/new-idea.sh --json --silent
```

See `.github/scripts/README.md` for complete documentation.
