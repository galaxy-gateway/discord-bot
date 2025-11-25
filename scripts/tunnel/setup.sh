#!/bin/bash

# Setup script for ngrok authentication
# This script helps configure ngrok with your authtoken

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🔧 ngrok Setup for Discord Bot${NC}"
echo ""

# Check if ngrok binary exists
if [ ! -f "./ngrok" ]; then
    echo -e "${RED}❌ ngrok binary not found in current directory${NC}"
    echo "Please download ngrok first or run from the correct directory."
    exit 1
fi

echo -e "${GREEN}✅ Found ngrok binary${NC}"

# Check current auth status
echo -e "${BLUE}🔍 Checking ngrok authentication status...${NC}"
AUTH_STATUS=$(./ngrok config check 2>&1 || echo "not authenticated")

if echo "$AUTH_STATUS" | grep -q "not authenticated\|invalid\|Unauthorized"; then
    echo -e "${YELLOW}⚠️  ngrok is not authenticated${NC}"
    echo ""
    echo "To set up ngrok authentication:"
    echo "1. Go to https://dashboard.ngrok.com/get-started/your-authtoken"
    echo "2. Sign up/log in to get your authtoken"
    echo "3. Run: ./ngrok config add-authtoken YOUR_TOKEN_HERE"
    echo ""
    echo "Or you can enter your authtoken now:"
    read -p "Enter your ngrok authtoken (or press Enter to skip): " AUTHTOKEN
    
    if [ ! -z "$AUTHTOKEN" ]; then
        echo -e "${BLUE}🔐 Setting up authtoken...${NC}"
        if ./ngrok config add-authtoken "$AUTHTOKEN"; then
            echo -e "${GREEN}✅ Authtoken configured successfully${NC}"
        else
            echo -e "${RED}❌ Failed to configure authtoken${NC}"
            exit 1
        fi
    else
        echo -e "${YELLOW}⏭️  Skipping authtoken setup${NC}"
        echo "Note: You can run this script again or manually configure with:"
        echo "./ngrok config add-authtoken YOUR_TOKEN_HERE"
    fi
else
    echo -e "${GREEN}✅ ngrok is already authenticated${NC}"
fi

echo ""
echo -e "${BLUE}📋 Configuration Summary:${NC}"
echo "  - ngrok binary: ✅ Ready"
echo "  - Configuration file: ngrok.yml"
echo "  - Startup script: start-with-tunnel.sh"
echo ""

echo -e "${GREEN}🎉 Setup complete!${NC}"
echo ""
echo "Next steps:"
echo "1. Make sure your .env file is configured with:"
echo "   - DISCORD_SPACE_CADET (your Discord bot token)"
echo "   - OPENAI_API_KEY (your OpenAI API key)"
echo ""
echo "2. Start the bot with tunnel:"
echo "   ./start-with-tunnel.sh"
echo ""
echo "3. Access ngrok dashboard at:"
echo "   http://localhost:4040"
echo ""

# Check if .env exists
if [ -f ".env" ]; then
    echo -e "${GREEN}✅ .env file found${NC}"
else
    echo -e "${YELLOW}⚠️  .env file not found${NC}"
    echo "Create .env with your configuration:"
    echo ""
    echo "DISCORD_SPACE_CADET=your_discord_bot_token"
    echo "OPENAI_API_KEY=your_openai_api_key"
    echo "DATABASE_PATH=persona.db"
    echo "LOG_LEVEL=info"
fi