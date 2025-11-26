# Per-Bot Command Allowlists

## Overview

Command allowlists enable fine-grained control over which slash commands each bot instance can register with Discord. This prevents command duplication when running multiple bots on the same server and allows you to create specialized bots with limited command sets.

**Key Features:**
- Per-bot command filtering via YAML configuration
- Automatic overlap detection for bots sharing guilds
- **Shared commands** - Allow specific commands to be registered by multiple bots
- Fail-fast validation with helpful error messages
- Backward compatible (no allowlist = all commands)

**Version:** 2.0.0 (Added shared_commands support)

---

## Quick Start

### Basic Configuration

Add a `commands` field to any bot in `config.yaml`:

```yaml
bots:
  - name: "chat-bot"
    discord_token: "${DISCORD_CHAT_TOKEN}"
    commands:
      - "ping"
      - "help"
      - "hey"
      - "explain"
      - "personas"
```

### Multiple Bots, Same Guild

```yaml
bots:
  # Bot 1: Chat commands
  - name: "chat-bot"
    discord_token: "${DISCORD_CHAT_TOKEN}"
    discord_guild_id: "123456789"
    commands:
      - "ping"
      - "hey"
      - "explain"
      - "simple"
      - "personas"

  # Bot 2: Admin commands (no overlap!)
  - name: "admin-bot"
    discord_token: "${DISCORD_ADMIN_TOKEN}"
    discord_guild_id: "123456789"  # Same guild
    commands:
      - "help"
      - "settings"
      - "features"
      - "toggle"
      - "sysinfo"
```

