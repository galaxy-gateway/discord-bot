# Verbosity Control & Role-Based Permissions

This document describes the channel-based verbosity system and role-based command permissions for the Persona Discord Bot.

## Overview

The bot implements a multi-layered approach to control response length and manage administrative access:

- **Channel-based verbosity**: Different channels can have different response detail levels
- **Custom admin role**: A designated role controls who can change bot settings
- **Smart message splitting**: Long responses are split intelligently at paragraph/sentence boundaries

## Verbosity Levels

| Level | Description | Max Tokens | Use Case |
|-------|-------------|------------|----------|
| `concise` | Brief, direct answers | ~300 | General chat, quick questions |
| `normal` | Balanced responses | ~600 | Default for most channels |
| `detailed` | Comprehensive explanations | ~1500 | Help channels, tutorials |

### Verbosity Cascade

When generating a response, the bot looks up verbosity in this order:
1. **Channel-specific setting** → If set, use this
2. **Guild default setting** → Fall back to server-wide default
3. **System default** → `concise` (brief responses)

## Database Schema

### channel_settings Table

```sql
CREATE TABLE channel_settings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    verbosity TEXT DEFAULT 'concise',
    conflict_enabled BOOLEAN DEFAULT 1,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(guild_id, channel_id)
);
```

### Guild Settings (existing table)

