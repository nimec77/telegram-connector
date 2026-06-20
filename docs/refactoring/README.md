# Refactoring & Improvement Report — Telegram MCP Connector

**Date:** 2026-06-20
**Scope:** Full source tree (`src/`, ~12.7k LOC), `Cargo.toml`, project docs
**Baseline:** `master` @ v0.11.0 (commit `9899f1c`)
**Author:** Code analysis pass (Claude Code)

---

## 1. Purpose

This report is a structured audit of the codebase with three goals, in the
order requested:

1. **Split large modules** into smaller, single-purpose ones.
2. **Surface deviations** from the project's own architecture and conventions.
3. **Recommend other improvements** (correctness, performance, hygiene).

It is an **analysis + roadmap**, not a set of implementation plans. Each finding
carries evidence (`file:line`), the reason it matters, a concrete fix sketch, and
an effort/risk/impact rating. The team can promote any item to a
`docs/superpowers/` plan when scheduling the work.

The repository is healthy: layering is clean (MCP → application → Telegram), DI
via traits is consistent, error handling is typed, and test coverage is broad
(~400+ tests). The findings below are refinements, not rescues.

---

## 2. Methodology

- Read every production module end-to-end (entry points, the generic
  `McpServer` and all 11 tools, the `TelegramClient` + trait, converters,
  observability, config, rate limiter, link parser, domain types).
- Cross-checked against the project's stated rules in `CLAUDE.md`,
  `docs/conventions.md`, and the file-as-module / no-`mod.rs` patterns.
- Measured production-vs-test line counts to separate "genuinely large module"
  from "large because tests are inline."
- Verified dependency usage by source scan.

---

## 3. How to read this report

| Order | File | What it answers |
|-------|------|-----------------|
| 1 | `README.md` (this file) | What was found, how severe, where |
| 2 | `01-large-modules.md` | Which files are too big and how to split them |
| 3 | `02-architecture-and-duplication.md` | Where the code repeats itself or drifts from its own patterns |
| 4 | `03-conventions-and-quality.md` | Rule violations, dead code, hygiene |
| 5 | `04-roadmap.md` | What to do first, grouped and sequenced |

---

## 4. Legends

**Severity** — how much it hurts if left alone:

| Badge | Meaning |
|-------|---------|
| 🔴 High | Active maintainability/correctness/perf drag; fix soon |
| 🟡 Medium | Real improvement; schedule it |
| 🟢 Low | Polish / hygiene; do opportunistically |

**Effort** — rough implementation cost: **S** (<½ day) · **M** (½–2 days) · **L** (>2 days)

**Risk** — chance of regression: **Low** (mechanical / test-backed) · **Med** · **High**

---

## 5. Codebase snapshot

Largest files, split into production vs inline-test lines (approx.):

| File | Total | Prod | Inline tests | Note |
|------|------:|-----:|-------------:|------|
| `telegram/client.rs` | 923 | ~923 | 0 (tests external) | **Genuinely large** |
| `mcp/server.rs` | 880 | ~877 | 0 (tests external) | **Genuinely large** |
| `telegram/converters.rs` | 828 | ~457 | ~371 | Large prod + big inline tests |
| `mcp/observability.rs` | 729 | ~348 | ~381 | 3 concerns in one file |
| `config.rs` | 487 | ~485 | 0 (tests external) | Large prod |
| `mcp/tools/types/serde_helpers.rs` | 506 | ~178 | ~328 | Mostly tests |
| `mcp/tools/types/responses.rs` | 463 | ~267 | ~196 | Inline tests |
| `rate_limiter.rs` | 423 | ~104 | ~319 | Mostly tests |

**Takeaway:** the "large module" problem is two problems. `client.rs`,
`server.rs`, and `config.rs` are large in *production* code. `converters.rs`,
`observability.rs`, `serde_helpers.rs`, `rate_limiter.rs`, `responses.rs`, and
`requests.rs` are inflated by **inline tests** — and the repo *already* extracts
tests elsewhere (`config/tests.rs`, `mcp/tests/`, `telegram/tests/`), so the
convention is applied inconsistently.

---

## 6. Findings index

### Large modules — `01-large-modules.md`
| ID | Finding | Sev | Effort | Risk |
|----|---------|-----|--------|------|
| LM-1 | Inline tests inflate files; extract per existing `#[path]` convention | 🟡 | M | Low |
| LM-2 | `telegram/client.rs` (923L) — split per-operation + share resolution | 🔴 | L | Med |
| LM-3 | `mcp/server.rs` (877L) — move `*_impl` methods out of the router file | 🟡 | M | Low |
| LM-4 | `telegram/converters.rs` — split media / message / channel concerns | 🟡 | M | Low |
| LM-5 | `mcp/observability.rs` — split metrics / buffer / transport | 🟡 | M | Low |
| LM-6 | `config.rs` — extract `default_*` fns and env expansion | 🟢 | S | Low |

### Architecture & duplication — `02-architecture-and-duplication.md`
| ID | Finding | Sev | Effort | Risk |
|----|---------|-----|--------|------|
| AD-1 | Peer resolution implemented 3 ways; `resolve_peer()` unused by 2 callers | 🔴 | M | Med |
| AD-2 | `get_recent_messages` resolves a username channel twice over the network | 🟡 | S | Low |
| AD-3 | 11× near-identical `#[tool]` logging-wrapper boilerplate | 🟡 | M | Med |
| AD-4 | Peer→(id,name,username) extraction duplicated across converters | 🟢 | S | Low |
| AD-5 | `serde_json::to_string(..).map_err(..)` repeated ~13×; centralize | 🟢 | S | Low |
| AD-6 | Hard limits hardcoded while siblings are config-driven (inconsistent) | 🟡 | M | Low |

### Conventions & quality — `03-conventions-and-quality.md`
| ID | Finding | Sev | Effort | Risk |
|----|---------|-----|--------|------|
| CQ-1 | `.unwrap()` in production (`converters.rs` ×5, `rate_limiter.rs` ×3) | 🟡 | S | Low |
| CQ-2 | `apply_defaults()` is a documented no-op (dead code) | 🟢 | S | Low |
| CQ-3 | Unused dependencies: `dashmap`, `tokio-test` | 🟢 | S | Low |
| CQ-4 | `Channel.member_count = 0` / `description = None` hardcoded (dishonest) | 🟡 | M | Low |
| CQ-5 | `has_more = total >= limit` over-reports another page | 🟢 | S | Low |
| CQ-6 | Doc drift: CLAUDE.md says "10 tools" (11 exist); stale `memory.md` | 🟡 | S | Low |

---

## 7. Guiding principles applied

Recommendations are checked against the project's own stated values
(`docs/conventions.md`): KISS, Single Responsibility, **no premature
abstraction** (extract only after the Rule of Three), Explicit over Implicit,
**Delete over Comment**, and "no production code without a failing test." Where a
fix would add abstraction, the report says so and justifies it against Rule of
Three.
