# List available recipes
default:
    @just --list

# Debug build
build:
    cargo build

# Release build
release:
    cargo build --release

# Run all tests
test:
    cargo test

# Run config tests serially (env var mutation)
test-config:
    cargo test config -- --test-threads=1

# Format all code
fmt:
    cargo fmt --all

# Check formatting without writing
fmt-check:
    cargo fmt --check

# Clippy with warnings as errors
lint:
    cargo clippy -- -D warnings

# Pre-commit gate: fmt-check + clippy + all tests
check: fmt-check lint test

# Run the MCP server binary
run:
    cargo run --bin telegram-mcp
