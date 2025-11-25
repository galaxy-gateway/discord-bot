# ngrok Setup for Discord Bot

This guide explains how to set up ngrok to create a public tunnel for your Discord bot during development.

## Overview

**Important Note**: Discord bots typically communicate with Discord via Gateway WebSockets, so ngrok tunnels are **not required** for basic bot functionality. However, ngrok can be useful for:

- **Development/Testing**: Exposing local HTTP endpoints for testing
- **Webhook Development**: If you add webhook functionality later
- **Debugging**: Monitoring HTTP traffic during development
- **Integration Testing**: Testing with external services that need to reach your bot

## Quick Start

### 1. Initial Setup

Run the setup script to configure ngrok:

```bash
./setup-ngrok.sh
```

This will:
- Check if ngrok is installed
- Help you configure your authtoken
- Verify your environment setup

### 2. Get ngrok Authtoken

1. Go to [ngrok Dashboard](https://dashboard.ngrok.com/get-started/your-authtoken)
2. Sign up or log in
3. Copy your authtoken
4. Run: `./ngrok config add-authtoken YOUR_TOKEN_HERE`

### 3. Start Bot with Tunnel

```bash
./start-with-tunnel.sh
```

This will:
- Start ngrok tunnel on port 8080
- Build and start your Discord bot
- Show tunnel URL and dashboard link
- Handle cleanup when stopped

## Configuration

### ngrok Configuration (`ngrok.yml`)

```yaml
version: "2"

tunnels:
  discord-bot:
    proto: http
    addr: 8080
    inspect: true
    log_level: info
    
  bot-alt:
    proto: http
    addr: 3000
    inspect: true
```

### Environment Variables

Make sure your `.env` file contains:

```env
DISCORD_SPACE_CADET=your_discord_bot_token
OPENAI_API_KEY=your_openai_api_key
DATABASE_PATH=persona.db
LOG_LEVEL=info
```

## Usage

### Starting the Bot

```bash
# Option 1: With ngrok tunnel (for development)
./start-with-tunnel.sh

# Option 2: Bot only (normal operation)
cargo run --bin bot
```

### Monitoring

When running with ngrok:
- **ngrok Dashboard**: http://localhost:4040
- **Bot Logs**: Displayed in terminal
- **Tunnel URL**: Shown on startup

### Stopping

Press `Ctrl+C` to stop both the bot and ngrok tunnel cleanly.

## Discord Application Configuration

For webhook-based features (if you add them later):

1. Go to [Discord Developer Portal](https://discord.com/developers/applications)
2. Select your application
3. Go to "General Information"
4. Set "Interactions Endpoint URL" to your ngrok URL + endpoint
   - Example: `https://abc123.ngrok.io/interactions`

**Note**: This is **NOT needed** for the current bot implementation since it uses Gateway WebSockets.

## Troubleshooting

### Common Issues

1. **"ngrok not found"**
   - Make sure you're in the correct directory
   - Check if ngrok binary exists: `ls -la ngrok`

2. **"Tunnel failed to start"**
   - Check if port 8080 is available
   - Verify authtoken is configured: `./ngrok config check`

3. **"Bot won't start"**
   - Check environment variables
   - Verify Discord token is valid
   - Check OpenAI API key

### Port Conflicts

If port 8080 is in use, edit `ngrok.yml`:

```yaml
tunnels:
  discord-bot:
    proto: http
    addr: 3000  # Change to available port
```

### Logs

Check ngrok logs:
```bash
tail -f ngrok.log
```

## Security Notes

1. **Never commit your authtoken** to version control
2. **Tunnel URLs are public** - anyone with the URL can access your endpoints
3. **Use tunnels only for development** - not for production
4. **Discord tokens are sensitive** - keep them secure

## Advanced Usage

### Custom Subdomain (Paid Plan)

Edit `ngrok.yml`:
```yaml
tunnels:
  discord-bot:
    proto: http
    addr: 8080
    subdomain: my-discord-bot  # Requires paid plan
```

### Multiple Tunnels

```bash
# Start specific tunnel
./ngrok start discord-bot --config=ngrok.yml

# Start multiple tunnels
./ngrok start discord-bot bot-alt --config=ngrok.yml
```

### HTTP Authentication

```yaml
tunnels:
  discord-bot:
    proto: http
    addr: 8080
    auth: "username:password"
```

## Integration with Discord Bot

Since this Discord bot uses Gateway WebSockets, ngrok is primarily useful for:

1. **Future webhook development**
2. **HTTP health check endpoints**
3. **Admin/monitoring interfaces**
4. **Integration testing**

The bot will work perfectly without ngrok for all current functionality (slash commands, interactions, etc.).

## Support

If you encounter issues:
1. Check the logs in your terminal
2. Visit ngrok dashboard at http://localhost:4040
3. Review this documentation
4. Check ngrok's official documentation at https://ngrok.com/docs