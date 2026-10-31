# Codex Workflow Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan
> inline. Steps use checkbox syntax for tracking.

**Goal:** Make the existing project usable and maintainable with Codex.

**Architecture:** Preserve the client-independent MCP runtime. Consolidate shared
instructions in AGENTS.md and provide one focused bump-version skill.

**Tech Stack:** Rust nightly, Cargo, just, Codex skills and MCP configuration.

**Spec:** `docs/superpowers/specs/2026-10-31-codex-workflow-design.md`

## Global Constraints

- Keep dependency pins, lockfile resolution, tool names, schemas, and runtime behavior.
- Do not copy personal permission allowlists or modify global Codex configuration.
- Remove obsolete Claude-specific files.
- Do not run authenticated Telegram probes or publish a release.
- The user authorizes committing and pushing the migration branch.

## Review Focus

- The bump skill reads `[package].version` and synchronizes the root lockfile entry.
- Version-only bumps preserve unrelated edits and do not require release notes.
- Dry runs and already-current targets leave files unchanged.
- Removed workflows have no live documentation, recipe, or CI references.
- MCP timeout examples accommodate 120-second transcription.

## Progress and rulings

- Working on `chore/codex-workflow` in the original checkout.
- Ruling: keep the established nightly channel and runtime architecture. Use
  locked dependencies and all-target clippy in local and CI quality gates.
- Ruling: the user requested one skill named `bump-version` instead of separate
  changelog/release workflows. Remove the old workflows, scripts, tests, recipe,
  and CI job rather than retaining duplicate entry points.
- Baseline skill evaluation: the release workflow selected 0.22.6 to 0.22.7 but
  blocked a version-only request on unrelated README edits and empty Unreleased
  notes, and would otherwise perform additional release-note operations.
- The root package version stays 0.22.6: this change creates the reusable skill;
  it is not itself a request to bump the application version.
- Prior migration validation passed format, all-target clippy, 758 Rust tests
  (5 ignored plus one ignored doctest), configuration parsing, and independent
  whole-diff review. Live Telegram authentication remains a manual check.
- Replacement validation: skill/frontmatter/UI metadata and local documentation
  paths passed; no deleted helper references remain. CI and local Rust gates match.
  `CARGO_NET_OFFLINE=true just check` passed formatting, all-target clippy, and
  758 Rust tests (5 ignored plus one ignored doctest).
- Independent fixture test applied 0.22.6 to 0.23.0 in the manifest/lockfile only,
  preserved an unrelated README edit and empty Unreleased section, and passed
  cargo check plus the Rust gate offline. Patch/major/explicit dry runs and an
  already-current target left fixture files unchanged. Integration review found
  no material issues. The application version in this checkout is unchanged.

### Task 1: Canonical Codex instructions and onboarding

**Files:** AGENTS.md, README.md, docs/codex.md, docs/conventions.md,
docs/memory.md, .codex/config.toml.example.

**Interfaces:** AGENTS.md is canonical; the shared skill refers to it and just check.

- [x] Move repository guidance into AGENTS.md and remove Claude-specific files.
- [x] Document Codex MCP configuration, migration inventory, and optional tools.
- [x] Keep configuration examples inert and free of credentials/permission overrides.

### Task 2: Focused version-bump skill

**Files:** .agents/skills/bump-version/SKILL.md and agents/openai.yaml; remove
superseded skill directories and their helper references from CI/justfile/docs.

**Interfaces:** Read the package version from Cargo.toml and synchronize Cargo.lock.

- [x] Evaluate the old release skill against a version-only request before replacement.
- [x] Create the skill as bump-version with matching metadata and invocation.
- [x] Remove the two old skills, helper scripts/tests, recipe, and CI job.
- [x] Update active documentation to the selected workflow.
- [x] Validate skill metadata and local documentation paths.
- [x] Test the skill in a disposable Rust crate with dirty unrelated work and dry runs.

### Task 3: Verify and publish the branch

**Files:** rust-toolchain.toml, justfile, .github/workflows/ci.yml,
src/cli.rs, src/mcp/tests/server_core.rs, .gitignore.

**Interfaces:** Local and CI checks use the same locked Rust gate. Runtime interfaces
and application dependency versions stay unchanged.

- [x] Align local/CI commands, select nightly, and refresh client-neutral CLI copy.
- [x] Run the Rust gate and final integration checks after the skill replacement.
- [x] Review the final diff for the authorized commit and push.
