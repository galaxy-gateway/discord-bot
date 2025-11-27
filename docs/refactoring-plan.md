# Refactoring Plan: Clean Architecture Reorganization

## Goals
1. **Remove bang commands** - Delete `!` prefix commands entirely, keep only slash commands
2. **Clean Architecture structure** - Reorganize into layered architecture (core/, infrastructure/, application/, presentation/, features/)
3. **Split large files** - Break up `command_handler.rs` (3319 lines) and `database.rs` (2174 lines)

## Target Directory Structure

```
src/
├── bin/
│   └── bot.rs                        # Main entry point
│
├── core/                             # Domain Layer - Shared types & config
│   ├── mod.rs
│   ├── config.rs                     # (move from src/config.rs)
│   ├── error.rs                      # Unified error types
│   └── types.rs                      # Shared domain types
│
├── features/                         # Feature modules (bounded contexts)
│   ├── mod.rs                        # Feature registry & versioning
│   ├── personas/
│   │   ├── mod.rs
│   │   └── manager.rs                # (from personas.rs)
│   ├── conversation/
│   │   ├── mod.rs
│   │   └── service.rs                # AI conversation logic
│   ├── audio/
│   │   ├── mod.rs
│   │   └── transcriber.rs            # (from audio.rs)
│   ├── image_gen/
│   │   ├── mod.rs
│   │   └── generator.rs              # (from image_gen.rs)
│   ├── conflict/
│   │   ├── mod.rs
│   │   ├── detector.rs               # (from conflict_detector.rs)
│   │   └── mediator.rs               # (from conflict_mediator.rs)
│   ├── reminders/
│   │   ├── mod.rs
│   │   └── scheduler.rs              # (from reminder_scheduler.rs)
│   ├── analytics/
│   │   ├── mod.rs
│   │   ├── usage_tracker.rs          # (from usage_tracker.rs)
│   │   ├── interaction_tracker.rs    # (from interaction_tracker.rs)
│   │   └── system_info.rs            # (from system_info.rs)
│   ├── settings/
│   │   ├── mod.rs
│   │   └── service.rs                # Guild/channel settings logic
│   ├── introspection/
│   │   ├── mod.rs
│   │   └── service.rs                # (from introspection.rs)
│   ├── rate_limiting/
│   │   ├── mod.rs
│   │   └── limiter.rs                # (from rate_limiter.rs)
│   └── startup/
│       ├── mod.rs
│       └── notification.rs           # (from startup_notification.rs)
│
├── infrastructure/                   # Infrastructure Layer
│   ├── mod.rs
│   ├── database/
│   │   ├── mod.rs
│   │   ├── connection.rs             # Database connection management
│   │   ├── migrations.rs             # Table initialization
│   │   └── repositories/
│   │       ├── mod.rs
│   │       ├── user.rs               # User preferences
│   │       ├── conversation.rs       # Message history
│   │       ├── settings.rs           # Guild/channel settings
│   │       ├── analytics.rs          # Usage stats, DM tracking
│   │       ├── reminders.rs          # Reminder storage
│   │       └── conflict.rs           # Conflict tracking
│   ├── openai/
│   │   ├── mod.rs
│   │   ├── chat.rs                   # Chat completion client
│   │   ├── whisper.rs                # Audio transcription
│   │   └── dalle.rs                  # Image generation
│   └── http/
│       ├── mod.rs
│       └── server.rs                 # (from http_server.rs)
│
├── application/                      # Application Layer - Use cases
│   ├── mod.rs
│   └── handlers/
│       ├── mod.rs
│       ├── message.rs                # Message event handling
│       ├── interaction.rs            # Slash command dispatch
│       └── component.rs              # Button/modal handling
│
├── presentation/                     # Presentation Layer - Discord interface
│   ├── mod.rs
│   ├── commands/
│   │   ├── mod.rs                    # Command registration
│   │   ├── chat.rs                   # /hey, /explain, /simple, /steps
│   │   ├── persona.rs                # /personas, /set_persona
│   │   ├── utility.rs                # /ping, /help, /status, /version, /uptime, /forget
│   │   ├── admin.rs                  # /settings, /set_*, /admin_role, /toggle, /features, /sysinfo
│   │   ├── imagine.rs                # /imagine
│   │   ├── recipe.rs                 # /recipe
│   │   ├── remind.rs                 # /remind, /reminders
│   │   ├── dm_stats.rs               # /dm_stats_*, /session_history
│   │   ├── usage.rs                  # /usage
│   │   └── introspect.rs             # /introspect
│   ├── context_menu/
│   │   ├── mod.rs
│   │   └── analyze.rs                # Context menu commands
│   ├── components/
│   │   ├── mod.rs
│   │   └── buttons.rs                # (from message_components.rs)
│   └── formatters/
│       ├── mod.rs
│       └── response.rs               # Response formatting helpers
│
└── lib.rs                            # Library root with exports
```

## Implementation Phases

### Phase 1: Remove Bang Commands
**Risk: Low | Files: 5**

1. Delete `src/commands/bang/` directory entirely (4 files)
2. Update `src/commands/mod.rs` - remove `pub mod bang` and bang re-exports
3. Update `src/command_handler.rs` - remove `!` prefix handling in `handle_message()`
4. Update `src/lib.rs` if needed

**Verification:** Bot compiles, slash commands work, `!` messages are ignored

---

### Phase 2: Create Core Module
**Risk: Low | Files: 4 new**

1. Create `src/core/mod.rs`
2. Move `src/config.rs` to `src/core/config.rs`
3. Create `src/core/error.rs` - unified error types
4. Create `src/core/types.rs` - shared types (if needed)
5. Update `src/lib.rs` to export `core`

