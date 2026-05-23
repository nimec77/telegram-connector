# Dependency Updates — Design

**Date:** 2026-05-23
**Status:** DESIGN_PROPOSED
**Scope:** Bump every direct dependency in `Cargo.toml` to the highest stable version available on crates.io as of 2026-05-23. `grammers-*` stays on git master and is out of scope.

## Goals

- Bring every direct dependency to its latest stable release.
- Keep the manifest idiomatic: leave caret specs that already resolve to the latest (e.g. `serde = "1.0"`) loose; only edit specs where the current spec lags the latest release.
- Land changes in small, bisectable commits so a regression in any single crate can be reverted in isolation.

## Non-Goals

- Updating `grammers-{client,session,mtsender}`. They track `master` and a `cargo update` already picks up upstream changes.
- Tightening loose caret specs (e.g. `serde = "1.0"` → `"1.0.228"`) for crates already on the latest patch. Idiomatic Rust leaves these loose.
- Toolchain or edition changes.
- Adding, removing, or replacing any crate.

## Current state vs. latest

| Crate | Current spec | Latest stable | Bump |
|---|---|---|---|
| rmcp | 1.6.0 | 1.7.0 | **minor** |
| tokio | 1.52.3 | 1.52.3 | none |
| async-trait | 0.1 | 0.1.89 | patch (resolved via caret) |
| dialoguer | 0.12.0 | 0.12.0 | none |
| clap | 4.6.1 | 4.6.1 | none |
| toml | 1.1.2 | 1.1.2 | none |
| serde | 1.0 | 1.0.228 | patch (resolved via caret) |
| serde_json | 1.0.149 | 1.0.150 | patch |
| schemars | 1.2.1 | 1.2.1 | none |
| directories | 6.0.0 | 6.0.0 | none |
| tracing | 0.1.44 | 0.1.44 | none |
| tracing-subscriber | 0.3 | 0.3.23 | patch (resolved via caret) |
| tracing-appender | 0.2.5 | 0.2.5 | none |
| anyhow | 1.0.101 | 1.0.102 | patch |
| thiserror | 2.0.18 | 2.0.18 | none |
| chrono | 0.4.43 | 0.4.44 | patch |
| dashmap | 6.1.0 | 6.2.1 | **minor** |
| secrecy | 0.10 | 0.10.3 | patch (resolved via caret) |
| tokio-test (dev) | 0.4.5 | 0.4.5 | none |
| tempfile (dev) | 3.25.0 | 3.27.0 | **minor** |
| proptest (dev) | 1.11.0 | 1.11.0 | none |
| mockall (dev) | 0.14.0 | 0.14.0 | none |
| filetime (dev) | 0.2.28 | 0.2.29 | patch |

**Aggregate:** 0 major, 3 minor, 8 patch, 12 already current. Every caret spec that already resolves to latest will be advanced purely via `cargo update`.

## Approach: phased in three commits

Each phase is independent and ends with the full pre-commit gate: `cargo fmt --check && cargo clippy -- -D warnings && cargo test` (config tests with `--test-threads=1` per `CLAUDE.md`).

### Phase A — Patches

Edit `Cargo.toml` for the four spec strings that lag their latest patch:

- `serde_json = "1.0.149"` → `"1.0.150"`
- `anyhow = "1.0.101"` → `"1.0.102"`
- `chrono = { version = "0.4.43", ... }` → `version = "0.4.44"`
- `filetime = "0.2.28"` → `"0.2.29"` (dev)

Then `cargo update` to pull the latest patch for caret-spec'd crates (`serde`, `async-trait`, `tracing-subscriber`, `secrecy`) and re-resolve everything else.

**Risk:** trivially low. Patch releases on these crates are bug-fix only.

### Phase B — Non-`rmcp` minors

- `dashmap = "6.1.0"` → `"6.2.1"` — maintenance release; raises MSRV to 1.85 (non-issue on nightly).
- `tempfile = "3.25.0"` → `"3.27.0"` (dev) — 3.x line has been API-stable; affects test code only.

**Risk:** low. Both crates are used through narrow surfaces in this project (a few `DashMap::new` / `Entry` patterns and `tempfile::NamedTempFile` in tests).

### Phase C — `rmcp` 1.6 → 1.7

Single-crate phase, isolated for easy revert.

`rmcp` underpins the MCP layer in three load-bearing places:

- `#[tool_router] impl` block in `src/mcp/server.rs` containing all 8 tools.
- `#[tool(...)]` attribute macros on each tool method (descriptions are strings inside attributes — invisible to `ast-index`, must `Grep` if needed).
- `InitializeResult::new(capabilities).with_server_info(...).with_instructions(...)` builder chain.

**Verification steps:**

1. Edit `rmcp = { version = "1.6.0", features = ["server"] }` → `version = "1.7.0"`.
2. `cargo build` — first signal. Any macro-API drift surfaces here.
3. Scan the rmcp 1.7 changelog (or commit log on `modelcontextprotocol/rust-sdk`) for `#[tool_router]`, `#[tool(...)]`, `InitializeResult`, `ServerCapabilities`. Apply call-site edits if needed.
4. `cargo clippy -- -D warnings` — catches any newly-deprecated APIs.
5. `cargo test` — exercises every MCP tool via the mock-based suite in `src/mcp/tests/`.

**Risk:** medium-low. 1.6→1.7 is a minor bump and rmcp follows semver, but the macro surface this project uses is unusually deep, so the changelog scan is mandatory.

## Verification per phase

After each phase:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo test config -- --test-threads=1   # config tests can race on env-var mutation
```

Only proceed to the next phase once all four pass.

## Rollback

Each phase is a single commit (user-managed per `CLAUDE.md`). Reverting any phase is `git revert <sha>` plus `cargo update` to re-converge `Cargo.lock`.

## Out-of-scope follow-ups (not part of this work)

- A future ticket can pin `grammers-*` to a specific commit if reproducibility becomes a concern.
- `serde`, `tracing-subscriber`, `async-trait`, `secrecy` caret specs can be tightened to exact pins later if the project adopts a stricter version-pinning policy.

## Acceptance criteria

- [ ] Every dependency in `Cargo.toml` and `Cargo.lock` is at the latest stable version available on crates.io as of 2026-05-23 (excluding `grammers-*`).
- [ ] `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` all pass on the final commit.
- [ ] Each phase is a separate commit with a descriptive message.
- [ ] No call-site changes outside what's strictly required by the bumps (no incidental refactors).
