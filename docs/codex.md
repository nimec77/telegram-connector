# Developing with Codex

## What the migration changes

The connector is already a client-independent MCP stdio server built with rmcp.
It has no Anthropic API integration to replace. The migration changes development
instructions, reusable workflows, onboarding, and validation. Tool names, schemas,
Telegram authentication, and dependency pins stay compatible with existing clients.

The previous setup had a tracked `CLAUDE.md` and 21 ignored local skills under
`.claude/skills/`. Those skills were unavailable to a fresh clone and many required
Claude's `Task`, `Skill`, `AskUserQuestion`, `opus`/`sonnet`, or absent ticket files.
The local settings contained permission allowlists, no hooks. Personal permissions
are not portable project policy and have not been copied.

The old release skill described a Synapse workspace and read `[workspace.package]`.
This crate uses `[package]`. The replacement is a focused version-bump skill that
updates the package and lockfile together without imposing a release workflow.

`AGENTS.md` is now canonical. The former `CLAUDE.md` and `.claude/` directory have
been removed. Codex discovers root project instructions from `AGENTS.md` and
repository skills from
`.agents/skills/`. See the official [instruction discovery guide](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
and [skill discovery guide](https://learn.chatgpt.com/docs/build-skills).

## Develop the connector

1. Install Rust through rustup and optionally install `just`.
2. Start a new Codex session in the checkout. Read `AGENTS.md` and the Current state
   / Open items in `docs/memory.md` before changing code.
3. Build and verify:

   ```bash
   cargo build --locked
   just check
   ```

Without just, use the same commands CI runs:

```bash
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

`rust-toolchain.toml` selects nightly, rustfmt, and clippy, matching the existing
CI channel. It selects a channel rather than freezing a dated nightly. Edition
2024 alone does not choose a toolchain. Tests use mocks and require no Telegram
credentials. With cached dependencies, add `--offline` to Cargo build/clippy/test.
Do not initialize a live Telegram session just to run development checks.

For source navigation, follow the ast-index workflow in `AGENTS.md`. Its cache may
live outside the checkout; if the sandbox blocks it, use an authorized cache
operation or fall back to rg and targeted reads. No graph build is needed for
ordinary symbol searches.

## Use the connector from Codex

Development and consumption are separate: a Codex session can work on the Rust
code without launching the Telegram MCP server.

To consume Telegram tools, build with `cargo build --locked --release`, create a
private Telegram configuration as described in README, and authenticate once:

```bash
./target/release/telegram-mcp --config /absolute/path/to/telegram-connector-config.toml --setup
```

Keep the config, session, and logs outside version control. Merge the table from
[the Codex example](../.codex/config.toml.example) into `~/.codex/config.toml`,
replacing both absolute paths. Do not replace an existing config wholesale or
create duplicate `[mcp_servers.telegram]` tables. The example file itself is inert.

Alternatively, register the binary using the CLI:

```bash
codex mcp add telegram -- /absolute/path/to/telegram-connector/target/release/telegram-mcp --config /absolute/path/to/telegram-connector-config.toml
codex mcp list
```

Then add `startup_timeout_sec = 90` and `tool_timeout_sec = 180` to the generated
server table. Startup account RPCs can use two 30-second resolve budgets; 90 seconds
leaves headroom. Codex defaults to a 60-second tool timeout, while transcription can
take 120 seconds; the tool budget leaves headroom. Restart the Codex session and use
`/mcp` to check its active connection. These CLI/TOML settings follow the
[official MCP guide](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

As a manual smoke check, ask Codex to call `check_mcp_status`, then list a small
number of subscribed channels. Test searches/media only when needed; those tools
spend account rate-limit tokens. For protocol diagnosis, use the
[MCP Inspector](https://modelcontextprotocol.io/docs/tools/inspector). For measured
live calls, the existing `python3 scripts/mcp_probe.py <binary> <calls.json>`
provides round-trip timings. Both require an authenticated session for this server.

## Version-bump skill

The checked-in `$bump-version` skill needs no global installation. Start
a new Codex session to discover it, then request, for example:

```text
Use $bump-version to bump the patch version.
```

It accepts patch, minor, major, or an explicit target version; reads
`[package].version`; updates `Cargo.toml` and `Cargo.lock`; and runs the Rust gate.
Dry runs leave files untouched. Unrelated edits are preserved, and an empty
Unreleased section does not block a version-only bump. Release notes and publication
follow a separately requested release scope.

The skill is self-contained Markdown with Codex UI metadata. There are no helper
scripts or extra CI dependencies. Superpowers is useful when installed but is not
required to build, test, or use the repository skill.

## Where the former skills went

The former `.claude/` directory was removed after porting the useful workflows.
Shared development instructions now live in the Codex files. This is a deliberate
consolidation rather than a mechanical copy of overlapping orchestrators:

| Former local skills | Codex replacement |
| --- | --- |
| `analysis`, `research`, `plan`, `tasklist` | AGENTS.md plus Superpowers brainstorming/writing-plans when installed; otherwise an inline plan sized to the work. |
| `feature-development`, `dev-cycle`, `implement-orchestrated`, `phase-loop`, `quick-implement` | Superpowers executing-plans or subagent-driven-development; inline execution works without delegation. |
| `run-reviewer`, `qa`, `validate` | Relevant tests, `just check`, independent review, and Superpowers review/verification skills when available. |
| `docs-update`, `sync-phases` | Maintain the affected docs and one active plan checklist; no parallel ticket/phase state system. |
| `project-conventions` and the ast-index rule | Root AGENTS.md, docs/conventions.md, and docs/memory.md. |
| `changelog-from-commits`, `release` | Replaced by the focused `$bump-version` skill; broader release work follows the user's requested scope. |
| `rust-async-patterns`, `rust-best-practices`, `rust-learner`, `rust-refactor-helper` | Keep optional Rust references only when useful. Use current upstream crate docs and actual IDE/LSP capabilities; no copied tutorials or assumed Codex LSP tool. |

## Additional tools worth integrating

Keep the mandatory gate small. Add tools in response to a concrete development need;
none of the optional tools below is installed or enabled by this repository.

| Integration | Why it fits this project | Suggested use |
| --- | --- | --- |
| ast-index skill/CLI | Fast structural navigation across the trait seam and split tests. | Primary source navigation; already required by AGENTS.md. |
| Superpowers | Planning, debugging, TDD, and independent review without maintaining a separate ticket orchestrator. | Use installed skills as appropriate; preserve task authorization and the repository's autonomous workflow. |
| `gh` skill + [GitHub CLI](https://cli.github.com/manual/) | PR review, issue context, and Actions diagnostics. | Native gh commands, then gh api; Git remains the local checkout tool. |
| OpenAI Docs skill + [official Docs MCP](https://developers.openai.com/learn/docs-mcp) | Verified Codex/MCP setup instead of stale agent configuration assumptions. | Optional read-only server: `codex mcp add openaiDeveloperDocs --url https://developers.openai.com/mcp`. |
| [rust-analyzer](https://rust-analyzer.github.io/) | Compiler-aware editor navigation/completion complements ast-index's structural view. | Use in a supported editor; an installed language server is not automatically a Codex tool. |
| [cargo-llvm-cov](https://docs.rs/crate/cargo-llvm-cov/latest) | The durable backlog already distinguishes well-covered logic from the mocked production ops layer. | `cargo llvm-cov --locked --lib`; inspect the changed area instead of chasing a global percentage. |
| [cargo-audit / cargo-deny](https://rustsec.org/) | Advisory checks and, for cargo-deny, license/source/dependency policy. | Start with `cargo audit` when investigating dependency risk. Add a deny policy only after deciding which sources/licenses are acceptable. |
| [MCP Inspector](https://modelcontextprotocol.io/docs/tools/inspector) | Inspect actual schemas, content blocks, and protocol negotiation. | Live compatibility diagnosis after authentication; retain offline schema tests in the fast gate. |
| [cargo-nextest](https://nexte.st/) | Test isolation, filtering, and CI reporting if the suite becomes slow. | Optional after measuring a need; retain `cargo test --doc` separately because nextest does not run doctests. |

For delegated development, use only the tools and models exposed by the active
Codex installation. Assign bounded independent work, file ownership, and explicit
acceptance checks. Keep integration and final verification with the parent agent.
An instruction file cannot enable an unavailable subagent or MCP tool, so avoid
copying Claude's tool names or personal permission allowlists into Codex config.

## Verification limits

The migration can verify the offline Rust suite, formatting, all-target clippy,
configuration parsing, helper behavior, and skill scenarios without credentials.
It does not establish a live Codex-to-Telegram connection, run Premium transcription,
or validate private account permissions. Complete the manual smoke check above
after configuring and authenticating your own server.