**Verification:** Bot compiles, all features work

---

### Phase 3: Create Features Module Structure
**Risk: Medium | Files: 13 moves + new mod.rs files**

Move feature modules into subdirectories:

| Source | Destination |
|--------|-------------|
| `personas.rs` | `features/personas/manager.rs` |
| `audio.rs` | `features/audio/transcriber.rs` |
| `image_gen.rs` | `features/image_gen/generator.rs` |
| `conflict_detector.rs` | `features/conflict/detector.rs` |
| `conflict_mediator.rs` | `features/conflict/mediator.rs` |
| `reminder_scheduler.rs` | `features/reminders/scheduler.rs` |
| `usage_tracker.rs` | `features/analytics/usage_tracker.rs` |
| `interaction_tracker.rs` | `features/analytics/interaction_tracker.rs` |
| `system_info.rs` | `features/analytics/system_info.rs` |
| `introspection.rs` | `features/introspection/service.rs` |
| `rate_limiter.rs` | `features/rate_limiting/limiter.rs` |
| `startup_notification.rs` | `features/startup/notification.rs` |
| `features.rs` | `features/mod.rs` (integrate registry) |

Create `mod.rs` for each feature subdirectory with proper exports.

**Verification:** Bot compiles, all features work, tests pass

---

### Phase 4: Create Infrastructure Module
**Risk: High | Files: 10+ new**

#### 4A: Split database.rs into repositories

Create `src/infrastructure/database/`:
- `connection.rs` - Database struct, connection management
- `migrations.rs` - `init_tables()` function
- `repositories/mod.rs` - Re-exports
- `repositories/user.rs` - User preferences, personas
- `repositories/conversation.rs` - Message history
- `repositories/settings.rs` - Guild/channel settings, feature flags
- `repositories/analytics.rs` - Usage tracking, DM sessions
- `repositories/reminders.rs` - Reminder CRUD
- `repositories/conflict.rs` - Conflict tracking

#### 4B: Create OpenAI infrastructure

Move OpenAI client logic from `command_handler.rs` to:
- `infrastructure/openai/chat.rs`
- `infrastructure/openai/whisper.rs`
- `infrastructure/openai/dalle.rs`

#### 4C: Move HTTP server

- Move `http_server.rs` to `infrastructure/http/server.rs`

**Verification:** Bot compiles, database operations work, AI features work

---

### Phase 5: Create Application Layer
**Risk: High | Files: 4 new**

Extract from `command_handler.rs`:
- `application/handlers/message.rs` - `handle_message()`, DM handling, mention handling
- `application/handlers/interaction.rs` - Slash command dispatch
- `application/handlers/component.rs` - Button/modal handling

Keep `CommandHandler` struct but delegate to these handlers.

**Verification:** Bot compiles, all message handling works

---

### Phase 6: Create Presentation Layer
**Risk: Medium | Files: 15+ moves/new**

#### 6A: Reorganize slash commands

Move from `src/commands/slash/` to `src/presentation/commands/`:
- Keep existing file structure but under new path
- Update `mod.rs` for command registration

#### 6B: Move components

- Move `message_components.rs` to `presentation/components/buttons.rs`

#### 6C: Create context menu module

- Move context menu handlers to `presentation/context_menu/`

**Verification:** All slash commands work, context menus work

---

### Phase 7: Final Cleanup
**Risk: Low | Files: Various**

1. Delete empty/unused files from old locations
2. Update `src/lib.rs` with final module structure
3. Update all import paths throughout codebase
4. Run `cargo clippy` and fix warnings
5. Run all tests
6. Update documentation

---

## Critical Files to Read Before Implementation

| File | Lines | Purpose |
|------|-------|---------|
| `src/command_handler.rs` | 3319 | Central dispatcher - understand before splitting |
| `src/database.rs` | 2174 | All DB methods - understand before splitting |
| `src/lib.rs` | 425 | Module declarations - update throughout |
| `src/commands/mod.rs` | ~50 | Command exports - remove bang, reorganize |
| `src/commands/slash/mod.rs` | ~200 | Slash registration pattern |
| `src/features.rs` | 232 | Feature registry to preserve |
| `src/bin/bot.rs` | ~300 | Entry point - understand wiring |

---

## Dependency Rules (Clean Architecture)

```
presentation/ ──► application/ ──► features/
                      │               │
                      └───► infrastructure/
                                    │
                                    ▼
                                 core/
```

- **core/** - No internal dependencies
- **infrastructure/** - Depends only on core/
- **features/** - Depends on core/, defines repository traits
- **application/** - Depends on features/, infrastructure/, core/
- **presentation/** - Depends on application/, core/
- **Features do NOT depend on each other** - coordination in application layer

---

## Commit Strategy (Commitizen)

```
Phase 1: feat(commands)!: remove bang commands (0.7.0)
Phase 2: refactor(core): create core module with config and types (0.7.0)
Phase 3: refactor(features): reorganize feature modules into subdirectories (0.8.0)
Phase 4: refactor(infrastructure): split database and create OpenAI modules (0.9.0)
Phase 5: refactor(application): create handlers layer from command_handler (0.9.0)
Phase 6: refactor(presentation): reorganize commands and components (0.9.0)
Phase 7: chore: final cleanup and documentation (1.0.0)
```

---

## Risk Mitigation

1. **Compile after each phase** - Don't proceed if build fails
2. **Test after each phase** - Run `cargo test` and manual verification
3. **Git commit boundaries** - Each phase is a separate commit for easy rollback
4. **Preserve original files** - Move rather than delete until verified
5. **Update imports incrementally** - Fix one module at a time
