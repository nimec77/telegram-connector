# Telegram MCP Connector

Rust workspace for working with Telegram over MTProto. It includes the local
stdio connector, shared Telegram operations, a hosted HTTP API, and a remote
MCP gateway. Codex is the primary development agent.

## Start here

- Read `docs/memory.md` (Current state and Open items) before working.
- Follow [docs/rust-style.md](docs/rust-style.md), the authoritative Rust style policy,
  including its scoped adoption exceptions. Read `docs/conventions.md` for development
  commands/test locations and `docs/codex.md` for agent setup.
- Use the nightly toolchain selected by `rust-toolchain.toml`. Rust edition 2024
  does not itself select nightly. CI also uses nightly.
- Run `just` to list recipes. `just check` is the local quality gate:

  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --locked --all-targets -- -D warnings
  cargo test --workspace --locked
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --locked --no-deps
  ```

- Offline tests use mocks; Telegram credentials and a live session are not required.
  Use `--offline` as well as `--locked` when dependencies are already cached.
- Run local agent validation on macOS. Linux setup and checks are owner-managed unless
  explicitly requested; see the validation ownership policy in `docs/memory.md`.
- Build the stdio connector with `cargo build --locked --release -p telegram-connector`.
  Authentication with `telegram-mcp --setup` is a separate, interactive operation.
  For API and gateway builds and isolation checks, see the
  [hosted service guide](docs/hosted-services.md) and
  [development conventions](docs/conventions.md#hosted-verification).

## Source navigation

Use `ast-index` for codebase orientation and structural source navigation. Consult
the installed ast-index skill when available.

- Before the first indexed search, run `ast-index stats` from the project root.
  If no source index exists, run `ast-index rebuild`; otherwise `ast-index update`.
  Update again after source edits or Git operations that change the checkout,
  before the next indexed query.
- Use `explore` for intent; `search`, `symbol`, or `class` for identifiers;
  `usages`, `refs`, `callers`, and `implementations` for relationships.
- Use `outline` before reading a large source file, then read the relevant range.
- The symbol graph is separate: run `ast-index graph build` after index changes
  before relying on graph queries. Build it only when needed.
- Use `rg --files` for paths and `rg` for literals, comments, regex, configuration,
  non-code files, unsupported types, or empty index results.
- Index results can omit excluded files, macro-generated symbols, unsupported
  syntax, or hits beyond result limits. Check coverage before declaring a symbol
  absent or unused. `mockall` and rmcp proc-macro output is not indexed.
- Respect `.gitignore` and `.ast-index.yaml`. Use `--no-ignore`/`--force` only for
  task-required files with a clear scope; use `--walk-up` only for an intentional
  monorepo root index. Do not duplicate successful searches without a coverage reason.
- If ast-index is unavailable or fails, report the limitation briefly and use
  `rg` plus targeted reads. Pass these rules to delegated agents doing source work.

## Architecture and change placement

- The root `Cargo.toml` defines a virtual workspace with `telegram-connector` as
  its default member. All packages live under `crates/`.
- `crates/telegram-connector` owns the `telegram-mcp` stdio binary. Its `src/lib.rs`
  preserves public connector imports, including core reexports. Its `src/main.rs`
  handles CLI setup, signals, and stdio. Keep stdout exclusively for MCP messages
  and send diagnostics through `tracing` to stderr or a log file.
- Shared Telegram operations, rate limiting, domain types, and native stream
  primitives live in `crates/telegram-core`. Keep transport adapters out of core.
- `crates/telegram-api` owns HTTP v1 contracts, hosted identity, account and
  session ownership, grants, and SSE streams. The `service` feature enables
  the stateful implementation. [HTTP contract](crates/telegram-api/openapi/v1.yaml)
- `crates/telegram-mcp-http` owns the hosted tool catalog and Streamable HTTP
  transport. It uses the API's `identity` and `schemas` features in production
  and delegates operations over HTTP. Keep the API service, SQLite, grammers,
  and the stdio connector out of the gateway's production dependency graph.
- Stdio `McpServer<T: TelegramClientTrait, R: RateLimiterTrait>` composes `Arc<T>` and
  `Arc<R>`. Production uses `TelegramClient`/`RateLimiter`; tests use mockall mocks.
- In the connector crate, keep all stdio tool entry points in the single `#[tool_router]`
  impl in `src/mcp/server.rs`. The macro scans the impl before declarative macro
  expansion. Keep entry points thin and operation bodies in `src/mcp/server/impl_*.rs`.
- Stdio request and response types live in the connector's `src/mcp/tools/types/`.
  Requests derive `Deserialize` and schemars **v1** `JsonSchema`. Wire routes return
  `CallToolResult` with canonical typed JSON in `structuredContent` and matching text,
  followed by media image blocks. Explicit legacy mode retains historical text shapes.
  Direct Rust convenience methods remain legacy adapters. [Stdio contract](docs/tool-contract.md)
