.PHONY: help build build-release run clean test install-service start stop restart status logs logs-follow uninstall-service env-check scripts/% check-commands cleanup-commands test-env test-openai

# Self-documenting Makefile
.DEFAULT_GOAL := help

# Project configuration
BINARY_NAME := bot
SERVICE_NAME := persona.service
RELEASE_BINARY := target/release/$(BINARY_NAME)
SERVICE_PATH := /etc/systemd/system/$(SERVICE_NAME)
PROJECT_DIR := $(shell pwd)

help: ## Display this help message
	@echo "$(BINARY_NAME) - Discord Bot Makefile"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@awk 'BEGIN {FS = ":.*##"; printf ""} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

##@ Development

build: ## Build the project in debug mode
	cargo build

build-release: ## Build the project in release mode (optimized)
	cargo build --release

run: ## Run the bot in development mode
	cargo run --bin bot

run-release: build-release ## Run the bot in release mode
	$(RELEASE_BINARY)

test: ## Run tests
	cargo test

clean: ## Clean build artifacts
	cargo clean

check: ## Check code without building
	cargo check

fmt: ## Format code with rustfmt
	cargo fmt

lint: ## Run clippy linter
	cargo clippy -- -D warnings

##@ Deployment

install-service: build-release ## Install systemd service (requires sudo)
	@echo "Installing systemd service..."
	@if [ ! -f "$(PROJECT_DIR)/.env" ]; then \
		echo "Error: .env file not found. Copy .env.example and configure it first."; \
		exit 1; \
	fi
	sudo cp $(PROJECT_DIR)/$(SERVICE_NAME) $(SERVICE_PATH)
	sudo systemctl daemon-reload
	sudo systemctl enable $(SERVICE_NAME)
	@echo "Service installed successfully!"
	@echo "Run 'make start' to start the service"

uninstall-service: ## Uninstall systemd service (requires sudo)
	@echo "Uninstalling systemd service..."
	sudo systemctl stop $(SERVICE_NAME) 2>/dev/null || true
	sudo systemctl disable $(SERVICE_NAME) 2>/dev/null || true
	sudo rm -f $(SERVICE_PATH)
	sudo systemctl daemon-reload
	@echo "Service uninstalled successfully!"

start: ## Start the systemd service (requires sudo)
	sudo systemctl start $(SERVICE_NAME)
	@echo "Service started!"
	@echo "Run 'make status' to check status or 'make logs-follow' to view logs"

stop: ## Stop the systemd service (requires sudo)
	sudo systemctl stop $(SERVICE_NAME)
	@echo "Service stopped!"

restart: ## Restart the systemd service (requires sudo)
	sudo systemctl restart $(SERVICE_NAME)
	@echo "Service restarted!"

reload-service: ## Reload systemd service after config changes (requires sudo)
	@echo "Stopping service..."
	sudo systemctl stop $(SERVICE_NAME)
	@echo "Copying updated service file..."
	sudo cp $(PROJECT_DIR)/$(SERVICE_NAME) $(SERVICE_PATH)
	@echo "Reloading systemd daemon..."
	sudo systemctl daemon-reload
	@echo "Starting service..."
	sudo systemctl start $(SERVICE_NAME)
	@echo "Service configuration reloaded!"

status: ## Show systemd service status
	sudo systemctl status $(SERVICE_NAME)

##@ Logging

logs: ## View recent service logs (last 100 lines)
	sudo journalctl -u $(SERVICE_NAME) -n 100

logs-follow: ## Follow service logs in real-time
	sudo journalctl -u $(SERVICE_NAME) -f

logs-boot: ## View logs since last boot
	sudo journalctl -u $(SERVICE_NAME) -b

logs-export: ## Export logs to file
	sudo journalctl -u $(SERVICE_NAME) > $(BINARY_NAME)-logs-$$(date +%Y%m%d-%H%M%S).log
	@echo "Logs exported to $(BINARY_NAME)-logs-$$(date +%Y%m%d-%H%M%S).log"

##@ Environment

env-check: ## Check if required environment variables are set
	@echo "Checking environment configuration..."
	@if [ ! -f "$(PROJECT_DIR)/.env" ]; then \
		echo "❌ .env file not found"; \
		echo "   Run: cp .env.example .env"; \
		exit 1; \
	fi
	@if ! grep -q "DISCORD_SPACE_CADET=your_discord_bot_token_here" $(PROJECT_DIR)/.env; then \
		echo "✓ DISCORD_SPACE_CADET is configured"; \
	else \
		echo "❌ DISCORD_SPACE_CADET needs to be configured in .env"; \
	fi
	@if ! grep -q "OPENAI_API_KEY=your_openai_api_key_here" $(PROJECT_DIR)/.env; then \
		echo "✓ OPENAI_API_KEY is configured"; \
	else \
		echo "❌ OPENAI_API_KEY needs to be configured in .env"; \
	fi
	@echo "Environment check complete!"

env-setup: ## Copy .env.example to .env
	@if [ -f "$(PROJECT_DIR)/.env" ]; then \
		echo ".env file already exists. Backup created as .env.backup"; \
		cp $(PROJECT_DIR)/.env $(PROJECT_DIR)/.env.backup; \
	fi
	cp $(PROJECT_DIR)/.env.example $(PROJECT_DIR)/.env
	@echo ".env file created! Please edit it with your credentials."

##@ Database

db-status: ## Check database status
	@if [ -f "$(PROJECT_DIR)/persona.db" ]; then \
		echo "✓ Database file exists: persona.db"; \
		ls -lh $(PROJECT_DIR)/persona.db; \
	else \
		echo "❌ Database file not found (will be created on first run)"; \
	fi

db-backup: ## Backup database
	@if [ -f "$(PROJECT_DIR)/persona.db" ]; then \
		cp $(PROJECT_DIR)/persona.db $(PROJECT_DIR)/persona.db.backup-$$(date +%Y%m%d-%H%M%S); \
		echo "Database backed up to persona.db.backup-$$(date +%Y%m%d-%H%M%S)"; \
	else \
		echo "No database file to backup"; \
	fi

##@ Complete Workflows

setup: env-check build-release install-service ## Complete setup: check env, build, and install service
	@echo ""
	@echo "=========================================="
	@echo "Setup complete!"
	@echo "=========================================="
	@echo "Next steps:"
	@echo "  1. Verify .env configuration: make env-check"
	@echo "  2. Start the service: make start"
	@echo "  3. Check status: make status"
	@echo "  4. View logs: make logs-follow"

update: build-release restart ## Update: rebuild and restart service
	@echo "Bot updated and restarted!"

reinstall: stop build-release start ## Full reinstall: stop, rebuild, and restart
	@echo "Bot reinstalled and restarted!"

##@ Scripts

# Delegate to scripts/Makefile for organized script targets
# Usage: make scripts/commands/check, make scripts/test/all, etc.
scripts/%:
	@$(MAKE) -C scripts $*

# Convenience aliases for common script operations
check-commands: ## Check registered Discord commands
	@$(MAKE) -C scripts commands/check

cleanup-commands: ## Remove duplicate command registrations
	@$(MAKE) -C scripts commands/cleanup

test-env: ## Validate environment configuration
	@$(MAKE) -C scripts test/env

test-openai: ## Test OpenAI API connectivity
	@$(MAKE) -C scripts test/openai
