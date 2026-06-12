# Design: Justfile for the Core Dev Loop

## Summary

The canonical build/test/lint commands live only as prose in `CLAUDE.md`
("Build & Test Commands"), so running the pre-commit gate means copy-pasting a
three-command `&&` chain. This change adds a `justfile` at the repo root that
wraps the core dev loop in named recipes, with `just check` as the single
entry point for the pre-commit gate. `just` 1.52.0 is already installed via
Homebrew; there is no existing `justfile` or `Makefile` to conflict with.

Scope is deliberately minimal ("core dev loop", per design discussion): only
the commands already documented in `CLAUDE.md`. No watch mode, doc generation,
ast-index helpers, or parameterized test runner.

## Approach

**Composed recipes (selected).** Each command is its own recipe; `check` is a
dependency chain (`check: fmt-check lint test`). `just` stops at the first
failing dependency, which is the same semantics as the documented
`cargo fmt --check && cargo clippy -- -D warnings && cargo test` chain.

### Approaches considered

- **A — Composed recipes (selected):** idiomatic just, no duplicated command
  strings, recipes are individually reusable, `just --list` self-documents.
- **B — Verbatim mirror:** `check` copies the exact `&&` chain from `CLAUDE.md`
  as one shell line. Exact textual parity with the docs, but duplicates the
  commands across recipes so they can drift. Rejected.

## Recipes

`justfile` at the repo root. Running bare `just` lists recipes (default
recipe). Each recipe carries a one-line doc comment so `just --list` reads
well.

| Recipe | Command | Notes |
|--------|---------|-------|
| `default` | `@just --list` | bare `just` lists recipes |
| `build` | `cargo build` | debug build |
| `release` | `cargo build --release` | release build |
| `test` | `cargo test` | all tests, same as the gate |
| `test-config` | `cargo test config -- --test-threads=1` | serial run for env-var-mutating config tests |
| `fmt` | `cargo fmt --all` | format (the post-edit convention) |
| `fmt-check` | `cargo fmt --check` | format check only |
| `lint` | `cargo clippy -- -D warnings` | warnings = errors |
| `check` | depends on `fmt-check lint test` | the pre-commit gate |
| `run` | `cargo run --bin telegram-mcp` | run the MCP server |

`test` stays plain `cargo test` (exactly what the pre-commit gate runs — the
non-ignored config tests use distinct env vars per test and do not race);
`test-config` exists separately for the serial invocation documented in
`CLAUDE.md`, rather than bolting it onto `test` and double-running the config
module.

## Documentation

`CLAUDE.md`'s "Build & Test Commands" section gains a one-line mention that
the same commands are available as `just` recipes and that `just check` runs
the pre-commit gate. The raw cargo commands stay listed — `just` is a
convenience layer, not a replacement, and contributors without `just`
installed lose nothing.

## Testing

No Rust code changes, so no unit tests. Verification is behavioral:

- `just --list` shows all recipes with their doc comments.
- `just check` passes end-to-end on a clean tree (this *is* the pre-commit
  gate, so it doubles as the gate run for this change).
