# Persona Discord Bot

A sophisticated Discord bot that provides AI-powered conversations through different personas. Built with Rust, Serenity, and OpenAI integration.

## Features

- **Multiple Personas**: Switch between different AI personalities (Muppet Expert, Chef, Obi-Wan Kenobi, Teacher, Analyst)
- **User Preferences**: Each user can set their default persona
- **Rate Limiting**: Prevents API abuse with configurable rate limits
- **Database Storage**: SQLite database for user preferences and usage statistics
- **Comprehensive Logging**: Detailed logging with configurable levels
- **Error Handling**: Robust error handling throughout the application
- **Full Discord Interactions Support**: All Discord interaction types supported
  - **Slash Commands**: Modern Discord commands with auto-completion
  - **Button Interactions**: Interactive buttons for quick actions
  - **Modal Forms**: Pop-up forms for detailed input
  - **Context Menu Commands**: Right-click commands on messages and users
  - **Autocomplete**: Smart suggestions for command parameters
  - **Deferred Responses**: Proper handling of Discord's 3-second timeout with 15-minute processing window
- **Audio Transcription**: Automatic transcription of audio files using OpenAI Whisper

## Available Commands

The bot supports both Discord slash commands (recommended) and traditional text commands:

### Slash Commands (Recommended)

**User Commands:**
- `/ping` - Test bot responsiveness
- `/help` - Show help message with all commands
- `/personas` - List available personas and show current persona
- `/set_persona <persona>` - Set your default persona (with dropdown)
- `/ask @persona <message>` - Chat with any persona (supports modifiers and history)
- `/forget` - Clear your conversation history with the bot
- `/remind <time> <message>` - Set a reminder
- `/reminders [action] [id]` - List or cancel reminders
- `/imagine <prompt> [size] [style]` - Generate an image using DALL-E

**Utility Commands:**
- `/status` - Show bot status and uptime
- `/version` - Show bot and feature versions
- `/uptime` - Show how long the bot has been running

**Admin Commands** (require MANAGE_GUILD):
- `/features` - List all features with their toggle status
- `/toggle <feature>` - Enable/disable toggleable features for this server
- `/introspect <component>` - Explain bot internals
- `/settings` - View current guild configuration
- `/set_channel_verbosity <level> [channel]` - Set response verbosity

### Bang Commands (Text-based)

Quick text-based commands for power users:

**Info Commands:**
- `!help` - Quick command reference
- `!status` - Bot status and uptime
- `!version` - Show bot and feature versions
- `!uptime` - How long bot has been running

**Quick Commands:**
- `!ping` - Fast ping (text response)
- `!features` - List all features with versions

**Admin Commands** (require MANAGE_GUILD):
- `!toggle <feature>` - Enable/disable a feature
- `!reload` - Reload guild settings
- `!sync` - Force sync slash commands

### Interactive Features

#### Button Interactions
- **Help Buttons**: Interactive help with modal forms for detailed questions
- **Persona Selection**: Quick persona switching with emoji buttons
- **Confirmation Dialogs**: Confirm/cancel actions with visual feedback

#### Modal Forms
- **Help & Feedback**: Detailed help requests with context
- **Custom Prompts**: Create custom AI prompts on-the-fly

#### Context Menu Commands
- **Analyze Message**: Right-click any message to get AI analysis
- **Explain Message**: Right-click any message for explanations
- **Analyze User**: Right-click users for general information

#### Auto-completion
- Smart suggestions for command parameters (future enhancement)

## Available Personas

- **muppet** - Enthusiastic Muppet expert (default)
- **chef** - Passionate cooking expert
- **teacher** - Patient teacher who explains things clearly
- **analyst** - Step-by-step analyst who breaks down complex processes

## Setup

### Prerequisites

- Rust (latest stable version)
- Discord Bot Token
- OpenAI API Key

### Installation

1. Clone the repository:
   ```bash
   git clone <repository-url>
   cd persona
   ```

2. Copy the environment file and fill in your credentials:
   ```bash
   cp .env.example .env
   ```

3. Edit `.env` with your actual tokens:
   ```
   DISCORD_MUPPET_FRIEND=your_discord_bot_token_here
   OPENAI_API_KEY=your_openai_api_key_here
   ```

4. Build and run the bot:
   ```bash
   cargo run --bin bot
   ```

### Development with ngrok (Optional)

For development purposes, you can set up ngrok to create public tunnels:

```bash
# Setup ngrok (one-time)
./setup-ngrok.sh

# Start bot with ngrok tunnel
./start-with-tunnel.sh
```

See [NGROK_SETUP.md](NGROK_SETUP.md) for detailed instructions.

**Note**: ngrok is **not required** for normal Discord bot operation since the bot uses Gateway WebSockets, not webhooks.

### Discord Bot Setup

