# 04 — Roadmap

A suggested order for the work, grouped to match the project's phase model
(`docs/tasklist.md`). Sequencing matters: a couple of items unlock or de-risk
later ones, and some should be bundled so duplicated code is consolidated *during*
a move rather than after it.

---

## Sequencing principles

1. **Mechanical before structural.** Test extraction (LM-1) and hygiene
   (CQ-2/3/6) change no behavior and shrink/clean the surface before the
   production splits.
2. **Consolidate while moving, not after.** AD-1 (one resolver) lands *inside*
   LM-2 (split `client.rs`). AD-4 (peer identity) lands inside LM-4 (split
   `converters.rs`). CQ-1's `Username` fallback collapses into AD-4. Don't move
   duplicated code and then de-dupe in a second PR.
3. **One module per PR for the big splits.** LM-2/3/4/5 are independent; ship them
   separately so each diff stays reviewable and `cargo test` bisects cleanly.
4. **Honor the gate every step.** `cargo fmt --check && cargo clippy -- -D
   warnings && cargo test` (and `cargo test config -- --test-threads=1`) must pass
   per PR. TDD: any behavior change (AD-1, AD-2, CQ-4, CQ-5) gets its failing test
   first.

---

## Phase A — Quick wins (low risk, high signal)

Small, safe, and they make everything after them easier to measure. Could be a
single afternoon / one phase.

| ID | Action | Effort | Risk |
|----|--------|--------|------|
| CQ-3 | Remove unused deps (`dashmap`, `tokio-test`); commit `Cargo.lock` | S | Low |
| CQ-2 | Delete no-op `apply_defaults()` + call site | S | Low |
| CQ-6 | Fix `CLAUDE.md` "10 tools" → 11; reconcile `memory.md`/`tasklist.md` | S | Low |
| CQ-1 | `.unwrap()` → `expect()` in `converters.rs` / `rate_limiter.rs` | S | Low |
| AD-5 | Add `json_response` helper; adopt in `*_impl` tails | S | Low |

**Exit check:** `just check` green; `grep` shows no bare `.unwrap()` in non-test
`src/`; `CLAUDE.md` tool count matches `#[tool(` count.

---

## Phase B — Test extraction (mechanical, big footprint)

| ID | Action | Effort | Risk |
|----|--------|--------|------|
| LM-1 | Move inline tests to `#[path]` siblings for `converters`, `observability`, `serde_helpers`, `rate_limiter`, `responses`, `requests` | M | Low |

Do this before the production splits — it removes ~1,850 lines of test code from
six production files, so the subsequent splits operate on the real logic only.
Pure move; the compiler proves equivalence.

**Exit check:** identical test count before/after (`cargo test 2>&1 | tail`);
no production file edited except the trailing `mod tests;` declaration.

---

## Phase C — Structural splits + consolidation (the core refactor)

Each row is its own PR. Bundled de-duplication noted in **bold**.

| ID | Action | Effort | Risk |
|----|--------|--------|------|
| LM-2 + **AD-1** + **AD-2** | Split `client.rs` into `client/` submodules + `timeout.rs`; **collapse 3 resolvers into one `resolve_peer`**; **drop the double username resolution** | L | Med |
| LM-3 + **AD-3** | Move `*_impl` methods into `server/impl_*.rs`; **tackle wrapper boilerplate** (macro *or* guard — or consciously accept) | M | Med |
| LM-4 + **AD-4** + **CQ-1** | Split `converters.rs` into `media`/`message`/`channel`; **extract `peer_identity`**; **single `fallback_username`** | M | Low |
| LM-5 | Split `observability.rs` into `metrics`/`buffer`/`transport` | M | Low |

**Why this order:** `client.rs` is the highest-severity module and carries the
highest-severity duplication (AD-1) — do it first while the analysis is fresh.
`server.rs` next (most-touched file). Converters/observability are
lower-contention and can follow or run in parallel by a second person.