- The client seam is `crates/telegram-core/src/telegram/trait_def.rs`. Dispatch in
  that crate's `src/telegram/client.rs` and implement RPCs in `src/telegram/client/ops_*.rs`.
- Domain newtypes (`ChannelId`, `MessageId`, `UserId`, `Username`, `ChannelName`)
  live in `crates/telegram-core/src/telegram/types/`. Keep flexible scalar coercion
  at the stdio MCP boundary in the connector's `src/mcp/tools/types/serde_helpers.rs`.
- Use file-as-module layout without `mod.rs` and sibling `#[path]` files for large unit
  tests. Stdio MCP tests live in the connector's `src/mcp/tests/`. The connector
  and core each have a `src/test_helpers.rs`. Integration tests live under each
  member's `tests/`.
- Config expansion tests use injected lookups; actual environment/path loader tests
  run in subprocesses configured through `Command::env`/`env_remove`. Never mutate the
  parent process environment. Global logging initialization is also subprocess-isolated.
- Stdio resolves its TOML configuration from `--config`, then `TELEGRAM_MCP_CONFIG`,
  then the platform config directory. It supports `${VAR}` expansion. Hosted
  binaries take separate JSON configuration files with secrets supplied through
  the environment. See [README](README.md#configuration) and the
  [hosted service guide](docs/hosted-services.md).

## Dependencies and implementation rules

- Keep all three `grammers` dependencies pinned to the same Codeberg revision.
  Do not use the stale GitHub mirror or casually regenerate `Cargo.lock`.
- Stdio supports MCP 2026-07-28 and the 2025-11-25 `initialize` handshake. The
  gateway serves MCP 2026-07-28 over Streamable HTTP. Gate version-specific stdio
  response fields on the negotiated protocol and keep the connector's loopback HTTP
  SDK feature in dev dependencies. [Temporary Chrono-optional rmcp pin](docs/dependency-status.md#temporary-rmcp-pin--2026-11-20)
- Apply the manual’s error/secret rules (§5), behavior-test workflow (§9), and
  checked-in formatter settings (§1). Never commit config/session files. The existing
  canonical MCP boundary maps typed errors to safe `ToolError` values; legacy adapters
  preserve historical string output.
- The manual’s adoption exceptions preserve extracted test globs, the macro router
  layout, established fallible `new` APIs, and conservative diagnostic wrappers; they do
  not authorize new violations outside those scopes. Consuming server builders have
  `must_use`; selected Cargo lints and warning-free rustdoc enforce the cleaned scopes.
- Keep changes focused and add no dependency for an operation existing code can
  perform simply.

## Workflow and skills

- Act on the authorized task without routine approval checkpoints. Ask when a
  missing requirement changes the outcome or an action needs additional authority.
- Use feature branches; use a worktree when isolation is needed. Preserve unrelated
  work. Use conventional commits (`feat:`, `fix:`, `refactor:`, `docs:`, `chore:`).
- When Superpowers is installed, use its planning, TDD, debugging, review, and
  verification workflows as appropriate. This repository's no-routine-checkpoint
  workflow takes precedence over generic skill approval pauses.
- Repository skills in `.agents/skills/` are discovered by Codex.
  Use `$bump-version` for patch, minor, major, or explicit version requests. Read
  the release version from `crates/telegram-connector/Cargo.toml` and keep all four
  package versions, versioned internal dependency requirements, the root lockfile,
  changelog, and README in sync.
- Delegate independent bounded tasks when available and authorized; assign file
  ownership and acceptance criteria. Otherwise complete the work inline. Use only
  actual available tools/models; do not rely on Claude `Task`/`Skill` APIs.
- Before completion, run the relevant checks, inspect the diff, and report results
  and limitations. Obtain independent review when available before merging.
- For GitHub operations, load the installed `gh` skill and use GitHub CLI: native
  subcommands first, then `gh api`. Use `git` for local work and fetch/pull/push.
- `docs/memory.md` stores durable project knowledge, not session transcripts.
  Dedupe facts and remove stale ones. Do not put project knowledge in global memory.
- Active designs/plans belong in `docs/superpowers/specs/` and
  `docs/superpowers/plans/`. Delete the corresponding files when their change
  merges; Git history is the archive.

## Code Review Rules

- Local validation must precede rate-limit acquisition; every RPC follows it.
  Fan-out charges each attempted channel and does not refund failed channels.
- Preserve per-channel pagination semantics, response byte budgets, partial-result
  metadata, and per-item errors. Message IDs are unique only within a channel.
- Keep stdio tool schemas self-contained. Request enums use `#[schemars(inline)]`
  where needed because advertised input schemas do not carry `$defs`.
- Keep every network path bounded by its configured timeout. Preserve the search
  deadline and paused-time tests; do not introduce unbounded retries.
- Preserve the image budgets, binary-content handling, and secret redaction paths.
  Read the relevant decisions in `docs/memory.md` before changing these behaviors.
