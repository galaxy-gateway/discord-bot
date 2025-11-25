#!/bin/bash

# Script to start Discord bot with HTTP interactions and ngrok tunnel
# This script starts both the HTTP Discord bot and ngrok tunnel for HTTP interactions

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🚀 Starting Discord Bot (HTTP Mode) with ngrok tunnel...${NC}"

# Check if ngrok binary exists
if [ ! -f "./ngrok" ]; then
    echo -e "${RED}❌ ngrok binary not found. Please run the setup first.${NC}"
    exit 1
fi

# Check if .env file exists
if [ ! -f ".env" ]; then
    echo -e "${YELLOW}⚠️  .env file not found. Make sure environment variables are set.${NC}"
    echo "Required variables:"
    echo "  - DISCORD_SPACE_CADET"
    echo "  - OPENAI_API_KEY" 
    echo "  - DISCORD_PUBLIC_KEY (for HTTP interactions)"
    echo "  - DATABASE_PATH (optional)"
    echo "  - LOG_LEVEL (optional)"
fi

# Function to cleanup background processes
cleanup() {
    echo -e "\\n${YELLOW}🧹 Cleaning up...${NC}"
    if [ ! -z "$NGROK_PID" ]; then
        kill $NGROK_PID 2>/dev/null || true
        echo -e "${GREEN}✅ Stopped ngrok tunnel${NC}"
    fi
    if [ ! -z "$BOT_PID" ]; then
        kill $BOT_PID 2>/dev/null || true  
        echo -e "${GREEN}✅ Stopped Discord bot (HTTP mode)${NC}"
    fi
    exit 0
}

# Set up signal handlers
trap cleanup SIGINT SIGTERM

# Start ngrok tunnel in background
echo -e "${BLUE}🌐 Starting ngrok tunnel...${NC}"
./ngrok start discord-bot --config=ngrok.yml &
NGROK_PID=$!

# Wait a moment for ngrok to start
sleep 3

# Get ngrok tunnel URL
TUNNEL_URL=""
for i in {1..10}; do
    TUNNEL_URL=$(curl -s http://localhost:4040/api/tunnels 2>/dev/null | jq -r '.tunnels[0].public_url // empty' 2>/dev/null || echo "")
    if [ ! -z "$TUNNEL_URL" ]; then
        break
    fi
    echo -e "${YELLOW}⏳ Waiting for ngrok tunnel to start... (attempt $i/10)${NC}"
    sleep 2
done

if [ ! -z "$TUNNEL_URL" ]; then
    echo -e "${GREEN}✅ ngrok tunnel started successfully!${NC}"
    echo -e "${BLUE}🌐 Public URL: ${TUNNEL_URL}${NC}"
    echo -e "${BLUE}🎛️  ngrok dashboard: http://localhost:4040${NC}"
    echo -e "${BLUE}📡 Interactions endpoint: ${TUNNEL_URL}/interactions${NC}"
else
    echo -e "${RED}❌ Failed to start ngrok tunnel${NC}"
    kill $NGROK_PID 2>/dev/null || true
    exit 1
fi

# Build the bot
echo -e "${BLUE}🔨 Building Discord bot (HTTP mode)...${NC}"
if cargo build --bin http_bot; then
    echo -e "${GREEN}✅ Bot built successfully${NC}"
else
    echo -e "${RED}❌ Failed to build bot${NC}"
    cleanup
    exit 1
fi

# Start the Discord bot in HTTP mode
echo -e "${BLUE}🤖 Starting Discord bot (HTTP mode)...${NC}"
cargo run --bin http_bot &
BOT_PID=$!

echo -e "${GREEN}✅ Discord bot (HTTP mode) started with PID: $BOT_PID${NC}"
echo -e "${BLUE}📊 Monitor the bot logs above${NC}"
echo -e "${BLUE}🌐 ngrok web interface: http://localhost:4040${NC}"
echo -e "${BLUE}📡 Interactions endpoint: ${TUNNEL_URL}/interactions${NC}"
echo ""
echo -e "${YELLOW}📋 Next steps:${NC}"
echo -e "${YELLOW}1. Go to Discord Developer Portal${NC}"
echo -e "${YELLOW}2. Set Interactions Endpoint URL to: ${TUNNEL_URL}/interactions${NC}"
echo -e "${YELLOW}3. Test with slash commands in Discord${NC}"
echo ""
echo -e "${YELLOW}💡 Press Ctrl+C to stop both services${NC}"

# Wait for bot process
wait $BOT_PID

# Cleanup when bot exits
cleanup