**Risk control for LM-2/AD-1:** the three resolvers are *not* byte-identical
(`02-architecture-and-duplication.md`, AD-1). Write a test per prior branch
(username, `@username`, numeric-ID dialog walk, "username without @" last resort)
**before** consolidating, so the merge is provably behavior-preserving.

---

## Phase D — Data & config refinements (behavior changes — TDD)

| ID | Action | Effort | Risk |
|----|--------|--------|------|
| CQ-4 | Make `Channel.member_count`/`description` honest (Optional first; optional full-fetch in `get_channel_info`) | M | Low |
| AD-6 | Centralize scattered limits; selectively promote `max_download_bytes` + transcription timeouts to config (`#[serde(default)]`) | M | Low |
| CQ-5 | Fix or document `has_more` boundary over-report | S | Low |

These alter observable output/config, so they're grouped last and each needs a
failing test first. CQ-4 in particular changes the `Channel` JSON schema — treat
it as a small feature, not a refactor.

---

## At-a-glance matrix

| ID | Title | Sev | Effort | Risk | Phase |
|----|-------|-----|--------|------|-------|
| LM-1 | Extract inline tests | 🟡 | M | Low | B |
| LM-2 | Split `client.rs` | 🔴 | L | Med | C |
| LM-3 | Split `server.rs` `*_impl` | 🟡 | M | Low | C |
| LM-4 | Split `converters.rs` | 🟡 | M | Low | C |
| LM-5 | Split `observability.rs` | 🟡 | M | Low | C |
| LM-6 | Split `config.rs` defaults/env | 🟢 | S | Low | opportunistic |
| AD-1 | One peer resolver | 🔴 | M | Med | C (w/ LM-2) |
| AD-2 | Kill double resolution | 🟡 | S | Low | C (w/ LM-2) |
| AD-3 | Tool-wrapper boilerplate | 🟡 | M | Med | C (w/ LM-3) |
| AD-4 | Shared peer identity | 🟢 | S | Low | C (w/ LM-4) |
| AD-5 | `json_response` helper | 🟢 | S | Low | A |
| AD-6 | Centralize limits | 🟡 | M | Low | D |
| CQ-1 | No `.unwrap()` in prod | 🟡 | S | Low | A (+C) |
| CQ-2 | Delete no-op | 🟢 | S | Low | A |
| CQ-3 | Drop unused deps | 🟢 | S | Low | A |
| CQ-4 | Honest channel fields | 🟡 | M | Low | D |
| CQ-5 | `has_more` boundary | 🟢 | S | Low | D |
| CQ-6 | Doc drift | 🟡 | S | Low | A |

---

## Explicitly out of scope / accept as-is

To keep the effort bounded, these are **deliberately not** recommended for change:

- **The `#[tool]` macro constraint** — keeping all tools in one `#[tool_router]`
  block is correct; don't try to scatter the `#[tool]` methods. (AD-3 only touches
  the *body* boilerplate, and even that is optional.)
- **`Result<String, String>` tool signatures** — dictated by rmcp; the string
  error surface is acceptable for an MCP transport.
- **`SearchResult` reuse for history** (`query = ""` sentinel) — slightly awkward
  but harmless; not worth a new type.
- **Internal constants** `JPEG_QUALITY`, `POLL_INTERVAL_SECS`, the base64 cap, the
  500-char preview cap — leave as named `const`s; promoting them to config is
  premature (KISS).
- **`config.rs` size (LM-6)** — mostly declarative; split only if you're in the
  file anyway.
- **The extra serialization in `InstrumentedTransport::send`** (`observability.rs:257`)
  — it's a deliberate trade (exact payload size + recovery copy) documented in
  place; only revisit if large-payload throughput becomes a measured problem.

---

## Suggested tracking

Add these as phases in `docs/tasklist.md` (the project's existing tracker), e.g.:

- **Phase 24 — Refactor: hygiene & test extraction** (Phases A + B here)
- **Phase 25 — Refactor: client/server module splits** (Phase C)
- **Phase 26 — Channel data & config refinements** (Phase D)

Each links back to the relevant finding IDs in this report so the rationale is one
hop away.
