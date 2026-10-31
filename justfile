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

# Verify repository agent helper scripts (Python 3 + Bash)
skills-check:
    python3 .agents/skills/telegram-changelog/scripts/test_history.py
    bash -n .agents/skills/telegram-changelog/scripts/commits_since_release.sh

# Run the MCP server binary
run:
    cargo run --locked --bin telegram-mcp
