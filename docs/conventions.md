# Development Conventions

The [Rust coding manual](rust-style.md) is authoritative for Rust style, API design,
errors, ownership, concurrency, documentation, and tests. Its adoption exceptions cover
extracted test globs, the rmcp router layout, established fallible constructors, and
documented conservative diagnostic wrappers. Production imports now name their owning
modules explicitly. Follow [AGENTS.md](../AGENTS.md) for repository workflow, source
navigation, dependency constraints,
and review invariants. The [Rust API inventory](rust-api.md) classifies supported,
transitional and internal surfaces and records error/deadline/compatibility contracts.

## Development workflow

Write a failing observable behavior test before changing production behavior, then
implement and refactor while green (manual §9). Documentation and configuration changes
need relevant validation rather than tests that match their wording. Keep changes focused;
choose abstractions and dependencies for a concrete need.

## Test locations

| Type | Location | Tools |
| --- | --- | --- |
| Unit | `#[cfg(test)]` / `#[path]` sibling files | `#[test]`, `mockall` |
| Integration | `tests/` inside the package under test | `tokio::test` |
| Property-based | Within unit/integration tests | `proptest` |

Stdio MCP tests live in `crates/telegram-connector/src/mcp/tests/`. The connector
and core each have a `src/test_helpers.rs`. The hosted API route fixtures live
alongside the service modules in `crates/telegram-api/src/service/`. The account
lifecycle and gateway process tests live in their respective member `tests/` directories.
Environment expansion uses pure injected lookups in tests. Full environment/path loaders
and global logging initialization run in re-executed test subprocesses configured by
`Command::env`/`env_remove`; parent tests invoke each ignored child entrypoint. No test
mutates its own process environment. Intentional poison-thread/child panics verify refused
admission and process isolation; join/capture their outcomes deterministically.
The coverage target is 80%+. Prioritize observable production decisions and invariants;
a successful mock expectation alone is insufficient evidence.

## Formatting and quality gate

`rust-toolchain.toml` selects nightly with rustfmt and clippy. Run `cargo fmt --all`
after Rust changes. The checked-in `rustfmt.toml` implements manual §1. From the
virtual workspace root, unqualified Cargo build and test commands select the
default member `telegram-connector`. `just check` checks the whole workspace,
using the same Rust commands as CI.

```bash
cargo fmt --all --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --locked --no-deps
```

`cargo test` runs doctests too; `just check-docs` runs focused warning-free rustdoc and
explicit doctests. Cargo warns on wildcard imports, missing error contracts and consuming
`Self` returns without `must_use`; Clippy's `-D warnings` enforces these selected lints.
`missing_docs` remains module-scoped, `must_use_candidate` advisory, and project targets
forbid unsafe code. See manual §9 for exact scope and legitimate compatibility exceptions.

All must pass before commit. Use `--offline` for Cargo dependency resolution when
cached; tests use mocks and need no Telegram credentials or live session.

## Hosted verification

Run member builds separately to check production feature isolation.

```bash
cargo build --locked --offline --release -p telegram-connector
cargo build --locked --offline --release -p telegram-core
cargo build --locked --offline --release -p telegram-api --features service
cargo build --locked --offline --release -p telegram-mcp-http
cargo tree --locked --offline -p telegram-mcp-http -e normal,build,features --prefix none
```

The production gateway graph uses the API's `identity` and `schemas` features.
It excludes the API's `service` feature, SQLite, grammers, `telegram-core`, and
`telegram-connector`. The gateway integration tests enable the API service in
their development dependencies. The API and gateway fixtures use numeric loopback
sockets for issuer, HTTP, SSE, and MCP traffic, with disposable roots for SQLite
and session data. To run the hosted suites and owner page fixture individually, use
these commands from the workspace root.

```bash
cargo test --locked --offline -p telegram-api --features service
cargo test --locked --offline -p telegram-mcp-http --test mcp_http
node --test crates/telegram-api/tests/account_page.cjs
```

The [operator guide](hosted-services.md) describes production configuration.

## Git commits

Branch, commit, and open PRs within the authorized task without routine approval gates.
Preserve unrelated work and make small, atomic commits with an imperative description.
Use `<type>: <description>` (`feat`, `fix`, `test`, `refactor`, `docs`, or `chore`).
Keep policy/formatting-only commits separate from behavioral changes (manual §1 and §9).
Codex setup and optional development tools are documented in [codex.md](codex.md).