**Important:** Bots sharing a guild **must have non-overlapping command lists** unless those commands are in `shared_commands`. See the [Shared Commands](#shared-commands) section below.

---

## Shared Commands

Discord natively supports multiple applications registering the same slash command name in the same guild. When a user types a shared command, Discord shows both bots' versions in autocomplete, and the user explicitly chooses which bot responds.

### Configuring Shared Commands

Add a top-level `shared_commands` list to your config:

```yaml
# Commands that can be registered by multiple bots in the same guild
shared_commands:
  - "ping"       # Health check - both bots respond to their own ping
  - "help"       # Each bot shows its own available commands
  - "hey"        # Core chat - users choose which personality to talk to
  - "explain"    # Different explanation styles per persona
  - "forget"     # Clear conversation with that specific bot

bots:
  # Space-cadet bot
  - name: "space-cadet"
    discord_token: "${DISCORD_SPACE_CADET}"
    discord_guild_id: "123456789"
    default_persona: "space-cadet"
    commands:
      - "ping"      # Shared - OK!
      - "help"      # Shared - OK!
      - "hey"       # Shared - OK!
      - "explain"   # Shared - OK!
      - "forget"    # Shared - OK!
      - "imagine"   # Unique to space-cadet
      - "remind"

  # Obi-wan bot
  - name: "obi-wan"
    discord_token: "${DISCORD_OBI_WAN}"
    discord_guild_id: "123456789"  # Same guild!
    default_persona: "obi"
    commands:
      - "ping"      # Shared - OK!
      - "help"      # Shared - OK!
      - "hey"       # Shared - OK!
      - "explain"   # Shared - OK!
      - "forget"    # Shared - OK!
      - "recipe"    # Unique to obi-wan
      - "settings"
```

### How It Works in Discord

When a user types `/hey` in a guild with both bots:

1. Discord shows the command autocomplete with both bots listed
2. Each entry shows the bot's name and avatar
3. User clicks on the specific bot's command
4. That bot handles the interaction

### Recommended Shared Commands

| Command | Why Share? |
|---------|------------|
| `ping` | Both bots should respond to health checks |
| `help` | Each bot shows its own command list |
| `version`, `uptime`, `status` | Bot-specific info each should report |
| `forget` | Clear conversation with that specific bot |
| `hey` | Core chat - users pick which personality |
| `explain`, `simple`, `steps` | Different styles per persona |

### Commands to Keep Unique

| Command | Why Unique? |
|---------|-------------|
| `recipe` | May fit one persona better than another |
| `imagine` | Assign to creative-focused bot |
| Admin commands | Centralize on one bot for clarity |

---

## Configuration Reference

### Field: `commands`

**Type:** `Option<Vec<String>>`
**Required:** No
**Default:** `None` (all commands allowed)

**Description:**
List of slash command names this bot is allowed to register with Discord. Commands not in this list will not be available to users.

**Behavior:**
- `commands: null` or omitted → All commands registered (backward compatible)
- `commands: []` → Empty list = All commands registered
- `commands: ["ping", "hey"]` → Only "ping" and "hey" registered

**Example:**
```yaml
commands:
  - "ping"
  - "help"
  - "hey"
  - "explain"
  - "simple"
  - "steps"
  - "personas"
  - "set_persona"
  - "imagine"
  - "remind"
  - "reminders"
```

---

## Available Commands

### Utility Commands
- `ping` - Test bot responsiveness
- `help` - Show help message
- `forget` - Clear conversation history
- `status` - Show bot status and uptime
- `version` - Show bot and feature versions
- `uptime` - Show bot uptime

### Persona Commands
- `personas` - List available personas
- `set_persona` - Change default persona

### Chat/AI Commands
- `hey` - Chat with current persona
- `explain` - Get detailed explanation
- `simple` - Get simple explanation with analogies
- `steps` - Break into actionable steps

### Content Commands
- `recipe` - Get a recipe
- `imagine` - Generate image with DALL-E

### Reminder Commands
- `remind` - Set a reminder
- `reminders` - View/manage reminders

### Admin Commands
*(Require MANAGE_GUILD permission)*
- `introspect` - Explain bot internals
- `set_channel_verbosity` - Set response verbosity
- `set_guild_setting` - Configure guild settings
- `settings` - View current settings
- `admin_role` - Set admin role
- `features` - List features with status
- `toggle` - Enable/disable features
- `sysinfo` - System diagnostics
- `usage` - API usage metrics

### Context Menu Commands
- `Analyze Message` - Right-click message context menu
- `Explain Message` - Right-click message context menu
- `Analyze User` - Right-click user context menu

---

## Validation

### Overlap Detection

The bot validates configuration **at startup** before connecting to Discord. If multiple bots target the same `discord_guild_id`, their command lists are checked for overlaps.

**Example Error:**
```
Error: Command overlap detected in guild '123456789':
  Bot 'chat-bot' and 'admin-bot' both register: ping, help

Fix: Remove overlapping commands from one bot's allowlist
```

### No Allowlist in Shared Guild

If one bot in a shared guild has an allowlist but another doesn't, validation fails:

```
Error: Bot 'chat-bot' in guild '123456789' has no command allowlist while other bots are present.
All bots sharing a guild must have explicit command allowlists to prevent overlaps.
```

### Invalid Command Names

Future enhancement (not yet implemented): Validation will warn about unknown command names.

---

## Common Use Cases

### 1. Specialized Bot Roles

Create bots with specific purposes:

```yaml
bots:
  # Support bot - user-facing commands only
  - name: "support"
    commands:
      - "ping"
      - "help"
      - "hey"
      - "explain"
      - "simple"
      - "personas"
      - "set_persona"
      - "forget"

  # Utility bot - images and reminders
  - name: "utility"
    commands:
      - "imagine"
      - "remind"
      - "reminders"
      - "recipe"

  # Admin bot - server management
  - name: "admin"
    commands:
      - "settings"
      - "features"
      - "toggle"
      - "sysinfo"
      - "usage"
      - "set_channel_verbosity"
```

### 2. Development vs Production

Different command sets for different environments:

```yaml
bots:
  # Production: Limited, stable commands
  - name: "prod-bot"
    discord_token: "${PROD_TOKEN}"
    commands:
      - "ping"
      - "hey"
      - "personas"
      - "help"

  # Development: All commands for testing
  - name: "dev-bot"
    discord_token: "${DEV_TOKEN}"
    discord_guild_id: "${DEV_GUILD}"
    # No commands field = all commands
```

### 3. Gradual Feature Rollout

Enable new commands progressively:

```yaml
bots:
  # Stable bot - only proven commands
  - name: "stable"
    commands:
      - "ping"
      - "hey"
      - "explain"

  # Beta bot - includes experimental features
  - name: "beta"
    discord_guild_id: "${BETA_GUILD}"
    commands:
      - "ping"
      - "hey"
      - "explain"
      - "imagine"  # New feature
      - "recipe"   # New feature
```

---

## Troubleshooting

### Error: "Command overlap detected"

**Problem:** Two bots in the same guild both have the same command in their allowlists, and that command is not in `shared_commands`.

**Solutions:**

1. **Add to shared_commands** (if both bots should have it):
```yaml
shared_commands:
  - "ping"  # Now both bots can register ping

bots:
  - name: "bot1"
    discord_guild_id: "123"
    commands: ["ping", "hey"]
  - name: "bot2"
    discord_guild_id: "123"
    commands: ["ping", "help"]  # OK now!
```

2. **Remove from one bot's list** (if only one bot needs it):
```yaml
bots:
  - name: "bot1"
    discord_guild_id: "123"
    commands: ["hey"]  # Removed ping
  - name: "bot2"
    discord_guild_id: "123"
    commands: ["ping", "help"]
```

### Error: "No command allowlist while other bots are present"

**Problem:** One bot has an allowlist, another in the same guild doesn't.

**Solution:** Either:
1. Add allowlist to the bot missing one
2. Remove allowlist from both (all bots get all commands)
3. Move one bot to a different guild

### Commands Not Appearing in Discord

**Possible Causes:**
1. Command not in allowlist → Add to `commands` list
2. Typo in command name → Check spelling against available commands
3. Bot not restarted → Restart to register new commands
4. Global command propagation delay → Use `discord_guild_id` for instant updates

---

## Implementation Details

### How It Works

1. **Config Loading:** `MultiConfig::from_file()` parses YAML, including `commands` lists
2. **Validation:** `MultiConfig::validate()` checks for overlaps in shared guilds
3. **Filtering:** At bot startup, `filter_slash_commands()` removes non-allowed commands
4. **Registration:** Only filtered commands are registered with Discord API
5. **Runtime:** Discord enforces - users can't invoke commands that weren't registered

### Files Modified

- `src/config.rs` - Added `commands` field to BotConfig, overlap validation
- `src/commands/slash/mod.rs` - Added filtering logic, updated registration functions
- `src/bin/bot.rs` - Passes bot_config to Handler, forwards allowlist to registration
- `src/commands/mod.rs` - Exports filter function

---

## Future Enhancements (Stretch Goals)

### 1. Command Categories

**Status:** Not implemented
**Priority:** Medium

Allow filtering by category instead of individual commands:

```yaml
commands:
  allow_categories:
    - "Utility"
    - "Chat"
    - "Persona"
  deny_categories:
    - "Admin"
```

**Benefits:**
- Easier to maintain (don't need to list every command)
- Automatically includes new commands in category
- Clear intent (e.g., "no admin commands")

**Implementation:**
- Define `CommandCategory` enum (Utility, Chat, Admin, etc.)
- Add metadata to each command definition
- Support both `allow_categories` and individual `commands`

---

### 2. Command Metadata Registry

**Status:** Not implemented
**Priority:** Low

Centralized registry of all commands with metadata:

```rust
pub struct CommandMetadata {
    pub name: &'static str,
    pub category: CommandCategory,
    pub description: &'static str,
    pub required_features: &'static [&'static str],
    pub admin_only: bool,
}
```

**Benefits:**
- Validate command names against registry (catch typos)
- Enable feature-based filtering
- Support permission-based filtering
- Single source of truth for command information

**Implementation:**
- Create `src/commands/metadata.rs`
- Define `ALL_COMMANDS` constant with metadata
- Update validation to use registry

---

### 3. Deny Lists

**Status:** Not implemented
**Priority:** Low

Exclude specific commands instead of allowlisting:

```yaml
commands:
  deny:
    - "imagine"  # Don't allow image generation
    - "sysinfo"  # Don't expose system info
```

**Benefits:**
- Easier for "all except X" scenarios
- Can combine with allowlist for fine control

**Implementation:**
- Add `deny: Vec<String>` to allowlist config
- Apply deny filter after allow filter
- Validate allow + deny don't conflict

---

### 4. Feature-Based Command Filtering

**Status:** Not implemented
**Priority:** Medium

Automatically filter commands based on enabled features:

```yaml
bots:
  - name: "limited-bot"
    feature_flags:
      image_generation: false
      conflict_mediation: false
    # Commands requiring disabled features auto-excluded
```

**Benefits:**
- Commands automatically hidden when features disabled
- No need to manually maintain sync between features and commands
- Prevents user confusion (command exists but doesn't work)

**Implementation:**
- Add `required_features` to command metadata
- Check feature flags during filtering
- Update validation to consider features

---

### 5. Command Name Validation

**Status:** Not implemented
**Priority:** High (should be in v1.1)

Validate command names at config load time:

```yaml
commands:
  - "pnig"  # ERROR: Unknown command. Did you mean "ping"?
```

**Benefits:**
- Catch typos early (config load vs runtime)
- Helpful error messages with suggestions
- Prevents silent failures

**Implementation:**
- Extract list of valid commands from registration code
- Check allowlist against valid list in `validate()`
- Use edit distance for "did you mean?" suggestions

---

### 6. Runtime Command Reloading

**Status:** Not implemented
**Priority:** Low

Hot-reload command configuration without restarting:

```rust
/admin reload_commands
```

**Benefits:**
- Update command lists without bot downtime
- Faster iteration during development
- Better for production deployments

**Implementation:**
- Add command to reload config
- Re-validate allowlists
- Re-register commands with Discord
- Handle rollback on failure

---

### 7. Command Aliases

**Status:** Not implemented
**Priority:** Low

Allow bots to register same command under different names:

```yaml
commands:
  - name: "hey"
    alias: "chat"  # Also register as /chat
```

**Benefits:**
- Different bots can have same functionality, different names
- Gradual command renaming
- Support multiple conventions

**Implementation:**
- Add alias support to command definitions
- Update filtering to handle aliases
- Ensure no alias conflicts

---

### 8. Per-Guild Command Overrides

**Status:** Not implemented
**Priority:** Low

Different command sets for different guilds:

```yaml
bots:
  - name: "multi-guild-bot"
    commands:
      - "ping"
      - "hey"
    guild_overrides:
      "123456789":  # Premium guild gets extra commands
        commands:
          - "ping"
          - "hey"
          - "imagine"
          - "sysinfo"
```

**Benefits:**
- Tiered access (free vs premium guilds)
- Beta testing in specific guilds
- Custom deployments per customer

**Implementation:**
- Add `guild_overrides` to BotConfig
- Check guild ID during registration
- Apply override allowlist if present

---

### 9. Command Usage Analytics

**Status:** Not implemented
**Priority:** Medium

Track which commands are actually used:

```rust
/admin command_stats
> Most used: hey (450), ping (230), help (120)
> Never used: sysinfo (0), introspect (0)
```

**Benefits:**
- Identify unused commands (candidates for removal)
- Understand user behavior
- Optimize bot configurations

**Implementation:**
- Log command usage per bot
- Store in usage_stats table
- Create analytics query/report

---

### 10. Command Dependencies

**Status:** Not implemented
**Priority:** Low

Commands that require other commands:

```yaml
# If you enable "set_persona", "personas" is auto-included
```

**Benefits:**
- Prevents broken UX (can set but not view personas)
- Simplifies config (don't need to remember dependencies)
- Self-documenting

**Implementation:**
- Add `requires: Vec<String>` to command metadata
- Auto-add dependencies during filtering
- Warn if dependency would be excluded

---

## Migration Guide

### From No Allowlists to Allowlists

**Step 1:** Identify current commands (run bot, use `/help`)

**Step 2:** Create allowlist for first bot:
```yaml
bots:
  - name: "existing-bot"
    discord_token: "${TOKEN}"
    commands:  # Add this
      - "ping"
      - "help"
      # ... list all commands you want
```

**Step 3:** Test in development guild first:
```yaml
discord_guild_id: "${DEV_GUILD}"
```

**Step 4:** Verify commands appear in Discord

**Step 5:** Deploy to production

### Adding a Second Bot to Existing Guild

**Step 1:** List commands of existing bot

**Step 2:** Decide command split:
```
Existing bot: ping, hey, explain, help
New bot: imagine, remind, sysinfo
```

**Step 3:** Add allowlists to BOTH bots:
```yaml
bots:
  - name: "existing-bot"
    commands:  # Add to existing config
      - "ping"
      - "hey"
      - "explain"
      - "help"

  - name: "new-bot"
    discord_guild_id: "123456789"  # Same guild
    commands:
      - "imagine"
      - "remind"
      - "sysinfo"
```

**Step 4:** Restart existing bot (to apply allowlist)

**Step 5:** Start new bot

---

## FAQ

**Q: Can I use allowlists with global command registration?**
A: Yes! Allowlists work with both guild (`discord_guild_id` set) and global (no `discord_guild_id`) registration.

**Q: What happens if I remove a command from the allowlist?**
A: On next bot restart, that command will be unregistered from Discord. Users won't see it anymore.

**Q: Can I add commands without restarting?**
A: Not currently. You must restart the bot to register new commands. (See Future Enhancement #6)

**Q: Do allowlists affect bang commands (!help, !status, etc.)?**
A: Not currently. Allowlists only apply to slash commands (/command). Bang commands are always available.

**Q: Can I have the same command on different bots in different guilds?**
A: Yes! Overlap detection only applies to bots sharing the same `discord_guild_id`.

**Q: What if I don't specify discord_guild_id?**
A: Commands are registered globally (available in all guilds). Overlap detection doesn't apply since each bot registers to its own application.

**Q: How do I know which commands my bot has?**
A: Use `/help` in Discord, or check the registration logs at bot startup.

---

## See Also

- [Multi-Bot Support](multi-bot-support.md) - Overall multi-bot architecture
- [Feature Organization](feature-organization.md) - Feature system documentation
- [Configuration System](../src/config.rs) - Configuration code reference
