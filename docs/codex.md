# Developing with Codex

Codex is the primary development workflow for this Rust workspace. The local stdio
connector supports compatible MCP clients and can be developed without a Telegram
session. The workspace also contains shared Telegram operations, a stateful HTTP API,
and a separate Streamable HTTP MCP gateway. For hosted installation, see the
[operator guide](hosted-services.md).

All ten original roadmap tasks are implemented and accepted on `master`, including
typed outputs, runtime/media hardening, maintenance automation and Rust standards
alignment. See the [final acceptance evidence](validation/2026-11-01-final-acceptance.md)
and [changelog](../CHANGELOG.md). Optional cached resources and file export are
documented in the permanent
[media artifacts and DeepSeek guide](media-artifacts.md), including client prerequisites
and the limits of installed-adapter verification.

## Repository workflow

1. Read [AGENTS.md](../AGENTS.md) and [project memory](memory.md), including its Open
   items, before choosing work. Follow the [Rust manual](rust-style.md) and
   [development conventions](conventions.md).
2. Use a feature branch; use or reuse a worktree when isolation is needed. Preserve
   unrelated changes and keep Cargo.lock ownership explicit.
3. Write a failing behavior test before production logic changes. Documentation/config
   updates need relevant validation rather than tests that repeat their wording.
4. Verify the change and inspect the diff before committing. Obtain independent review
   before merging. Use conventional commit messages.

`AGENTS.md` is the shared instruction source; repository skills are in `.agents/skills/`.
Personal credentials, sessions and agent permissions stay outside version control. The
completed Codex migration and standards-alignment plans are archived in Git history.

## Local checks

`rust-toolchain.toml` selects `nightly-2026-10-31`, rustfmt and Clippy. Run the full
workspace gate from the repository root.

```bash
just check
```

The virtual workspace defaults to `telegram-connector` for Cargo commands that
select packages. Use `--workspace` to check all four members. These commands match
the `just` gate.

```bash
cargo fmt --all --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --locked --no-deps
```

Tests and doctests use offline fixtures/mocks and need no Telegram credentials. With
cached dependencies, add `--offline` to the Cargo compile/test/doc commands. Edition 2024
does not itself select nightly. The separate scheduled floating-nightly workflow does
not replace the pinned gate.

Agent checks run on macOS. Linux setup and validation are owner-managed if needed;
do not provision a VM/container or arrange Linux runs unless explicitly requested. The configured
CI matrix is retained. See [maintenance](maintenance.md) for security/dependency checks
and the distinction between configured workflows and actual run evidence.

## Source navigation and public APIs

Follow the ast-index workflow in AGENTS.md: stats, update/rebuild, then the appropriate
symbol/relationship query or outline. If its cache is unavailable, use `rg` and targeted
reads and report the limitation. Build the separate symbol graph only when needed.

The [Rust API inventory](rust-api.md) distinguishes supported, compatibility and internal
surfaces. Public direct tool methods retain legacy output; registered MCP wire routes use
the configured output mode. The library example at
`crates/telegram-connector/examples/inert_server.rs` runs without authentication,
Telegram RPC or desktop launch. Wire and process checks use the connector's
`offline_mcp_fixture` example. [Runtime guide](stdio-runtime.md)

## Use the connector from Codex

Build the stdio binary from the workspace root with
`cargo build --locked --release -p telegram-connector`. For a Cargo install, use
`cargo install --locked --path crates/telegram-connector` from the workspace root. Create a private
Telegram configuration and authenticate once.
[Configuration reference](../README.md#configuration)

```bash
./target/release/telegram-mcp --config /absolute/path/to/telegram-connector-config.toml --setup
```

For a Cargo installation, use `telegram-mcp` from your PATH in the setup
command and use its installed path in your Codex configuration.

Keep the config, session and logs outside version control. Merge the table from
[the Codex example](../.codex/config.toml.example) into `~/.codex/config.toml`, replacing
both absolute paths. Preserve existing tables; the example is not loaded automatically.

Alternatively, register the binary using the CLI:

```bash
codex mcp add telegram -- /absolute/path/to/telegram-connector/target/release/telegram-mcp --config /absolute/path/to/telegram-connector-config.toml
codex mcp list
```

Then add `startup_timeout_sec = 120` and `tool_timeout_sec = 180` to the generated server
table. Startup authorization and best-effort Premium RPCs each have a default 30-second
budget; optional media-root initialization adds bounded setup. Transcription and media
sending have total budgets of up to 120 seconds by default. The example client budgets
leave headroom. Restart the Codex session and use `/mcp` to check the connection.
See the [official MCP guide](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

Optional MCP servers have a separate initial-catalog wait, currently 1000 ms. If you
need Codex to wait for their full startup budgets, set `mcp_optional_startup_grace_ms = 0`
at the top level of your user config, before any table headers. This applies to all
optional servers and can delay startup. The server's `startup_timeout_sec` still bounds
its initialization; see the same official guide.

[Sending](sending.md) is disabled by default. Enable it explicitly for immediate account
writes, with separate prepared-source/staging roots for media; unknown delivery must not
be automatically retried.

Calls default to `json_v1`: read the envelope from `structuredContent` or its matching
text fallback, and inspect `data` or `error`. Applications parsing historical text can
select `[server] output_format = "legacy"` independently of protocol negotiation.
See the [contract guide](tool-contract.md) for ID types, error codes and media indices.

Live checks require an authenticated session and are separate from offline development.
If requested, start with cached `check_mcp_status`, then bounded channel reads; Telegram
operations spend the configured account tokens. `scripts/mcp_probe.py` supports modern
and legacy modes; use the offline fixture for credential-free verification.

## Version-bump skill

The checked-in `$bump-version` skill accepts patch, minor, major or an explicit target.
For example:

```text
Use $bump-version to bump the patch version.
```

The `[package].version` in `crates/telegram-connector/Cargo.toml` is the release
authority. Keep all four package versions, versioned internal path dependency
requirements, and the corresponding `Cargo.lock` entries aligned. Roll Unreleased
notes into a dated changelog section, leave a
fresh Unreleased section, and update README's current version and changelog link.
An empty Unreleased section still gets a dated version heading. An already-matching
target repairs stale metadata or documentation while preserving Unreleased notes
and historical release dates.

The bundled consistency checker covers the connector manifest, its entry in the
workspace lockfile, the changelog and README. Also check the member versions and
internal requirements with `cargo metadata --locked --format-version 1`, and run the
workspace Rust gate with `just check`.

Dry runs describe changes without editing. Preserve dependency pins, historical notes
and unrelated work. A normal bump commits its scoped metadata changes, creates an
annotated `v<version>` tag, and pushes only that tag to the selected GitHub remote
(default `origin`). It verifies the remote tag's commit and never moves an existing
version tag. A current-version request also repairs a missing tag. Use `local-only`
or `no-push` to stop after local validation; dry runs create no commits or tags.
Branch pushes, additional merges and GitHub Release publication remain separate
operations requiring their own scope.
