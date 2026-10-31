# Codex workflow migration

## Intent

Make Codex able to develop and consume Telegram MCP Connector using checked-in,
portable instructions. Preserve the existing Rust architecture and MCP contract.

## Findings before migration

- The runtime uses standard MCP over stdio; it has no Anthropic API dependency.
- Contributor guidance lives in `CLAUDE.md`; the repository has no `AGENTS.md`.
- Ignored `.claude/` contains 21 local skills, an ast-index rule, and permission
  settings. There are no hooks. Most ticket orchestrators overlap with Superpowers
  and require Claude-only tools, models, and absent ticket artifacts.
- The old release skill targets Synapse and `[workspace.package]`; this repository
  has one `[package]`. The requested replacement is a version-only bump workflow.
- CI lints all targets; `just check` currently does not. Nightly is used by CI but
  not selected by a checked-in toolchain file. Edition 2024 alone does not select it.

## Approaches

1. Rename the instruction file only: cheapest, but loses useful local workflows
   and leaves Claude-only onboarding and conflicting validation commands.
2. Port all 21 skills: retains every entry point, but also duplicate orchestration,
   unavailable tools/models, obsolete paths, and unrelated release assumptions.
3. **Selected by the implementer:** Codex as the primary workflow, with concise
   canonical instructions, one focused version-bump skill, portable MCP examples,
   and removal of obsolete Claude-specific files as requested. It addresses the
   actual integration gaps with less instruction duplication. The request and existing no-approval-gate workflow
   authorize proceeding without another design checkpoint.

## Deliverables

- Root `AGENTS.md`: onboarding, architecture, ast-index rules, verification,
  durable memory, and code review rules. Remove `CLAUDE.md` and `.claude/`.
- `.agents/skills/bump-version/`: update `[package].version` and the root package
  in `Cargo.lock` for patch/minor/major or explicit version requests. Preserve
  unrelated edits, support dry runs, and validate with the Rust gate. Remove the
  superseded changelog/release skills and their helper scripts/tests.
- `docs/codex.md`: analysis, setup, migration inventory, recommended integrations,
  optional coverage/security tooling, and the limits of offline verification.
- `.codex/config.toml.example`: opt-in configuration with placeholder paths,
  30-second startup and 180-second tool budgets, no secrets or permission overrides.
- README and CLI help describe Codex and other MCP clients. Add `rust-toolchain.toml`
  selecting nightly with rustfmt/clippy, and align local checks with CI.

## Constraints and non-goals

- Keep dependency pins, lockfile resolution, tool names, schemas, and runtime behavior.
- Do not copy personal permission allowlists or modify global Codex configuration.
- Remove the obsolete `.claude/` directory after porting the useful workflows;
  do not bulk-copy generic Rust tutorials or Claude-specific orchestrators.
  Document their replacements.
- Do not run authenticated Telegram probes, tag a release, push, or publish.
- Apply source tests only where behavior changes; documentation/configuration use
  parsing, actual command execution, skill validation, and independent review.
- Preserve Superpowers as an optional installed workflow, not a runtime dependency.

## Verification

Run the existing Rust gate with locked dependencies and all-target clippy; parse
TOML and skill metadata. Exercise the version-bump workflow in a disposable Rust
crate, including an unrelated edit and empty Unreleased notes. Check that dry runs
leave files unchanged. Review the integration for stale references to removed skills.
Live Codex-to-Telegram authentication remains a documented manual check.
