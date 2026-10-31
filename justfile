# List available recipes
default:
    @just --list

# Debug build
build:
    cargo build --locked

# Release build
release:
    cargo build --locked --release

# Run all tests
test:
    cargo test --locked

# Format all code
fmt:
    cargo fmt --all

# Check formatting without writing
fmt-check:
    cargo fmt --all --check

# Clippy with warnings as errors
lint:
    cargo clippy --locked --all-targets -- -D warnings

# Pre-commit gate: fmt-check + clippy + all tests
check: fmt-check lint test

# Run the MCP server binary
run:
    cargo run --locked --bin telegram-mcp
