#!/bin/bash

# Test script to verify .env file loading

echo "🧪 Testing .env file loading..."

# Check if .env file exists
if [ ! -f ".env" ]; then
    echo "❌ .env file not found!"
    echo "Create a .env file with your configuration:"
    echo "cp .env.example .env"
    echo "# Then edit .env with your actual tokens"
    exit 1
fi

echo "✅ .env file found"

# Test configuration loading
echo "🔍 Testing configuration loading..."

# Run a quick config test
cargo run --bin bot 2>&1 | head -5 | grep -E "(Starting|Configuration|✅|❌)" || {
    echo "❌ Bot failed to start - check your .env configuration"
    echo ""
    echo "Required variables in .env:"
    echo "- DISCORD_SPACE_CADET=your_discord_bot_token"
    echo "- OPENAI_API_KEY=your_openai_api_key"
    echo ""
    echo "Optional variables:"
    echo "- LOG_LEVEL=debug"
    echo "- DATABASE_PATH=persona.db"
    echo "- DISCORD_PUBLIC_KEY=your_public_key (only for HTTP mode)"
    exit 1
}

echo "✅ Configuration loading test completed"
echo ""
echo "The bot should now be loading all configuration from your .env file!"
echo "Start the bot with: cargo run --bin bot"