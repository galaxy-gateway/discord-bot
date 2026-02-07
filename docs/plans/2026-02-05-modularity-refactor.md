# Comprehensive Refactor: Modularity & Persona Injection Consolidation

> **Status**: Ready for implementation
> **Created**: 2026-02-05
> **Scope**: Major refactor - reduce duplication, split CommandHandler

---

## How to Execute This Plan

This plan should be executed using the **superpowers** skill system:

```
1. Start a new Claude Code session
2. Tell Claude: "Execute the plan at docs/plans/2026-02-05-modularity-refactor.md using superpowers:executing-plans"
3. Claude will use superpowers:writing-plans to create task breakdowns
4. Each milestone can be done in a git worktree using superpowers:using-git-worktrees
5. After each milestone, use superpowers:verification-before-completion to verify
6. Use superpowers:requesting-code-review after completing each phase
```

Recommended approach:
- **Milestone 1** (Shared Utilities): Can be done on main branch - low risk
- **Milestones 2-4** (Handler refactor): Use git worktree for isolation

---

## Goal

Full refactor to reduce duplication and improve modularity by:
1. Consolidating repeated PERSONA_CHOICES, response chunking, and prompt building
2. Splitting the 6,910-line CommandHandler into per-command handlers

---

## Current Problems

| Issue | Location | Impact |
|-------|----------|--------|
| PERSONA_CHOICES duplicated 3x | ask.rs, debate.rs, council.rs | 17 entries x 3 files = maintenance nightmare |
| Response chunking duplicated 8x | command_handler.rs (lines 618, 916, 2296, 2379, 3049, 3309, 6097) | ~400 lines of near-identical code |
| System prompt building scattered | 31+ places in command_handler.rs | Inconsistent patterns, hard to modify |
| CommandHandler monolith | 6,910 lines | Mixes Discord I/O, OpenAI API, formatting, DB |

---

## Phase 1: Shared Utilities (Non-Breaking)

### 1.1 Create Persona Choices Module

**New file:** `src/features/personas/choices.rs`

```rust
//! Shared persona choices for slash commands
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from duplicated constants in ask.rs, debate.rs, council.rs

use serenity::builder::CreateApplicationCommandOption;

/// All available persona choices (display_name, id)
pub const PERSONA_CHOICES: &[(&str, &str)] = &[
    ("Obi-Wan", "obi"),
    ("Muppet Friend", "muppet"),
    ("Chef", "chef"),
    ("Teacher", "teacher"),
    ("Analyst", "analyst"),
    ("Visionary", "visionary"),
    ("Noir Detective", "noir"),
    ("Zen Master", "zen"),
    ("Bard", "bard"),
    ("Coach", "coach"),
    ("Scientist", "scientist"),
    ("Gamer", "gamer"),
    ("Architect", "architect"),
    ("Debugger", "debugger"),
    ("Reviewer", "reviewer"),
    ("DevOps", "devops"),
    ("Designer", "designer"),
];

/// Add all persona choices to a command option builder
pub fn add_persona_choices(option: &mut CreateApplicationCommandOption) {
    for (name, value) in PERSONA_CHOICES {
        option.add_string_choice(name, value);
    }
}

/// Validate a persona ID exists
pub fn is_valid_persona(id: &str) -> bool {
    PERSONA_CHOICES.iter().any(|(_, pid)| *pid == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persona_choices_complete() {
        assert_eq!(PERSONA_CHOICES.len(), 17);
    }

    #[test]
    fn test_is_valid_persona() {
        assert!(is_valid_persona("obi"));
        assert!(is_valid_persona("designer"));
        assert!(!is_valid_persona("invalid"));
    }
}
```

**Update:** `src/features/personas/mod.rs` - add `pub mod choices;` and re-export

**Update files to use shared module:**
- `src/commands/slash/ask.rs` - remove local PERSONA_CHOICES, import from `crate::features::personas::choices`
- `src/commands/slash/debate.rs` - same
- `src/commands/slash/council.rs` - same

### 1.2 Create Response Utilities Module

**New file:** `src/core/response.rs`