1. Go to [Discord Developer Portal](https://discord.com/developers/applications)
2. Create a new application
3. Go to the "Bot" section and create a bot
4. Copy the token and add it to your `.env` file
5. Under "Privileged Gateway Intents", enable "Message Content Intent"
6. Use the OAuth2 URL generator to invite the bot to your server with appropriate permissions

### OpenAI Setup

1. Go to [OpenAI Platform](https://platform.openai.com/api-keys)
2. Create a new API key
3. Add it to your `.env` file

## Configuration

### Environment Variables

- `DISCORD_MUPPET_FRIEND` - Your Discord bot token (required)
- `OPENAI_API_KEY` - Your OpenAI API key (required)
- `OPENAI_MODEL` - OpenAI model to use (optional, defaults to "gpt-5.1")
  - Options: gpt-4o, gpt-4o-mini, gpt-4-turbo, gpt-3.5-turbo, etc.
- `DATABASE_PATH` - Path to SQLite database file (optional, defaults to "persona.db")
- `LOG_LEVEL` - Logging level (optional, defaults to "info")
- `DISCORD_GUILD_ID` - Your Discord server ID for development mode (optional)
  - When set, slash commands register instantly (guild commands)
  - Without this, commands register globally and take up to 1 hour to propagate
  - Get this by right-clicking your server > Copy Server ID (requires Developer Mode enabled in Discord settings)

### Logging Levels

- `error` - Only errors
- `warn` - Warnings and errors
- `info` - General information, warnings, and errors
- `debug` - Debug information and above
- `trace` - All logging information

## Architecture

The bot is structured with the following modules:

- `config.rs` - Configuration management
- `database.rs` - SQLite database operations
- `personas.rs` - Persona definitions and management
- `command_handler.rs` - Core command routing and processing
- `commands/` - Unified command system
  - `slash/` - Discord slash command definitions and registration
  - `bang/` - Text-based bang commands (!)
- `features.rs` - Feature registry with version tracking
- `message_components.rs` - Interactive components (buttons, modals, select menus)
- `rate_limiter.rs` - Rate limiting functionality
- `audio.rs` - Audio transcription functionality
- `bin/bot.rs` - Main bot entry point with full interaction support

## Database Schema

The bot uses SQLite with the following tables:

- `user_preferences` - Stores user's default persona settings
- `usage_stats` - Tracks command usage for analytics

## Rate Limiting

The bot implements rate limiting to prevent abuse:
- 10 requests per minute per user
- Automatic backoff and user notification when limits are exceeded

## Discord Interaction Handling

The bot properly handles Discord's interaction requirements:

### Deferred Response Pattern
- **3-Second Rule**: All interactions are acknowledged within 3 seconds
- **Deferred Processing**: AI-powered commands use `DeferredChannelMessageWithSource`
- **15-Minute Window**: Full processing time available after deferring
- **"Thinking..." Indicator**: Discord shows native thinking animation during processing

### Interaction Types Supported
- **Slash Commands**: Immediate defer → AI processing → edit response
- **Modal Submissions**: Immediate defer → AI processing → edit response  
- **Context Menu Commands**: Immediate defer → AI processing → edit response
- **Button Interactions**: Instant response for UI updates
- **Autocomplete**: Sub-3-second suggestions

### Error Recovery
- Intelligent error detection (timeout vs. API errors)
- Fallback response mechanisms if edit operations fail
- User-friendly error messages with retry suggestions

## Gateway Connection Management

The bot properly implements Discord's gateway WebSocket connection requirements:

### Automatic Gateway URL Retrieval
- **GET /api/gateway**: Serenity automatically calls Discord's gateway endpoint
- **WebSocket URL**: Retrieves the proper WebSocket URL for connection
- **Token Authentication**: Proper bot token authentication handling

### Connection Process
1. **Client Initialization**: `Client::builder()` configures the Discord client
2. **Gateway URL Fetch**: Automatic retrieval of WebSocket endpoint
3. **WebSocket Connection**: Establishes secure connection to Discord
4. **Heartbeat Management**: Automatic OP 1 heartbeat payloads
5. **Session Handling**: Proper session management and reconnection

### Monitoring & Diagnostics
- **Connection Logging**: Detailed gateway connection status
- **Session Information**: Gateway session ID and version tracking
- **Shard Information**: Multi-shard support for large bots (2500+ guilds)
- **Error Diagnostics**: Clear error messages for connection issues

## Audio Transcription

The bot includes an audio transcription script (`scripts/audio.sh`) that uses OpenAI's Whisper API:

```bash
./scripts/audio.sh <path-to-audio-file>
```

## Production Deployment

For production deployment, you can run the bot as a systemd service to ensure it:
- Runs persistently in the background
- Survives SSH disconnections and system reboots
- Automatically restarts on crashes
- Provides centralized logging

### Quick Start with Makefile

```bash
# View all available commands
make help

# Complete setup in one command (checks env, builds, installs service)
make setup

# Start the service
make start

# View logs in real-time
make logs-follow
```

For a complete list of Makefile commands, see the [Makefile Quick Reference](docs/makefile-reference.md).

See the [Systemd Deployment Guide](docs/systemd-deployment.md) for detailed setup instructions.

## Development

### Using the Makefile

The project includes a self-documented Makefile for common tasks:

```bash
# View all available commands
make help

# Build in debug mode
make build

# Build in release mode
make build-release

# Run the bot in development
make run

# Run tests
make test

# Format code
make fmt

# Run linter
make lint

# Clean build artifacts
make clean
```

### Manual Commands

```bash
# Build the project
cargo build

# Build in release mode
cargo build --release

# Run the bot
cargo run --bin bot

# Run tests (when implemented)
cargo test
```

### Adding New Personas

To add a new persona, edit `src/personas.rs` and add a new entry to the `PersonaManager::new()` function:

```rust
personas.insert("your_persona".to_string(), Persona {
    name: "Your Persona Name".to_string(),
    system_prompt: "Your persona's system prompt here.".to_string(),
    description: "Brief description of your persona".to_string(),
});
```

## License

This project is open source. Please check the license file for details.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request