Used for storing all guild-wide settings. See [Guild Settings](#guild-settings) below for the full list.

## Admin Commands

### /set_channel_verbosity

Set the verbosity level for a specific channel.

**Usage:** `/set_channel_verbosity level:<concise|normal|detailed> [channel:#channel]`

**Parameters:**
- `level` (required): The verbosity level to set
- `channel` (optional): Target channel (defaults to current channel)

**Permissions:** Requires `Manage Server` permission or Bot Admin role

**Example:**
```
/set_channel_verbosity level:detailed channel:#help
```

### /settings

View current bot settings for the guild and channel.

**Usage:** `/settings`

**Output:**
- Current channel verbosity
- Guild default verbosity
- Bot admin role (if set)
- Conflict mediation status

**Permissions:** Requires `Manage Server` permission or Bot Admin role

### /admin_role

Set which role can manage bot settings.

**Usage:** `/admin_role role:@RoleName`

**Parameters:**
- `role` (required): The role to grant bot management permissions

**Permissions:** Requires `Administrator` permission (Discord admin only)

**Example:**
```
/admin_role role:@Bot Managers
```

## Permission System

### Access Levels

| Level | Who | Can Do |
|-------|-----|--------|
| **Everyone** | All users | Use chat commands (/ask, context menus, etc.) |
| **Bot Admin** | Users with Bot Admin role | Change channel settings, view settings |
| **Administrator** | Discord server admins | Set bot admin role, all settings |

### Permission Check Flow

```
1. Is user a Discord Administrator?
   → YES: Allow all commands
   → NO: Continue...

2. Is command an admin command?
   → NO: Allow (public command)
   → YES: Continue...

3. Does guild have a bot_admin_role set?
   → NO: Deny (only Discord admins)
   → YES: Continue...

4. Does user have the bot_admin_role?
   → YES: Allow
   → NO: Deny
```

### Implementation

Admin commands use Discord's `default_member_permissions` to require `MANAGE_GUILD` permission by default. Server admins can override this in Server Settings > Integrations to allow specific roles.

Additionally, the bot checks for a custom "Bot Admin" role stored in the database for finer-grained control.

## Smart Message Splitting

When responses exceed Discord's 2000 character limit, the bot splits them intelligently:

### Split Priority Order

1. **Code block boundaries** (`\`\`\`\n`) - Never break mid-code-block
2. **Paragraph boundaries** (`\n\n`) - Preferred split point
3. **Single newlines** (`\n`) - Next best option
4. **Sentence endings** (`. `, `! `, `? `) - Maintain readability
5. **Word boundaries** (` `) - Last resort
6. **Hard split** - Only if no other option (rare)

### Example

Long response gets split into:
```
Message 1: [First paragraph + second paragraph]
Message 2: [Third paragraph + code block]
Message 3: [Remaining content]
```

## Prompt Modifications

Each persona prompt includes Discord-specific brevity guidelines:

```markdown
## Discord Response Style
- Default to brief, helpful answers (2-3 sentences)
- Offer to elaborate rather than front-loading detail
- Match response length to question complexity
- Use Discord formatting: **bold**, `code`, > quotes
```

### Verbosity Suffixes

Added to system prompts based on channel verbosity:

**Concise:**
```
Keep responses brief and to the point. Aim for 2-3 sentences unless the topic truly requires more. If more detail might help, end with "Want me to elaborate?"
```

**Normal:**
(No suffix - use base prompt as-is)

**Detailed:**
```
Provide comprehensive, detailed explanations. Include examples, context, and thorough coverage of the topic. The user wants depth.
```

## Configuration Examples

### Example 1: Help-focused Server

```
#general        → concise (quick chat)
#help           → detailed (thorough answers)
#off-topic      → concise (casual)
#announcements  → normal (balanced)
```

### Example 2: Development Server

```
#general        → concise
#code-review    → detailed
#questions      → normal
#random         → concise
```

## API Reference

### Database Methods

```rust
// Get verbosity for a channel (with cascade)
db.get_channel_verbosity(guild_id, channel_id) -> String

// Set verbosity for a channel
db.set_channel_verbosity(guild_id, channel_id, "concise"|"normal"|"detailed")

// Get all channel settings
db.get_channel_settings(guild_id, channel_id) -> (verbosity, conflict_enabled)

// Check if user has bot admin role
db.has_bot_admin_role(guild_id, user_roles) -> bool

// Guild settings
db.set_guild_setting(guild_id, "bot_admin_role", role_id)
db.set_guild_setting(guild_id, "default_verbosity", "normal")
db.get_guild_setting(guild_id, setting_key) -> Option<String>
```

### Command Handlers

```rust
// In commands.rs
handle_set_channel_verbosity(ctx, command) -> Result<()>
handle_settings(ctx, command) -> Result<()>
handle_admin_role(ctx, command) -> Result<()>

// Permission check helper
check_bot_admin_permission(ctx, command) -> Result<bool>
```

## Files Modified

| File | Changes |
|------|---------|
| `src/database.rs` | Added channel_settings table and methods |
| `src/slash_commands.rs` | Added admin commands with permissions |
| `src/commands.rs` | Added command handlers, verbosity integration |
| `src/personas.rs` | Added verbosity suffix system |
| `prompt/*.md` | Added Discord brevity guidelines |

## Guild Settings

All guild-wide settings are configured using the `/set_guild_setting` command.

**Usage:** `/set_guild_setting setting:<setting_name> value:<value>`

**Permissions:** Requires `Manage Server` permission or Bot Admin role

### Available Settings

| Setting | Values | Default | Description |
|---------|--------|---------|-------------|
| `default_verbosity` | concise, normal, detailed | concise | Guild-wide default response length |
| `default_persona` | obi, muppet, chef, teacher, analyst | obi | Default persona for users who haven't set their own |
| `conflict_mediation` | enabled, disabled | enabled | Toggle conflict detection and mediation |
| `conflict_sensitivity` | low, medium, high, ultra | medium | How aggressively the bot detects conflicts |
| `mediation_cooldown` | 1, 5, 10, 15, 30, 60 | 5 | Minutes between mediation attempts per channel |
| `max_context_messages` | 10, 20, 40, 60 | 40 | Conversation history messages included in AI context |
| `audio_transcription` | enabled, disabled | enabled | Toggle audio file transcription feature |
| `mention_responses` | enabled, disabled | enabled | Whether bot responds when @mentioned |
| `bot_admin_role` | Role ID | Not set | Role that can manage bot settings (set via `/admin_role`) |

### Conflict Sensitivity Thresholds

| Level | Threshold | Description |
|-------|-----------|-------------|
| `low` | 0.7 | Only very obvious conflicts trigger mediation |
| `medium` | 0.5 | Balanced detection (default) |
| `high` | 0.35 | More sensitive - catches subtle tension |
| `ultra` | 0.3 | Maximum sensitivity - triggers on single hostile keywords |

### Setting Cascade/Fallback Behavior

Settings follow this precedence:

1. **Guild-specific setting** (if set via `/set_guild_setting`)
2. **Environment variable** (if applicable, for backwards compatibility)
3. **System default** (hardcoded fallback)

For example, `default_persona`:
1. Check guild's `default_persona` setting
2. Check `PERSONA` environment variable
3. Fall back to "obi"

### Examples

```
# Set guild-wide verbosity default
/set_guild_setting setting:default_verbosity value:normal

# Set guild-wide default persona
/set_guild_setting setting:default_persona value:teacher

# Reduce conflict detection sensitivity
/set_guild_setting setting:conflict_sensitivity value:low

# Disable audio transcription
/set_guild_setting setting:audio_transcription value:disabled

# Increase mediation cooldown to 30 minutes
/set_guild_setting setting:mediation_cooldown value:30
```

## Future Enhancements

- [ ] Thread-based expansion ("Continue in Thread" button)
- [ ] "Show More" button for truncated responses
- [ ] Per-channel persona override
- [ ] Scheduled verbosity changes (e.g., detailed during business hours)
- [ ] Analytics on verbosity preferences
- [x] ~~Per-guild default persona~~ (Implemented)
- [x] ~~Per-guild conflict settings~~ (Implemented)
- [x] ~~Per-guild context message limit~~ (Implemented)
