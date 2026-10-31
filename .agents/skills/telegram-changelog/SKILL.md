---
name: telegram-changelog
description: Use when updating Telegram MCP Connector's unreleased changelog from local Git history or preparing release notes; not for changing versions or publishing releases.
---

# Telegram Changelog

Update this repository's `CHANGELOG.md` in its existing Keep a Changelog 1.0.0
style. Add or merge entries under `## [Unreleased]`; preserve released sections.

Resolve the repository with `git rev-parse --show-toplevel`. Resolve this skill's
directory from the loaded `SKILL.md` path, not the shell's working directory.
From any directory inside the checkout, collect history with:

```bash
bash "<skill-directory>/scripts/commits_since_release.sh" "<repository-root>"
```

The helper selects the first release boundary on HEAD's first-parent history:
a `vX.Y.Z`/`X.Y.Z` tag or a `chore: release vX.Y.Z` commit (optional conventional
scope). A newer release commit takes precedence over an older tag. With no
boundary, it reports full history. It reads local Git only; do not fetch or tag.

Read [the format reference](references/keep-a-changelog.md) when writing entries.
Inspect the commits/diffs when subjects alone do not establish the user-visible
effect. Deduplicate against both Unreleased and already released entries when
reconstructing history. Dependency updates belong under Changed; internal-only
docs/tests/CI changes are normally omitted. Preserve documented breaking changes.

A zero commit count means only that the chosen range has no non-merge commits.
Check existing notes and the requested scope before claiming completeness. The
helper excludes merge commits; inspect them separately when a merge introduces
behavior that is absent from its constituent commits. Uncommitted work is outside
the helper's range and must not be described as committed history.

Report the selected boundary/range, entries added or merged, and significant
omissions. A changelog request does not authorize version bumps, commits, tags,
pushes, or publication. No ticket artifacts, Claude tools, or model names are needed.
