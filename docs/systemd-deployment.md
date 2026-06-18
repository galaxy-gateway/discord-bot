# Systemd Deployment Guide

This guide explains how to run the Discord Persona Bot as a **systemd user
service**, ensuring it runs persistently in the background and survives SSH
disconnections and system reboots — all managed by the `botuser` account
**without sudo**.

## Why a user service?

The bot runs entirely as `botuser`. Installing it as a *user* service (under
`~/.config/systemd/user/`) instead of a system service (`/etc/systemd/system/`)
means `botuser` can install, start, stop, restart, and inspect logs for the
service with no root privileges. The only requirement is **lingering**, which
lets the user manager — and therefore the service — run at boot without anyone
logging in.

## Prerequisites

- The bot must be compiled in release mode: `cargo build --release`
- The `.env` file must be properly configured with all required environment variables
- The system must have systemd available (standard on most modern Linux distributions)
- **Lingering must be enabled for `botuser`** so the service runs at boot
  without a login session. This is a one-time action that requires root:
  ```bash
  sudo loginctl enable-linger botuser
  ```
  Verify with `loginctl show-user botuser | grep Linger` (should show `Linger=yes`).

## Available Binaries

The project builds multiple binaries:
- **`bot`** - The main Discord bot (Gateway WebSocket connection) - **USE THIS ONE**
- **`persona`** - Simple test binary (just prints "Hello, world!")

The systemd service is configured to run the `bot` binary.

## Service Configuration

The `persona.service` file in the project root contains the systemd service
definition. Note there is **no** `User=`/`Group=` — a user service always runs
as the owning user — and it installs into the user manager (`default.target`):

```ini
[Unit]
Description=Discord Persona Bot

[Service]
Type=simple
WorkingDirectory=%h/Projects/bot/persona
ExecStart=%h/Projects/bot/persona/target/release/bot
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=persona-bot

# Load environment variables from .env file
EnvironmentFile=%h/Projects/bot/persona/.env

[Install]
WantedBy=default.target
```

The user manager sets `XDG_RUNTIME_DIR=/run/user/1005` automatically, which
rootless podman (the transcribe plugin) relies on — so it no longer needs to be
hardcoded in the unit.

## Migrating from the old system service

If the bot was previously installed as a root-owned system service at
`/etc/systemd/system/persona.service`, tear it down once (requires root) before
enabling the user service, so two instances don't connect to Discord at the
same time:

```bash
sudo systemctl disable --now persona.service
sudo rm /etc/systemd/system/persona.service
sudo systemctl daemon-reload
```

Then follow the installation steps below as `botuser`.

## Installation Steps

### Quick Setup with Makefile (Recommended)

```bash
# View all available commands
make help

# Build the release binary and install the user service
make install-service

# Start the service
make start

# View logs in real-time
make logs-follow
```

### Manual Installation Steps

If you prefer to install manually (all as `botuser`, no sudo):

1. **Build the release binary:**
   ```bash
   cargo build --release
   ```

2. **Copy the service file into the user unit directory:**
   ```bash
   mkdir -p ~/.config/systemd/user
   cp $HOME/Projects/bot/persona/persona.service ~/.config/systemd/user/
   ```

3. **Reload the systemd user daemon:**
   ```bash
   systemctl --user daemon-reload
   ```

4. **Enable the service to start on boot:**
   ```bash
   systemctl --user enable persona.service
   ```

5. **Start the service:**
   ```bash
   systemctl --user start persona.service
   ```

6. **Verify the service is running:**
   ```bash
   systemctl --user status persona.service
   ```

## Service Management

### Using Makefile (Recommended)

```bash
# View service status
make status

# Start the service
make start

# Stop the service
make stop

# Restart the service
make restart

# Uninstall the service
make uninstall-service
```

### Using systemctl Directly

```bash
# View service status
systemctl --user status persona.service

# Stop the bot
systemctl --user stop persona.service

# Start the bot
systemctl --user start persona.service

# Restart the bot
systemctl --user restart persona.service

# Disable auto-start on boot
systemctl --user disable persona.service

# Re-enable auto-start on boot
systemctl --user enable persona.service
```

## Viewing Logs

The bot outputs to the user journal. You can view logs using the Makefile or `journalctl --user`:

### Using Makefile (Recommended)

```bash
# View real-time logs
make logs-follow

# View recent logs (last 100 lines)
make logs

# View logs since last boot
make logs-boot

# Export logs to a file
make logs-export
```

### Using journalctl Directly

```bash
# View real-time logs
journalctl --user -u persona.service -f

# View recent logs (last 100 lines)
journalctl --user -u persona.service -n 100

# View logs since boot
journalctl --user -u persona.service -b

# View logs from a specific time range
journalctl --user -u persona.service --since "2024-01-01 00:00:00" --until "2024-01-01 23:59:59"

# Export logs to a file
journalctl --user -u persona.service > persona-bot.log
```

## Service Features

The systemd service provides several important features:

- **Automatic Restart**: If the bot crashes, it will automatically restart after 10 seconds
- **Boot Persistence**: With lingering enabled, the bot starts automatically when the system boots
- **Session Independence**: The bot continues running even if you log out or your SSH connection drops
- **Centralized Logging**: All output is captured in the user journal
- **Environment Management**: Environment variables are loaded from the `.env` file

## Troubleshooting

### Service Won't Start

1. **Check service status for errors:**
   ```bash
   systemctl --user status persona.service
   ```

2. **View detailed logs:**
   ```bash
   journalctl --user -u persona.service -n 50
   ```

3. **Verify the binary exists and is executable:**
   ```bash
   ls -la $HOME/Projects/bot/persona/target/release/bot
   ```

4. **Check .env file permissions:**
   ```bash
   ls -la $HOME/Projects/bot/persona/.env
   ```

### Service Doesn't Start at Boot

The user manager only runs at boot if lingering is enabled:

```bash
loginctl show-user botuser | grep Linger   # expect Linger=yes
sudo loginctl enable-linger botuser        # one-time, requires root
```

### Environment Variables Not Loading

If the bot can't read environment variables:

1. **Verify .env file exists:**
   ```bash
   cat $HOME/Projects/bot/persona/.env
   ```

2. **Check file permissions:**
   ```bash
   chmod 600 $HOME/Projects/bot/persona/.env
   ```

3. **Ensure the .env file format is correct** (no spaces around `=`, one variable per line)

### Updating the Bot

When you update the code:

**Using Makefile:**
```bash
# Rebuild and restart in one command
make update
```

**Manual steps:**
1. **Rebuild the release binary:**
   ```bash
   cargo build --release
   ```

2. **Restart the service:**
   ```bash
   systemctl --user restart persona.service
   ```

3. **Verify it's running with the new code:**
   ```bash
   systemctl --user status persona.service
   journalctl --user -u persona.service -f
   ```

## Alternative: Running with nohup

If you prefer not to use systemd, you can use `nohup` as a simpler alternative:

```bash
nohup $HOME/Projects/bot/persona/target/release/bot > persona.log 2>&1 &
```

However, the systemd user service is recommended as it provides better
management, logging, and automatic restart capabilities.
