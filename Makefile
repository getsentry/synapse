# Makefile for Synapse Rust workspace

.PHONY: help setup build test clean fmt fmt-check lint fix check dev-locator dev-proxy run-locator run-locator-gcs  run-proxy run-echo-server run-mock-control-api setup-python

# Default target
help:
	@echo "Available targets:"
	@echo "  setup        - Install development dependencies"
	@echo "  setup-python - Set up Python virtual environment with uv"
	@echo "  build        - Build all workspace members"
	@echo "  test         - Run tests for all workspace members"
	@echo "  fmt          - Format code"
	@echo "  fmt-check    - Check code formatting (for CI)"
	@echo "  lint         - Run clippy linter (warnings as errors)"
	@echo "  fix          - Auto-fix formatting and clippy issues"
	@echo "  check        - Run cargo check"
	@echo "  clean        - Clean build artifacts"
	@echo "  run-locator  - Run the locator service"
	@echo "  run-proxy    - Run the proxy service"
	@echo "  run-ingest-router - Run the ingest-router service"
	@echo "  run-mock-control-api - Run the mock control API server"


# Setup development environment
setup: setup-python
	rustup update
	cargo install cargo-watch
	@echo "Setting up Git hooks..."
	@mkdir -p .git/hooks
	@cp scripts/pre-commit .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Git pre-commit hook installed successfully!"

# Setup Python virtual environment with uv
setup-python:
	@echo "Setting up Python virtual environment with uv..."
	@command -v uv >/dev/null 2>&1 || { echo "Error: uv is not installed. Install it from https://docs.astral.sh/uv/getting-started/installation/"; exit 1; }
	uv venv .venv --python 3.13
	@echo "Python virtual environment created at .venv"
	@echo "Run 'direnv allow' to automatically activate the virtual environment"

# Build all workspace members
build:
	cargo build --workspace

# Test all workspace members
test:
	docker compose -f docker-compose.test.yml up -d
	curl -s -X POST \
		-H "Content-Type: application/json" \
		-d '{"name":"test-bucket"}' \
		http://localhost:4443/storage/v1/b?project=test-project
	cargo test --workspace
	docker compose -f docker-compose.test.yml down -v

# Format code
fmt:
	cargo fmt --all

# Check code formatting (for CI)
fmt-check:
	cargo fmt --all -- --check

# Run clippy linter
lint:
	cargo clippy --workspace -- -D warnings

# Auto-fix formatting and clippy issues
fix:
	cargo fmt --all
	cargo clippy --workspace --fix --allow-dirty

# Run cargo check
check:
	cargo check --workspace

# Clean build artifacts
clean:
	cargo clean

# Run services
run-locator:
	cargo run locator --config-file-path example_config_locator.yaml

run-locator-gcs:
	cargo run locator --config-file-path example_config_locator_gcs.yaml

run-proxy:
	cargo run proxy --config-file-path example_config_proxy.yaml

run-ingest-router:
	cargo run ingest-router --config-file-path example_config_ingest_router.yaml

run-mock-control-api:
	python scripts/mock_control_api.py

# CI-like checks (what runs in GitHub Actions)
ci: fmt-check lint test build
	@echo "All CI checks passed!"

# Server for testing proxy locally
run-echo-server:
	python scripts/echo_server.py
