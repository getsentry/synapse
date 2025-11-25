# Makefile for Synapse Rust workspace

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
.PHONY: help

# Setup development environment
setup: setup-python
	rustup update
	cargo install cargo-watch
	@echo "Setting up Git hooks..."
	@mkdir -p .git/hooks
	@cp scripts/pre-commit .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Git pre-commit hook installed successfully!"
.PHONY: setup

# Setup Python virtual environment with uv
setup-python:
	@echo "Setting up Python virtual environment with uv..."
	@command -v uv >/dev/null 2>&1 || { echo "Error: uv is not installed. Install it from https://docs.astral.sh/uv/getting-started/installation/"; exit 1; }
	uv venv .venv --python 3.13
	@echo "Python virtual environment created at .venv"
	@echo "Run 'direnv allow' to automatically activate the virtual environment"
.PHONY: setup-python

# Build all workspace members
build:
	cargo build --workspace
.PHONY: build

# Test all workspace members
test:
	docker compose -f docker-compose.test.yml up -d
	curl -s -X POST \
		-H "Content-Type: application/json" \
		-d '{"name":"test-bucket"}' \
		http://localhost:4443/storage/v1/b?project=test-project
	cargo test --workspace
	docker compose -f docker-compose.test.yml down -v
.PHONY: test

# Format code
fmt:
	cargo fmt --all
.PHONY: fmt

# Check code formatting (for CI)
fmt-check:
	cargo fmt --all -- --check
.PHONY: fmt-check

# Run clippy linter
lint:
	cargo clippy --workspace -- -D warnings
.PHONY: lint

# Auto-fix formatting and clippy issues
fix:
	cargo fmt --all
	cargo clippy --workspace --fix --allow-dirty
.PHONY: fix

# Run cargo check
check:
	cargo check --workspace
.PHONY: check

# Clean build artifacts
clean:
	cargo clean
.PHONY: clean

# Run services
run-locator:
	cargo run locator --config-file-path example_config_locator.yaml
.PHONY: run-locator

run-locator-gcs:
	cargo run locator --config-file-path example_config_locator_gcs.yaml

run-proxy:
	cargo run proxy --config-file-path example_config_proxy.yaml
.PHONY: run-proxy

run-ingest-router:
	cargo run ingest-router --config-file-path example_config_ingest_router.yaml
.PHONY: run-ingest-router

run-mock-control-api:
	python scripts/mock_control_api.py
.PHONY: run-mock-control-api

# CI-like checks (what runs in GitHub Actions)
ci: fmt-check lint test build
	@echo "All CI checks passed!"
.PHONY: ci

# Server for testing proxy locally
run-echo-server:
	python scripts/echo_server.py
.PHONY: run-echo-server

# Build Docker image for local testing
build-docker-local:
	docker buildx build --platform linux/amd64 -t synapse --load .
.PHONY: build-docker-local

# Run locally built docker image, for testing locally
run-docker-proxy-local:
	docker run \
	--platform linux/amd64 \
	-v $(PWD)/example_config_proxy.yaml:/app/example_config_proxy.yaml \
	--rm synapse proxy \
	--config-file-path example_config_proxy.yaml
.PHONY: run-docker-local
