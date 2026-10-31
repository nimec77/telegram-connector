# Telegram MCP Connector

Rust MCP server for searching and reading Telegram channels over MTProto. Codex is
the primary development agent; the stdio service supports any compatible MCP client.

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
  cargo clippy --locked --all-targets -- -D warnings
  cargo test --locked
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
  ```

- Offline tests use mocks; Telegram credentials and a live session are not required.
  Use `--offline` as well as `--locked` when dependencies are already cached.
- Build the server with `cargo build --locked --release`. Authentication with
  `telegram-mcp --setup` is a separate, interactive operation.

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

- `src/lib.rs` exports modules; `src/main.rs` handles CLI setup, signals, and stdio.
  Keep stdout exclusively for MCP messages; diagnostics use `tracing`/stderr.
- `McpServer<T: TelegramClientTrait, R: RateLimiterTrait>` composes `Arc<T>` and
  `Arc<R>`. Production uses `TelegramClient`/`RateLimiter`; tests use mockall mocks.
- All tool entry points stay in the single `#[tool_router] impl` in
  `src/mcp/server.rs`. The macro scans the impl before declarative macro expansion.
  Keep entry points thin; operation bodies live in `src/mcp/server/impl_*.rs`.
- Request/response types live in `src/mcp/tools/types/`. Requests derive
  `Deserialize` and schemars **v1** `JsonSchema`. Most tools return JSON strings;
  media tools return `CallToolResult` for image content blocks.
- `src/telegram/trait_def.rs` is the client seam. Extend the trait, dispatch in
  `src/telegram/client.rs`, and implement operations in `client/ops_*.rs`.
- Domain newtypes (`ChannelId`, `MessageId`, `UserId`, `Username`, `ChannelName`)
  live in `src/telegram/types/`. Keep flexible scalar coercion at the MCP boundary
  in `serde_helpers.rs`; the domain remains strict.
- File-as-module layout throughout; no `mod.rs`. Large tests use sibling files
  via `#[path]`. MCP tests live in `src/mcp/tests/`; shared fixtures in
  `src/test_helpers.rs`.
- Config expansion tests use injected lookups; actual environment/path loader tests
  run in subprocesses configured through `Command::env`/`env_remove`. Never mutate the
  parent process environment. Global logging initialization is also subprocess-isolated.
- Config resolution: `--config` → `TELEGRAM_MCP_CONFIG` → the platform config
  directory. See README for platform paths. Config supports `${VAR}` expansion.

## Dependencies and implementation rules

- Keep all three `grammers` dependencies pinned to the same Codeberg revision.
  Do not use the stale GitHub mirror or casually regenerate `Cargo.lock`.
- `rmcp` v3.8 supports MCP 2026-07-28 and the legacy `initialize` handshake.
  Gate version-specific response fields on the negotiated client protocol.
- Apply the manual’s error/secret rules (§5), behavior-test workflow (§9), and
  checked-in formatter settings (§1). Never commit config/session files. The existing
  MCP tool boundary converts errors to strings as required by its API.
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
  `$bump-version` updates the crate version, lockfile, and changelog for patch,
  minor, major, or explicit version requests.
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
- Keep tool schemas self-contained: request enums use `#[schemars(inline)]`
  where needed because advertised input schemas do not carry `$defs`.
- Keep every network path bounded by its configured timeout. Preserve the search
  deadline and paused-time tests; do not introduce unbounded retries.
- Preserve the image budgets, binary-content handling, and secret redaction paths.
  Read the relevant decisions in `docs/memory.md` before changing these behaviors.