```rust
//! Response chunking and Discord message utilities
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from 8 duplicate implementations in command_handler.rs

/// Discord embed description limit
pub const EMBED_LIMIT: usize = 4096;
/// Discord message content limit
pub const MESSAGE_LIMIT: usize = 2000;

/// Chunk text into pieces that fit Discord limits (UTF-8 safe, line-aware)
pub fn chunk_text(text: &str, max_size: usize) -> Vec<String> {
    if text.len() <= max_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let line_with_newline = format!("{}\n", line);
        if current.len() + line_with_newline.len() > max_size {
            if !current.is_empty() {
                chunks.push(current.trim_end().to_string());
                current = String::new();
            }
            // Handle lines longer than max_size
            if line_with_newline.len() > max_size {
                let chars: Vec<char> = line.chars().collect();
                for chunk in chars.chunks(max_size - 1) {
                    chunks.push(chunk.iter().collect());
                }
            } else {
                current = line_with_newline;
            }
        } else {
            current.push_str(&line_with_newline);
        }
    }
    if !current.is_empty() {
        chunks.push(current.trim_end().to_string());
    }
    chunks
}

/// Chunk for embed descriptions (4096 limit)
pub fn chunk_for_embed(text: &str) -> Vec<String> {
    chunk_text(text, EMBED_LIMIT)
}

/// Chunk for message content (2000 limit)
pub fn chunk_for_message(text: &str) -> Vec<String> {
    chunk_text(text, MESSAGE_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_text_no_chunk() {
        let result = chunk_text("hello", 100);
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn test_chunk_respects_lines() {
        let text = "line1\nline2\nline3";
        let result = chunk_text(text, 12);
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_embed_limit() {
        let result = chunk_for_embed(&"a".repeat(5000));
        assert!(result.len() >= 2);
        assert!(result[0].len() <= EMBED_LIMIT);
    }
}
```

**Update:** `src/core/mod.rs` - add `pub mod response;`

**Update:** `src/command_handler.rs` - replace 8 chunking implementations with calls to `core::response::chunk_*`

### 1.3 Create Prompt Builder Utility

**New file:** `src/features/personas/prompt_builder.rs`

```rust
//! Unified system prompt construction
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Consolidated prompt building from 31+ scattered call sites

use super::PersonaManager;

/// Builder for constructing system prompts with modifiers and verbosity
pub struct PromptBuilder<'a> {
    persona_manager: &'a PersonaManager,
    persona_id: String,
    modifier: Option<String>,
    verbosity: String,
    max_paragraphs: Option<u32>,
}

impl<'a> PromptBuilder<'a> {
    pub fn new(persona_manager: &'a PersonaManager, persona_id: &str) -> Self {
        Self {
            persona_manager,
            persona_id: persona_id.to_string(),
            modifier: None,
            verbosity: "normal".to_string(),
            max_paragraphs: None,
        }
    }

    pub fn with_modifier(mut self, modifier: Option<&str>) -> Self {
        self.modifier = modifier.map(String::from);
        self
    }

    pub fn with_verbosity(mut self, verbosity: &str) -> Self {
        self.verbosity = verbosity.to_string();
        self
    }

    pub fn with_max_paragraphs(mut self, max: Option<u32>) -> Self {
        self.max_paragraphs = max;
        self
    }

    pub fn build(self) -> String {
        let mut prompt = self.persona_manager.get_system_prompt_with_verbosity(
            &self.persona_id,
            self.modifier.as_deref(),
            &self.verbosity,
        );

        if let Some(max) = self.max_paragraphs {
            if max > 0 {
                prompt.push_str(&format!(
                    "\n\nIMPORTANT: Limit your response to {} paragraph(s) maximum.",
                    max
                ));
            }
        }
        prompt
    }
}
```

---

## Phase 2: Command Handler Trait & Context

### 2.1 Define Command Handler Trait

**New file:** `src/commands/handler.rs`

```rust
//! Slash command handler trait and infrastructure
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0

use anyhow::Result;
use async_trait::async_trait;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::prelude::Context;
use std::sync::Arc;

use super::context::CommandContext;

/// Trait for slash command handlers
#[async_trait]
pub trait SlashCommandHandler: Send + Sync {
    /// Command name(s) this handler processes
    fn command_names(&self) -> &'static [&'static str];

    /// Handle the slash command
    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()>;
}
```

### 2.2 Create Shared Command Context

**New file:** `src/commands/context.rs`

