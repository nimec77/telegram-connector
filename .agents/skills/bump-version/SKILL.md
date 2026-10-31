---
name: bump-version
description: Use when bumping Telegram MCP Connector's package version by patch, minor, major, or an explicit version; not for dependency upgrades or publishing releases.
---

# Bump Version

Update `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md` together for this single Rust
crate while preserving dependency pins and unrelated work. Resolve the root with
`git rev-parse --show-toplevel`, read `AGENTS.md`, and run Cargo commands from there.

## Choose the target

Read the current version from **`[package].version` in `Cargo.toml`**. Take the
bump kind or exact target from the user's request; ask if neither was specified.
Apply normal SemVer patch/minor/major increments, resetting lower components.
For an existing prerelease and a relative bump, clarify the intended next version.
Validate an explicit target as a Cargo-compatible SemVer version. If already at
that target, report a no-op. A lower version needs an explicit downgrade request.

An explicit dry run reports the current/target versions and proposed changes to all
three files, including the dated changelog section and notes to move, without
editing files or running mutating Cargo commands.

## Apply and verify

1. Inspect `git status --short` and existing diffs for `Cargo.toml`, `Cargo.lock`,
   and `CHANGELOG.md`. Read the Unreleased notes and existing version headings.
   Preserve unrelated edits; a dirty tree alone does not block a version bump.
   Resolve conflicting edits to the package version or an existing changelog section
   for the target version before making changes; never duplicate a version heading.
2. Change only `[package].version` in `Cargo.toml`, then run `cargo check` to
   synchronize the root package version in `Cargo.lock`. Use `--offline` when
   dependencies are cached. Keep the lockfile and dependency declarations intact;
   do not run a broad `cargo update` or regenerate the entire dependency graph.
3. Update `CHANGELOG.md` as part of the bump. Insert `## [<target>] - YYYY-MM-DD`
   immediately below `## [Unreleased]`, using the current local date unless the user
   supplied a date. Move the existing Unreleased notes under the target heading,
   preserving their categories, order, and wording; leave a fresh empty Unreleased
   section above it. Even when Unreleased is empty, add the dated target section
   without inventing release notes. Preserve all historical sections and dates.
4. Inspect the diff. The expected new changes are the manifest package version,
   the lockfile entry for `telegram-connector`, and the changelog rollover.
   Investigate dependency churn instead of including it in the bump. Confirm both
   package versions and the single new changelog heading agree on the target;
   verify the date, transferred notes, and empty Unreleased section.
5. Run `just check`, or the equivalent Rust gate from `AGENTS.md` if just is
   unavailable. Report failures accurately; do not claim the bump is verified
   until the gate passes.

Preserve historical release/version dates in `docs/memory.md`; update those only
for a separately requested release operation.
Commit, tag, push, or publish only within the scope already authorized by the user.

Report the old and new versions, files changed, changelog version/date, and
validation result. Example request: `Use $bump-version to bump the patch version.`
