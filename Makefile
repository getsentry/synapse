# Makefile for Synapse Rust workspace

.PHONY: help setup build test clean fmt fmt-check lint fix check dev-locator dev-proxy run-locator run-proxy

# Default target
help:
	@echo "Available targets:"
	@echo "  setup        - Install development dependencies"
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
	@echo "  dev-locator  - Run locator with auto-reload"
	@echo "  dev-proxy    - Run proxy with auto-reload"

# Setup development environment
setup:
	rustup update
	cargo install cargo-watch
	@echo "Setting up Git hooks..."
	@mkdir -p .git/hooks
	@cp scripts/pre-commit .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Git pre-commit hook installed successfully!"

# Build all workspace members
build:
	cargo build --workspace

# Test all workspace members
test:
	cargo test --workspace

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
	cargo run --bin locator

run-proxy:
	cargo run --bin proxy

# Development with auto-reload (requires cargo-watch)
dev-locator:
	cargo watch -x "run --bin locator"

dev-proxy:
	cargo watch -x "run --bin proxy"

# CI-like checks (what runs in GitHub Actions)
ci: fmt-check lint test build
	@echo "All CI checks passed!"

