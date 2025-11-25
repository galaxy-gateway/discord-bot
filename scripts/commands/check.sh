#!/bin/bash
# Script to check currently registered Discord commands

set -e

# Load environment variables
source /home/caavere/Projects/bot/persona/.env

BOT_TOKEN="$DISCORD_SPACE_CADET"
APP_ID="1133318314949628007"
GUILD_ID="1028101463274164274"

echo "=========================================="
echo "Discord Command Status Check"
echo "=========================================="
echo ""

# Check global commands
echo "🌍 GLOBAL COMMANDS (show in DMs and all servers):"
echo "=================================================="
GLOBAL_COMMANDS=$(curl -s "https://discord.com/api/v10/applications/${APP_ID}/commands" \
    -H "Authorization: Bot ${BOT_TOKEN}")

GLOBAL_COUNT=$(echo "$GLOBAL_COMMANDS" | jq '. | length')
echo "Total: $GLOBAL_COUNT commands"
echo ""

if [ "$GLOBAL_COUNT" -gt 0 ]; then
    echo "$GLOBAL_COMMANDS" | jq -r '.[] | "  \(.name) (ID: \(.id))"'
    echo ""

    # Check set_persona command for "obi" option
    SET_PERSONA=$(echo "$GLOBAL_COMMANDS" | jq '.[] | select(.name=="set_persona")')
    if [ ! -z "$SET_PERSONA" ]; then
        echo "  📋 set_persona choices:"
        echo "$SET_PERSONA" | jq -r '.options[0].choices[]? | "    - \(.name)"'
    fi
else
    echo "  (none)"
fi

echo ""
echo "🏰 GUILD COMMANDS (show only in your server - instant updates):"
echo "================================================================"
GUILD_COMMANDS=$(curl -s "https://discord.com/api/v10/applications/${APP_ID}/guilds/${GUILD_ID}/commands" \
    -H "Authorization: Bot ${BOT_TOKEN}")

GUILD_COUNT=$(echo "$GUILD_COMMANDS" | jq '. | length')
echo "Total: $GUILD_COUNT commands"
echo ""

if [ "$GUILD_COUNT" -gt 0 ]; then
    echo "$GUILD_COMMANDS" | jq -r '.[] | "  \(.name) (ID: \(.id))"'
    echo ""

    # Check set_persona command for "obi" option
    SET_PERSONA=$(echo "$GUILD_COMMANDS" | jq '.[] | select(.name=="set_persona")')
    if [ ! -z "$SET_PERSONA" ]; then
        echo "  📋 set_persona choices:"
        echo "$SET_PERSONA" | jq -r '.options[0].choices[]? | "    - \(.name)"'
    fi
else
    echo "  (none)"
fi

echo ""
echo "=========================================="
echo "Summary:"
echo "=========================================="
echo "- Global commands: $GLOBAL_COUNT (show everywhere, 1hr update delay)"
echo "- Guild commands: $GUILD_COUNT (show in server only, instant updates)"
echo ""

if [ "$GLOBAL_COUNT" -gt 0 ] && [ "$GUILD_COUNT" -gt 0 ]; then
    echo "⚠️  WARNING: You have BOTH global and guild commands!"
    echo "    This causes duplicates in your server."
    echo "    Recommendation: Run ./cleanup-commands.sh to fix"
    echo ""
fi

if [ "$GLOBAL_COUNT" -gt 0 ]; then
    echo "ℹ️  DMs show global commands (with 1hr update delay)"
fi

if [ "$GUILD_COUNT" -gt 0 ]; then
    echo "ℹ️  Your server shows guild commands (instant updates)"
fi
