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

# Pre-commit gate: formatting, Clippy, all tests/doctests, and warning-free rustdoc
check: fmt-check lint test doc

# Run the MCP server binary
run:
    cargo run --locked --bin telegram-mcp

# Fresh security and full dependency declaration/source policy (tools installed separately)
security:
    cargo audit --deny unsound --deny yanked
    cargo deny --locked check sources advisories
    python3 scripts/check_dependency_policy.py

# Dependency policy's behavior fixtures (no dependency resolution required)
policy-test:
    python3 -m unittest discover -s scripts -p test_dependency_policy.py

# Warning-free public API documentation
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps

# Public API documentation and executable examples
check-docs: doc
    cargo test --locked --doc
