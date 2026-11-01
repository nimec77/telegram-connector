# Current Project Guides Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline. Steps use checkbox syntax for tracking.

**Goal:** Make active guides accurate for pushed version 0.24.1.

**Architecture:** Port verified documentation corrections onto current master,
preserving the accepted runtime/media contracts and historical evidence.

**Tech Stack:** Markdown, TOML comments, Python validation, nightly Rust docs.

**Spec:** `docs/superpowers/specs/2026-11-01-current-guides-design.md`

## Global Constraints

- Change no Rust code, dependencies, active TOML values or package version.
- Preserve historical reports and the unmerged `docs/current-project-state` branch.
- Keep optional M2/M3 unimplemented; no live Telegram or Linux checks.
- Work on `docs/refresh-0.24.1-guides`; execute inline without routine approval pauses.

## Review Focus

- No stale links to deleted roadmap/workflow documents.
- No reintroduction of pre-Task-8 recovery or budget limitations.
- Examples use literal absolute paths or supported variable expansion, not tilde paths.
- Explain platform defaults and setup-only credentials without claiming Windows validation.
- Distinguish accepted/pushed changes, historical evidence and optional proposals.

### Task 1: Reconcile guides and archive completed workflow documents

**Files:** README.md, AGENTS.md, config.example.toml, docs/codex.md,
docs/memory.md, docs/stdio-runtime.md, docs/tool-contract.md, CHANGELOG.md;
delete the completed 2026-10-31 Codex workflow spec and plan.

**Interfaces:** Consume current configuration/runtime contracts and final acceptance;
produce accurate onboarding, migration instructions and project status.

- [x] Port accurate old-branch improvements; verify against source and official docs.
- [x] Remove completed Codex workflow records and refresh memory/changelog.
- [x] Verify local Markdown targets and TOML examples; compare parsed example values
      with master to prove only comments changed; check metadata/source scope.
- [x] Run `CARGO_NET_OFFLINE=true just check-docs`; expect warning-free docs and 3 doctests.
- [x] Inspect the complete diff and obtain one independent review; resolve findings.
- [x] Commit with a conventional documentation message and report validation.

## Execution record

- Base: `1a53ea0` (0.24.1, pushed master); existing full gate passed 942 tests and
  3 doctests. No code changes are planned, so documentation-specific checks suffice.
- Plan reviewed against the short design: one deliverable, no shared task interfaces.

- Verification: 42 local Markdown file/anchor targets resolved; all changed Markdown TOML
  fences parsed. Both configuration examples retain exactly the same parsed values as
  master. Cargo metadata/source, historical reports and the old branch are unchanged.
- `CARGO_NET_OFFLINE=true just check-docs`: exit 0, warning-free rustdoc and 3 doctests.
- Independent staged whole-diff review: no findings; source/path/status/error and archive
  scope checked. No live Telegram or additional platform execution was claimed.
- Integration: retain this completed commit on `docs/refresh-0.24.1-guides`; no push is
  included in this task. This plan/spec remain active until their change merges.
