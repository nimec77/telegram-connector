# Changelog conventions

Preserve the existing Keep a Changelog **1.0.0** header and Semantic Versioning
link in `CHANGELOG.md`. Do not rewrite released sections or add comparison links
unless requested. This reference adapts the former local changelog skill.

Only include groups that have entries, in this order:

1. Added
2. Changed
3. Deprecated
4. Removed
5. Fixed
6. Security

| Commit meaning | Group |
| --- | --- |
| New user-facing feature (`feat`) | Added |
| Behavior/performance change (`perf`, visible `refactor`) | Changed |
| Dependency version update, including a `chore` | Changed |
| Announced deprecation | Deprecated |
| Removed functionality | Removed |
| Bug fix (`fix`) | Fixed |
| Security vulnerability fix | Security |
| Release marker; internal docs, tests, CI, formatting | Omit |

A conventional `!` or `BREAKING CHANGE:` footer signals a breaking change, not
necessarily removal. Choose the group by the effect and prefix `**Breaking:**`.

Lead with behavior, then name symbols/configuration when they help the reader.
For example, describe stalled Telegram RPCs returning a typed timeout instead of
only saying "add timeout handling". Consolidate related dependency bumps into a
single Changed bullet with the old/new versions when available from the diff.

Before adding a bullet, read the existing Unreleased section and relevant released
entries. Match by meaning, not exact text. A feature and follow-up fix may belong
in one note. Never infer an improvement that the commits/diffs do not establish.

If the file is absent, reproduce this header before adding entries:

```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
```
