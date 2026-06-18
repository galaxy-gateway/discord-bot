#!/bin/bash
# Script to check currently registered Discord commands

set -e

# Load environment variables (resolve repo root from this script's location)
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${REPO_ROOT}/.env"

BOT_TOKEN="$DISCORD_MUPPET_FRIEND"
APP_ID="1473528052062359786"

# Guild IDs to inspect, read from the same DISCORD_GUILD_ID the bot registers to.
# Accepts a comma-separated list (multiple servers); falls back to the original
# guild if the var is unset. Normalize commas to spaces for iteration.
GUILD_IDS="${DISCORD_GUILD_ID:-1028101463274164274}"
GUILD_IDS="${GUILD_IDS//,/ }"

# Abort early if the Discord API returned an error object instead of an array.
# Without this, an auth/permission error (e.g. code 20012) gets miscounted as
# "N commands" by `jq length` and then crashes downstream with a cryptic
# "Cannot index string with string" error.
assert_command_array() {
    local label="$1" payload="$2"
    if [ "$(echo "$payload" | jq -r 'type')" != "array" ]; then
        echo "❌ Discord API error fetching ${label} commands:" >&2
        echo "$payload" | jq -r '"   \(.message // .) (code \(.code // "?"))"' >&2
        echo "   Check DISCORD_MUPPET_FRIEND token and APP_ID (${APP_ID})." >&2
        exit 1
    fi
}

echo "=========================================="
echo "Discord Command Status Check"
echo "=========================================="
echo ""

# Check global commands
echo "🌍 GLOBAL COMMANDS (show in DMs and all servers):"
echo "=================================================="
GLOBAL_COMMANDS=$(curl -s "https://discord.com/api/v10/applications/${APP_ID}/commands" \
    -H "Authorization: Bot ${BOT_TOKEN}")
assert_command_array "global" "$GLOBAL_COMMANDS"

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
echo "🏰 GUILD COMMANDS (show only in that server - instant updates):"
echo "================================================================"

# Sum of commands across every configured guild, used for the summary below.
GUILD_COUNT=0
for GUILD_ID in $GUILD_IDS; do
    echo "--- Guild ${GUILD_ID} ---"
    GUILD_COMMANDS=$(curl -s "https://discord.com/api/v10/applications/${APP_ID}/guilds/${GUILD_ID}/commands" \
        -H "Authorization: Bot ${BOT_TOKEN}")
    assert_command_array "guild ${GUILD_ID}" "$GUILD_COMMANDS"

    THIS_COUNT=$(echo "$GUILD_COMMANDS" | jq '. | length')
    GUILD_COUNT=$((GUILD_COUNT + THIS_COUNT))
    echo "Total: $THIS_COUNT commands"
    echo ""

    if [ "$THIS_COUNT" -gt 0 ]; then
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
done
echo "=========================================="
echo "Summary:"
echo "=========================================="
GUILD_NUM=$(echo $GUILD_IDS | wc -w)
echo "- Global commands: $GLOBAL_COUNT (show everywhere, 1hr update delay)"
echo "- Guild commands: $GUILD_COUNT across $GUILD_NUM guild(s) (show in those servers only, instant updates)"
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
