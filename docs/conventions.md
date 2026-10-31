# Development Conventions

The [Rust coding manual](rust-style.md) is authoritative for Rust style, API design,
errors, ownership, concurrency, documentation, and tests. Its adoption exceptions cover
extracted test globs, the rmcp router layout, established constructors/builders, and
pending S2 failure/safety and S4 import migrations. Follow [AGENTS.md](../AGENTS.md)
for repository workflow, source navigation, dependency constraints, and review invariants.

## Development workflow

Write a failing observable behavior test before changing production behavior, then
implement and refactor while green (manual §9). Documentation and configuration changes
need relevant validation rather than tests that match their wording. Keep changes focused;
choose abstractions and dependencies for a concrete need.

## Test locations

| Type | Location | Tools |
| --- | --- | --- |
| Unit | `#[cfg(test)]` / `#[path]` sibling files | `#[test]`, `mockall` |
| Integration | `tests/` directory | `tokio::test` |
| Property-based | Within unit/integration tests | `proptest` |

MCP tests live in `src/mcp/tests/`; shared fixtures live in `src/test_helpers.rs`.
The coverage target is 80%+. Prioritize observable production decisions and invariants;
a successful mock expectation alone is insufficient evidence.

## Formatting and quality gate

`rust-toolchain.toml` selects nightly with rustfmt and clippy. Run `cargo fmt --all`
after Rust changes; the checked-in `rustfmt.toml` implements manual §1. `just check`
runs the same gate as CI:

```bash
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

All must pass before commit. Use `--offline` for Cargo dependency resolution when
cached; tests use mocks and need no Telegram credentials or live session.

## Git commits

Branch, commit, and open PRs within the authorized task without routine approval gates.
Preserve unrelated work and make small, atomic commits with an imperative description.
Use `<type>: <description>` (`feat`, `fix`, `test`, `refactor`, `docs`, or `chore`).
Keep policy/formatting-only commits separate from behavioral changes (manual §1 and §9).
Codex setup and optional development tools are documented in [codex.md](codex.md).