```rust
//! Shared context for command handlers
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0

use crate::database::Database;
use crate::features::{PersonaManager, UsageTracker, InteractionTracker};
use anyhow::Result;
use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};
use std::time::Duration;
use uuid::Uuid;

/// Shared context for all command handlers
pub struct CommandContext {
    pub persona_manager: PersonaManager,
    pub database: Database,
    pub usage_tracker: UsageTracker,
    pub interaction_tracker: InteractionTracker,
    pub openai_model: String,
    pub openai_api_key: String,
}

impl CommandContext {
    /// Get AI response with conversation context
    ///
    /// This is the core OpenAI integration, moved from CommandHandler::get_ai_response_with_context
    pub async fn get_ai_response(
        &self,
        system_prompt: &str,
        user_message: &str,
        history: Vec<(String, String)>,
        request_id: Uuid,
        user_id: Option<&str>,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
    ) -> Result<String> {
        // Build messages array
        let mut messages = vec![ChatCompletionMessage {
            role: ChatCompletionMessageRole::System,
            content: Some(system_prompt.to_string()),
            name: None,
            function_call: None,
        }];

        // Add conversation history
        for (role, content) in history {
            let role = match role.as_str() {
                "user" => ChatCompletionMessageRole::User,
                "assistant" => ChatCompletionMessageRole::Assistant,
                _ => continue,
            };
            messages.push(ChatCompletionMessage {
                role,
                content: Some(content),
                name: None,
                function_call: None,
            });
        }

        // Add current user message
        messages.push(ChatCompletionMessage {
            role: ChatCompletionMessageRole::User,
            content: Some(user_message.to_string()),
            name: None,
            function_call: None,
        });

        // Call OpenAI API
        let completion = tokio::time::timeout(
            Duration::from_secs(45),
            ChatCompletion::builder(&self.openai_model, messages)
                .create()
        ).await??;

        let response = completion
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default()
            .trim()
            .to_string();

        // Track usage
        if let Some(usage) = completion.usage {
            self.usage_tracker.track_request(
                request_id,
                user_id,
                guild_id,
                channel_id,
                usage.prompt_tokens as u32,
                usage.completion_tokens as u32,
                &self.openai_model,
            ).await;
        }

        Ok(response)
    }
}
```

### 2.3 Create Handler Registry

**New file:** `src/commands/registry.rs`

```rust
//! Command handler registry
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0

use std::collections::HashMap;
use std::sync::Arc;
use super::handler::SlashCommandHandler;

/// Registry mapping command names to handlers
pub struct CommandRegistry {
    handlers: HashMap<&'static str, Arc<dyn SlashCommandHandler>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self { handlers: HashMap::new() }
    }

    /// Register a handler for its declared command names
    pub fn register(&mut self, handler: Arc<dyn SlashCommandHandler>) {
        for name in handler.command_names() {
            self.handlers.insert(name, Arc::clone(&handler));
        }
    }

    /// Get handler for a command name
    pub fn get(&self, name: &str) -> Option<Arc<dyn SlashCommandHandler>> {
        self.handlers.get(name).cloned()
    }

    /// Number of registered commands
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## Phase 3: Per-Command Handler Modules

### New Directory Structure

```
src/commands/
├── mod.rs                    # Existing + new exports
├── handler.rs                # SlashCommandHandler trait
├── context.rs                # CommandContext struct
├── registry.rs               # Handler registry
├── slash/                    # Existing command definitions (keep as-is)
└── handlers/                 # NEW: Per-command implementations
    ├── mod.rs
    ├── utility.rs            # ping, help, status, version, uptime
    ├── ai_chat.rs            # hey, explain, simple, steps, recipe
    ├── ask.rs                # /ask handler
    ├── debate.rs             # /debate handler
    ├── council.rs            # /council handler
    ├── conclude.rs           # /conclude handler
    ├── imagine.rs            # /imagine handler
    ├── persona.rs            # personas, set_user
    ├── remind.rs             # remind, reminders, forget
    └── admin.rs              # settings, set_channel, set_guild, etc.
```

### Migration Order (Simplest First)

| Order | File | Commands | Est. Lines | Dependencies |
|-------|------|----------|------------|--------------|
| 1 | utility.rs | ping, help, status, version, uptime | ~200 | None |
| 2 | persona.rs | personas, set_user | ~150 | PersonaManager |
| 3 | remind.rs | remind, reminders, forget | ~300 | Database |
| 4 | imagine.rs | imagine | ~200 | ImageGenerator |
| 5 | ai_chat.rs | hey, explain, simple, steps, recipe | ~400 | PersonaManager, OpenAI |
| 6 | ask.rs | ask | ~250 | PersonaManager, OpenAI |
| 7 | admin.rs | settings, set_channel, set_guild, etc. | ~500 | Database |
| 8 | debate.rs | debate | ~600 | DebateOrchestrator |
| 9 | council.rs | council, conclude | ~700 | CouncilState |

---

## Phase 4: Refactor CommandHandler to Dispatcher

After Phase 3, `CommandHandler` becomes a thin dispatcher:

```rust
pub struct CommandHandler {
    context: Arc<CommandContext>,
    registry: CommandRegistry,
    rate_limiter: RateLimiter,
    conflict_detector: ConflictDetector,
    conflict_mediator: ConflictMediator,
    // Plugin manager stays here for fallback
    plugin_manager: Option<Arc<PluginManager>>,
}

