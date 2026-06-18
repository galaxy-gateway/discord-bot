# Scripts Makefile - Scoped targets for bot management scripts
#
# Usage from project root: make scripts/<category>/<target>
# Usage from scripts dir:  make <category>/<target>
#
# Examples:
#   make commands/check
#   make service/reload
#   make tunnel/start-http
#   make test/all

.PHONY: help \
        commands/check commands/cleanup \
        service/reload service/status \
        tunnel/setup tunnel/start-http tunnel/start-gateway \
        test/env test/openai test/all

.DEFAULT_GOAL := help

# Colors for output
CYAN := \033[36m
GREEN := \033[32m
YELLOW := \033[33m
RESET := \033[0m

help: ## Show this help message
	@echo "$(CYAN)Scripts Makefile$(RESET) - Bot management scripts"
	@echo ""
	@echo "$(GREEN)Usage:$(RESET)"
	@echo "  From project root: make scripts/<target>"
	@echo "  From scripts dir:  make <target>"
	@echo ""
	@echo "$(YELLOW)Commands:$(RESET)"
	@echo "  commands/check      Check registered Discord commands"
	@echo "  commands/cleanup    Remove duplicate command registrations"
	@echo ""
	@echo "$(YELLOW)Service:$(RESET)"
	@echo "  service/reload      Reload systemd service"
	@echo "  service/status      Check service status (alias)"
	@echo ""
	@echo "$(YELLOW)Tunnel:$(RESET)"
	@echo "  tunnel/setup        Configure ngrok tunnel"
	@echo "  tunnel/start-http   Start bot with HTTP tunnel"
	@echo "  tunnel/start-gateway Start bot with gateway tunnel"
	@echo ""
	@echo "$(YELLOW)Test:$(RESET)"
	@echo "  test/env            Validate environment configuration"
	@echo "  test/openai         Test OpenAI API connectivity"
	@echo "  test/all            Run all tests"
	@echo ""
	@echo "$(YELLOW)Options:$(RESET)"
	@echo "  ARGS=...            Pass additional arguments to scripts"
	@echo "  TIMEOUT=N           Set timeout for test/openai (seconds)"
	@echo ""
	@echo "$(GREEN)Examples:$(RESET)"
	@echo "  make commands/check ARGS='--verbose'"
	@echo "  make test/openai TIMEOUT=30"

##@ Command Management

commands/check: ## Check registered Discord commands
	@cd .. && ./scripts/commands/check.sh $(ARGS)

commands/cleanup: ## Remove duplicate command registrations
	@cd .. && ./scripts/commands/cleanup.sh $(ARGS)

##@ Service Management

service/reload: ## Reload systemd service
	@cd .. && ./scripts/service/reload.sh $(ARGS)

service/status: ## Check service status
	@systemctl --user status persona.service

##@ Tunnel Management

tunnel/setup: ## Configure ngrok tunnel
	@cd .. && ./scripts/tunnel/setup.sh $(ARGS)

tunnel/start-http: ## Start bot with HTTP tunnel
	@cd .. && ./scripts/tunnel/start-http.sh $(ARGS)

tunnel/start-gateway: ## Start bot with gateway tunnel
	@cd .. && ./scripts/tunnel/start-gateway.sh $(ARGS)

##@ Testing

test/env: ## Validate environment configuration
	@cd .. && ./scripts/test/env.sh $(ARGS)

test/openai: ## Test OpenAI API connectivity
	@cd .. && ./scripts/test/openai.sh $(if $(TIMEOUT),$(TIMEOUT),)

test/all: test/env test/openai ## Run all tests
	@echo "$(GREEN)All tests completed$(RESET)"
