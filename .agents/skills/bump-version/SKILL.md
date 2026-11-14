---
name: bump-version
description: Use when bumping Telegram MCP Connector's package version by patch, minor, major, or an explicit version; not for dependency upgrades or publishing releases.
---

# Bump Version

Synchronize **all four files**: `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, and
`README.md` for this single Rust crate. Preserve dependency pins and unrelated work.
Resolve the root with `git rev-parse --show-toplevel`, read `AGENTS.md`, and run
Cargo commands from there.

## Choose the target

Read the current version from **`[package].version` in `Cargo.toml`**. Take the
bump kind or exact target from the user's request; ask if neither was specified.
Apply normal SemVer patch/minor/major increments, resetting lower components.
For an existing prerelease and a relative bump, clarify the intended next version.
Validate an explicit target as a Cargo-compatible SemVer version. If already at
that target, inspect all four files and repair stale documentation before reporting
a no-op. Preserve existing release dates and Unreleased notes in this repair case;
do not roll a second release. A lower version needs an explicit downgrade request.
For an explicitly requested downgrade to an existing release, reuse its section
and date and preserve Unreleased notes instead of rewriting release history.

An explicit dry run reports the current/target versions and proposed changes to all
four files, including the dated changelog section, notes to move, and README's
current-version line and changelog link, without editing files or running mutating
Cargo commands.

## Apply and verify

1. Inspect `git status --short` and existing diffs for `Cargo.toml`, `Cargo.lock`,
   `CHANGELOG.md`, and `README.md`. Read the Unreleased notes, existing version
   headings, and README's current-version line.
   Preserve unrelated edits; a dirty tree alone does not block a version bump.
   Resolve conflicting edits to the package version or an existing changelog section
   for the target version before making changes; never duplicate a version heading.
2. Change only `[package].version` in `Cargo.toml`, then run `cargo check` to
   synchronize the root package version in `Cargo.lock`. Use `--offline` when
   dependencies are cached. Keep the lockfile and dependency declarations intact;
   do not run a broad `cargo update` or regenerate the entire dependency graph.
3. For a new version, update `CHANGELOG.md`. Insert `## [<target>] - YYYY-MM-DD`
   immediately below `## [Unreleased]`, using the current local date unless the user
   supplied a date. Move the existing Unreleased notes under the target heading,
   preserving their categories, order, and wording; leave a fresh empty Unreleased
   section above it. Even when Unreleased is empty, add the dated target section
   without inventing release notes. An existing target section keeps its date and
   notes; resolve conflicting content instead of duplicating it. Preserve all
   historical sections and dates.
   For an already-matching package version, preserve Unreleased notes and reuse
   the target section/date. If that section is missing, add an empty dated section
   using the same date rule instead of moving Unreleased notes.
4. Update README's current package version and link to the target changelog heading:

   ```markdown
   Current package version: [<target>](CHANGELOG.md#<release-heading-anchor>).
   ```

   If the line is absent, add it near the introduction. Use the actual target
   section's date for the GitHub heading anchor, including when repairing an
   already-bumped version. Preserve historical references such as "Since version
   0.23.0" and protocol/dependency versions. A package bump does not establish that
   a tag or GitHub release has been published.
5. Inspect the diff for the four required surfaces: manifest package version,
   root lockfile entry, changelog section, and README current-version line/link.
   Investigate dependency churn instead of including it in the bump. Confirm the
   date, transferred notes, and empty Unreleased section for a new bump. Run the
   bundled read-only consistency check from the project root. Check
   `python3 --version` first: it requires Python 3.11+. If the default is older,
   select an installed newer interpreter (for example `python3.14`) for this command:

   ```bash
   python3 .agents/skills/bump-version/scripts/check_version_docs.py .
   ```

   It checks both Cargo versions, the single latest dated changelog section, and
   README's displayed version and exact local changelog link. Resolve diagnostics
   before completion; the command prints the expected README line when it differs.
   Only for an explicitly requested downgrade to an existing historical release,
   append `--allow-older-release`; keep the default first-release check for bumps.
6. Run `just check`, or the equivalent Rust gate from `AGENTS.md` if just is
   unavailable. Report failures accurately; do not claim the bump is verified
   until the gate passes.

Preserve historical release/version dates in `docs/memory.md`; update those only
for a separately requested release operation.
Commit, tag, push, or publish only within the scope already authorized by the user.

Report the old and new versions, changelog version/date, README version/link,
files changed, and results of both the consistency check and Rust gate. Report a
no-op only when all four surfaces already agree and require no edits.
Example request: `Use $bump-version to bump the patch version.`
