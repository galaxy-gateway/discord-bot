#!/bin/bash

# Script to reload the systemd *user* service after configuration changes.
# Runs entirely as botuser via `systemctl --user` — no sudo required.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

echo "Stopping current service..."
systemctl --user stop persona.service

echo "Copying updated service file..."
mkdir -p "${UNIT_DIR}"
cp "${REPO_ROOT}/persona.service" "${UNIT_DIR}/"

echo "Reloading systemd user daemon..."
systemctl --user daemon-reload

echo "Starting service with new configuration..."
systemctl --user start persona.service

echo ""
echo "Service reloaded! Checking status..."
systemctl --user status persona.service