impl CommandHandler {
    pub async fn handle_slash_command(
        &self,
        ctx: &Context,
        cmd: &ApplicationCommandInteraction
    ) -> Result<()> {
        // Rate limit check
        if let Err(e) = self.rate_limiter.check(&cmd.user.id.to_string()) {
            return self.send_rate_limit_error(ctx, cmd, e).await;
        }

        // Dispatch via registry
        if let Some(handler) = self.registry.get(&cmd.data.name) {
            return handler.handle(Arc::clone(&self.context), ctx, cmd).await;
        }

        // Plugin fallback
        if let Some(pm) = &self.plugin_manager {
            if pm.has_command(&cmd.data.name) {
                return self.handle_plugin_command(ctx, cmd).await;
            }
        }

        // Unknown command
        self.send_unknown_command_error(ctx, cmd).await
    }
}
```

---

## Implementation Milestones

### Milestone 1: Shared Utilities
**Commit message template:** `feat(core): add shared persona choices and response utilities (3.38.0)`

- [ ] Create `src/features/personas/choices.rs`
- [ ] Update `src/features/personas/mod.rs` exports
- [ ] Update `src/commands/slash/ask.rs` to use shared choices
- [ ] Update `src/commands/slash/debate.rs` to use shared choices
- [ ] Update `src/commands/slash/council.rs` to use shared choices
- [ ] Create `src/core/response.rs`
- [ ] Update `src/core/mod.rs` exports
- [ ] Replace chunking in `command_handler.rs` (8 locations)
- [ ] Create `src/features/personas/prompt_builder.rs`
- [ ] Run `make test` and `make lint`

### Milestone 2: Handler Infrastructure
**Commit message template:** `feat(commands): add command handler trait and registry (3.39.0)`

- [ ] Create `src/commands/handler.rs` (trait)
- [ ] Create `src/commands/context.rs` (shared context)
- [ ] Create `src/commands/registry.rs` (handler registry)
- [ ] Update `src/commands/mod.rs` exports
- [ ] Run `make test` and `make lint`

### Milestone 3: Extract Handlers
**Commit message template:** `refactor(commands): extract <name> handlers (3.40.0)`

- [ ] Create `src/commands/handlers/mod.rs`
- [ ] Extract utility handlers (ping, help, status, version, uptime)
- [ ] Extract persona handlers
- [ ] Extract remind handlers
- [ ] Extract imagine handler
- [ ] Extract ai_chat handlers (hey, explain, simple, steps, recipe)
- [ ] Extract ask handler
- [ ] Extract admin handlers
- [ ] Extract debate handler
- [ ] Extract council handler
- [ ] Run `make test` after EACH extraction

### Milestone 4: Dispatcher Refactor
**Commit message template:** `refactor(command_handler): convert to thin dispatcher (3.41.0)`

- [ ] Wire up CommandRegistry in CommandHandler::new()
- [ ] Replace match statement dispatch with registry lookup
- [ ] Remove dead code from command_handler.rs
- [ ] Update tests
- [ ] Final `make test`, `make lint`, `make build-release`

---

## Verification Plan

After each milestone:

1. **Unit tests**: `make test`
2. **Lint**: `make lint`
3. **Build**: `make build-release`
4. **Manual testing** (key commands):
   - `/ping` - basic connectivity
   - `/hey test message` - AI response with persona
   - `/ask persona:obi prompt:hello` - persona selection dropdown works
   - `/debate` - two-persona thread creation
   - `/council` - multi-persona thread creation

---

## Critical Files Reference

| File | Role | Lines (current) |
|------|------|-----------------|
| `src/command_handler.rs` | Main handler (to be split) | 6,910 |
| `src/features/personas/manager.rs` | Persona definitions | 675 |
| `src/commands/slash/ask.rs` | Ask command definition | 102 |
| `src/commands/slash/debate.rs` | Debate command definition | 132 |
| `src/commands/slash/council.rs` | Council command definition | 164 |
| `src/commands/slash/mod.rs` | Command aggregation | 272 |
| `src/core/mod.rs` | Core module exports | 16 |

---

## Success Criteria

- [ ] PERSONA_CHOICES defined in exactly 1 place
- [ ] Response chunking defined in exactly 1 place
- [ ] CommandHandler under 500 lines (from 6,910)
- [ ] Each command handler in its own module
- [ ] All existing tests pass
- [ ] All slash commands work identically to before
- [ ] Feature versions updated in FEATURES array
