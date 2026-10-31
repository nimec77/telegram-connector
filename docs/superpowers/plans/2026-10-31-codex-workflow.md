# Codex Workflow Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan
> inline. Steps use checkbox syntax for tracking.

**Goal:** Make the existing project usable and maintainable with Codex.

**Architecture:** Keep the MCP service client-independent. Consolidate always-on
instructions in AGENTS.md and expose release/changelog work through repository skills.

**Tech Stack:** Rust nightly, Cargo, just, Python 3, Bash, Codex skills and MCP configuration.

**Spec:** `docs/superpowers/specs/2026-10-31-codex-workflow-design.md`

## Global Constraints

- Keep dependency pins, lockfile resolution, tool names, schemas, and runtime behavior.
- Do not copy personal permission allowlists or modify global Codex configuration.
- Remove obsolete `CLAUDE.md` and `.claude/` files as requested by the user.
- Do not run authenticated Telegram probes, tag a release, push, or publish.

## Review Focus

- Release instructions must work with `[package]`, not a nonexistent workspace.
- Skill resources must resolve from arbitrary working directories.
- Zero new commits must not be treated as proof that CHANGELOG is already correct.
- MCP configuration uses absolute example paths and a tool timeout above 120 seconds.
- Development needs no credentials, global skills, or live Telegram connection.

## Progress and rulings

- Initial tree clean; working on `chore/codex-workflow` in the existing checkout.
- Ruling: work in the current checkout without creating a worktree. Do not publish
  a PR or make release commits as part of a tooling migration. The user's follow-up
  explicitly authorizes committing and pushing the migration branch.
- Ruling: repository workflow explicitly allows autonomous work without manual
  approval checkpoints; proceed with the selected design and inline implementation.
- Independent builder check: nothing important left unanswered.
- Skill baseline: changelog helper chose stale v0.22.4 instead of release v0.22.6;
  release instructions failed on nonexistent `[workspace.package]`. Seven temporary
  history fixtures exposed stale tags, nonportable BSD awk delimiters, and an
  unjustified completeness claim (three failures before adaptation).
- Changelog behavior after adaptation: all seven fixtures pass; independent
  invocation from `src/mcp` selects release v0.22.6 and correctly reports zero
  commits without asserting changelog completeness.
- Ruling: retain nine portable helper regression cases in the changelog skill,
  expose `just skills-check`, and run them in a separate CI job. This keeps Python
  out of the Rust gate while preventing the ported boundary bugs from recurring.
- Release behavior after adaptation: independent dry run reads `[package]`,
  proposes 0.22.7, reports the dirty tree/empty Unreleased blockers, and performs
  no release mutation. Both skill frontmatter validators pass.
- Verification: `CARGO_NET_OFFLINE=true just check` passed format and all-target
  clippy with warnings denied; 758 Rust tests passed, 5 ignored, with 1 additional
  ignored doctest. `just skills-check` passed all nine fixtures and Bash syntax.
  `cargo run --locked --offline --bin telegram-mcp -- --help` exited successfully.
  TOML/YAML parsing, local Markdown links, metadata, and exact CI/local Rust gate
  consistency passed. Cargo.toml and Cargo.lock are unchanged.
- Independent whole-diff review: approved with no Critical, Important, or Minor
  findings. Live authenticated MCP compatibility, dated nightly pinning, and the
  existing runtime backlog were outside this migration; no runtime fix was implied.
- Ruling: keep the established nightly channel rather than changing compiler
  policy or resolving unrelated runtime backlog items. Live Telegram validation
  remains manual because it requires an authenticated account. The user's follow-up
  requests removal of Claude-specific files, then a commit and push of the branch.
- [x] Follow-up: remove `CLAUDE.md`, the local `.claude/` directory, and obsolete
  compatibility/retention instructions from the documentation.
- Follow-up verification: `CARGO_NET_OFFLINE=true just check` and
  `just skills-check` both passed after cleanup; Claude-specific paths are absent,
  local documentation links resolve, and `git diff --check HEAD` is clean.

### Task 1: Canonical instructions and Codex onboarding

**Files:** `AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/codex.md`,
`.codex/config.toml.example`, `docs/conventions.md`, `docs/memory.md`.

**Interfaces:** AGENTS.md is the canonical repository guide; skill instructions
refer to it and `just check`. Configuration is an opt-in example only.

- [x] Record the existing onboarding and skill failures in independent baseline review.
- [x] Write canonical guidance and remove obsolete Claude-specific files.
- [x] Document Codex MCP registration, recommendations, migration inventory, and limits.
- [x] Parse configuration examples and check referenced repository paths.

### Task 2: Portable changelog and release skills

**Files:** `.agents/skills/telegram-changelog/`, `.agents/skills/telegram-release/`.

**Interfaces:** Changelog skill bundles read-only Git history collection; release
skill consumes `[package].version`, the changelog, and the local quality gate.

- [x] Test legacy workflows in a fresh context before adaptation.
- [x] Initialize, adapt, and validate telegram-changelog, then test its behavior.
- [x] Exercise its helper against temporary Git histories and invalid input.
- [x] Initialize, adapt, and validate telegram-release, then test its behavior.
- [x] Keep skills discoverable without Claude model/tool declarations or ticket files.

### Task 3: Align local tooling and verify

**Files:** `rust-toolchain.toml`, `justfile`, `.github/workflows/ci.yml`,
`src/cli.rs`, `src/mcp/tests/server_core.rs`, `.gitignore`.

**Interfaces:** `just check` and CI run the same locked Rust gate, including linting
test targets. No MCP runtime interfaces change.

- [x] Align format/lint/test commands and record nightly explicitly.
- [x] Refresh client-neutral CLI copy and the stale instruction-file reference.
- [x] Ignore Codex local overrides and Superpowers scratch without hiding shared skills.
- [x] Run fmt, all-target clippy, tests, skill validators, and configuration parsing.
- [x] Obtain independent whole-diff review and resolve material findings.
- [x] Record verification evidence and remaining live-connection limits.
