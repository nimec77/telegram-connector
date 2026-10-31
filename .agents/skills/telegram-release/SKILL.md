---
name: telegram-release
description: Use when preparing a patch, minor, or major release of Telegram MCP Connector, including Cargo version and changelog updates; not for ordinary builds or unrelated projects.
---

# Telegram Release

Prepare a release of **Telegram MCP Connector**, a single Rust crate. Read the
root `AGENTS.md`, `Cargo.toml`, `CHANGELOG.md`, and Git state. Resolve the root with
`git rev-parse --show-toplevel`; run Cargo and Git commands there.

## Resolve the requested operation

Take `patch`, `minor`, `major`, or an exact version from the user's request. Read
the current version from **`[package].version`**, not `[workspace.package]`.
For a numeric `X.Y.Z`, patch increments Z, minor increments Y and resets Z, and
major increments X and resets Y/Z. If the version is a prerelease or the target
is ambiguous, clarify rather than silently choosing a different version policy.

Distinguish a dry run, local preparation, and a request that includes release
commit/tag/publication. Describe only the operations actually performed.
Preparation alone does not authorize a commit, tag, push, or GitHub release.

## Preflight

- Inspect `git status --short`, branch state, the target version's tag, and existing
  changelog sections before edits. If the tree is dirty, report the files and stop
  release mutations; never stash, reset, or include unrelated changes automatically.
  A dry run may still report a proposed version and blockers.
- Ensure `[Unreleased]` has substantive release notes. If empty, report this and
  prepare notes using `$telegram-changelog` when that is in the requested scope.
  Do not manufacture release content from internal-only commits.
- Run `just check` (or the exact gate in AGENTS.md if just is unavailable) before
  release mutations. For an explicit read-only dry run, report the gate as not run
  unless validation was requested. A failed gate blocks release completion.
- Refuse to overwrite an existing version section or tag. If publication is
  requested, load the installed `gh` skill and verify the intended remote first.

## Prepare locally

1. Change only `[package].version` in `Cargo.toml`.
2. Run `cargo check` to refresh the root package version in `Cargo.lock`. Do not
   delete the lockfile or run broad `cargo update`. Inspect the lockfile diff;
   dependency changes beyond the root version need a separate explanation.
3. Move the unreleased content under `## [X.Y.Z] - YYYY-MM-DD`, using the release
   date requested by the user or today's local date from `date +%Y-%m-%d`. Leave a
   fresh empty `## [Unreleased]` section above it; preserve older release sections.
4. Run `just check` again with the updated lockfile, inspect the diff, and verify
   that manifest, lockfile, and changelog versions agree. Update `docs/memory.md`
   if its Current state version would otherwise become stale.

For preparation, report the old/new version, files changed, validation, and any
remaining blockers. Say **prepared** when no release was tagged or published.

## When the request includes release operations

Respect the already authorized scope; do not ask for the same permission twice.
For an authorized release commit, stage only the reviewed release files and use
`chore: release vX.Y.Z`. For an authorized tag, create an annotated `vX.Y.Z` tag
and verify its target. Push only the requested branch and exact tag; never use
`git push --tags`. Use `gh release create` only when GitHub publication is requested.
After a partial failure, inspect existing commits/tags/remote state before retrying.
