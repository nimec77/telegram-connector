# Development Conventions

## TDD Workflow

`RED → GREEN → REFACTOR` — write a failing test first, write minimal code to pass, refactor
while green. **No production code without a failing test.**

## KISS Principles

| Principle | Rule |
|-----------|------|
| Single Responsibility | One module = one purpose; compose small functions, no god functions |
| No Premature Abstraction | Extract only after the Rule of Three (third duplication) |
| Explicit over Implicit | Clear signatures; defaults as named constants, not magic numbers |
| Flat over Nested | Prefer flat module structure; early returns / `let-else` guard clauses over nesting |
| Delete over Comment | Remove dead code, don't comment it out (git has history) |
| Minimal Dependencies | Don't add a crate for what a 3-line function can do |

## Code Style

- **Format:** `cargo fmt` (default settings); run `cargo fmt --all` after every change
- **Lint:** `cargo clippy -- -D warnings`
- **Naming:** snake_case functions, PascalCase types
- **Docs:** `///` on public API only
- **Line length:** 100 characters

## Module System

**No `mod.rs` files.** File-as-module pattern: `src/mcp.rs` declares `pub mod server;` etc.,
submodules live in `src/mcp/`. Large `#[cfg(test)]` blocks move to `#[path]`-included sibling
test files (`src/mcp/tests/*.rs`, `src/telegram/tests/*.rs`, `src/config/tests.rs`).

## Error Handling

| Layer | Crate | Usage |
|-------|-------|-------|
| Library | `thiserror` | Typed error variants in `src/error.rs` |
| Application | `anyhow` | `.context("...")` at each layer + `?` propagation |

- **NEVER `unwrap()`** in production code; never swallow errors with `unwrap_or_default()`
- No stringly-typed errors (`Result<_, String>`) — use the typed enum
- `expect()` with a clear message only in tests or truly impossible situations
- Handle `Option` via `.context(...)`/`ok_or`, not `.unwrap()`

## Domain Types (DDD)

Wrap primitives in newtypes — `ChannelId(i64)`, `MessageId(i64)`, `UserId(i64)`,
`Username(String)`, `ChannelName(String)` — so the compiler prevents argument mix-ups.
Put behavior on the type (`Message::link()`), don't scatter free functions over anemic data.

## Trait-Based Abstraction

External dependencies live behind traits for testability: `TelegramClientTrait` /
`RateLimiterTrait` with `#[cfg_attr(test, mockall::automock)]` + `#[async_trait]`.
Shared state via `Arc<T>`; `McpServer<T, R>` composes `Arc<T>` + `Arc<R>` (generic DI, mocks
in tests). Concrete types elsewhere — no generics until proven necessary.

## Testing

| Type | Location | Tools |
|------|----------|-------|
| Unit | `#[cfg(test)]` / `#[path]` sibling files | `#[test]`, `mockall` |
| Integration | `tests/` directory | `tokio::test` |
| Property-based | Within unit/integration | `proptest` |

- **Coverage target:** 80%+
- Test public behavior, not private internals (internals-based tests break on refactor)
- Don't write tests that only exercise the mock — they test nothing

## Logging

- Use `tracing` for structured async logging
- **Never log:** phone numbers, API hashes, passwords, session tokens
- Use the redaction helpers (`logging::redact_phone`) for sensitive data

## Git Commits

Branch, commit, and open PRs freely (in-house flow, no approval gate).

- Small, atomic, one logical change; imperative mood
- Format: `<type>: <description>` — types: `feat`, `fix`, `test`, `refactor`, `docs`, `chore`

## Pre-Commit Checklist

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

All must pass before commit.
