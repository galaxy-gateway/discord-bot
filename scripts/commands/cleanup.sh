#!/bin/bash
# Script to delete ALL registered Discord commands (both global and guild)

set -e

# Load environment variables (resolve repo root from this script's location)
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "${REPO_ROOT}/.env"

BOT_TOKEN="$DISCORD_MUPPET_FRIEND"
APP_ID="1473528052062359786"
GUILD_ID="1028101463274164274"

# Abort if the Discord API returned an error object instead of an array, so a
# stale token/app id can't be miscounted by `jq length` and then crash mid-delete.
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
echo "Discord Command Cleanup Script"
echo "=========================================="
echo ""

# Function to delete all global commands
delete_global_commands() {
    echo "🌍 Fetching global commands..."
    GLOBAL_COMMANDS=$(curl -s "https://discord.com/api/v10/applications/${APP_ID}/commands" \
        -H "Authorization: Bot ${BOT_TOKEN}")
    assert_command_array "global" "$GLOBAL_COMMANDS"

    GLOBAL_COUNT=$(echo "$GLOBAL_COMMANDS" | jq '. | length')
    echo "Found $GLOBAL_COUNT global commands"

    if [ "$GLOBAL_COUNT" -gt 0 ]; then
        echo "$GLOBAL_COMMANDS" | jq -r '.[].id' | while read cmd_id; do
            CMD_NAME=$(echo "$GLOBAL_COMMANDS" | jq -r ".[] | select(.id==\"$cmd_id\") | .name")
            echo "  Deleting global command: $CMD_NAME (ID: $cmd_id)"
            curl -s -X DELETE "https://discord.com/api/v10/applications/${APP_ID}/commands/${cmd_id}" \
                -H "Authorization: Bot ${BOT_TOKEN}" > /dev/null
            echo "    ✓ Deleted"
        done
        echo "✅ All global commands deleted"
    else
        echo "✓ No global commands to delete"
    fi
    echo ""
}

# Function to delete all guild commands
delete_guild_commands() {
    echo "🏰 Fetching guild commands..."
    GUILD_COMMANDS=$(curl -s "https://discord.com/api/v10/applications/${APP_ID}/guilds/${GUILD_ID}/commands" \
        -H "Authorization: Bot ${BOT_TOKEN}")
    assert_command_array "guild" "$GUILD_COMMANDS"

    GUILD_COUNT=$(echo "$GUILD_COMMANDS" | jq '. | length')
    echo "Found $GUILD_COUNT guild commands"

    if [ "$GUILD_COUNT" -gt 0 ]; then
        echo "$GUILD_COMMANDS" | jq -r '.[].id' | while read cmd_id; do
            CMD_NAME=$(echo "$GUILD_COMMANDS" | jq -r ".[] | select(.id==\"$cmd_id\") | .name")
            echo "  Deleting guild command: $CMD_NAME (ID: $cmd_id)"
            curl -s -X DELETE "https://discord.com/api/v10/applications/${APP_ID}/guilds/${GUILD_ID}/commands/${cmd_id}" \
                -H "Authorization: Bot ${BOT_TOKEN}" > /dev/null
            echo "    ✓ Deleted"
        done
        echo "✅ All guild commands deleted"
    else
        echo "✓ No guild commands to delete"
    fi
    echo ""
}

# Main execution
echo "This will delete ALL registered commands (both global and guild)"
echo "The bot will re-register them automatically on next restart"
echo ""
read -p "Continue? (y/n) " -n 1 -r
echo ""

if [[ $REPLY =~ ^[Yy]$ ]]; then
    delete_global_commands
    delete_guild_commands

    echo "=========================================="
    echo "✅ Command cleanup complete!"
    echo "=========================================="
    echo ""
    echo "Next steps:"
    echo "1. Restart the bot: systemctl --user restart persona.service"
    echo "2. Wait 10 seconds for registration"
    echo "3. Test in your guild channel (not DMs)"
    echo "4. You should see only one /set_persona with 'obi' option"
    echo ""
    echo "Note: DMs won't have commands since we're using guild-only mode"
else
    echo "Cancelled"
    exit 1
fi
