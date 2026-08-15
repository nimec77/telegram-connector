# Audit Stage 4 — Ops-Layer Verification (design, 2026-08-15)

Design for stage 4 of the project audit
(`docs/superpowers/specs/2026-08-15-project-audit.md`). Stages 1–3 are merged.

**Goal:** the message-fetch ops layer's decision logic is pinned by tests. Coverage
movement is a consequence, not the target — there is no percentage to hit.

**In scope:** `ops_search.rs`, `ops_history.rs`, `ops_message.rs`, `resolve.rs`,
`channels.rs`.

**Out of scope:** `ops_media.rs`, `ops_stats.rs`, `ops_transcribe.rs`, `lifecycle.rs`,
`client/auth.rs`. Also out: `mcp/server.rs`'s tool-wrapper log paths (they need a
`RequestContext<RoleServer>`, which `docs/memory.md` records as unconstructible in unit
tests) and the `impl_message_batch.rs` / `impl_media.rs` six-line precheck duplication
(the audit spec itself rates it "barely worth it").

---

## Why the ops layer is at 0%

The DI seam is `TelegramClientTrait`. Tests swap the entire production client for
`MockTelegramClientTrait`, so every `*_impl` method sits below the seam and no unit test
reaches it. The remedy proven in-repo is to keep lifting pure decision logic above the
seam — `albums.rs`, `search_budget.rs`, and the paging math already work this way.

Stage 4 applies that remedy to the part that carries the real invariants: the three
message-accumulation loops.

## What the three loops actually share

Measured, not assumed. The loops in `get_recent_messages_impl`, `search_in_channel`, and
`search_global` differ along seven axes:

| axis | history | channel search | global search |
|---|---|---|---|
| below-cutoff message | break | break | **continue** |
| peer for conversion | fixed | fixed | **per-message, skip if `None`** |
| client-side media filter | **yes** | no (server-side) | no (server-side) |
| `after_bound` | yes | yes | n/a (rejected upstream) |
| `budget.expired()` at loop top | **absent** | present | present |
| error text | `Failed to iterate messages` | `Search failed` | `Search failed` |
| per-page debug log + mtproto timing | no | no | **yes** |

Five of the seven collapse into ordinary parameters once the seam is synchronous:

- **peer** — `Option<&Peer>` unifies "always present" with "skip when the chat did not
  resolve". Inert on the peer-scoped paths.
- **below-cutoff** — a two-value enum, not a caller identity switch.
- **`after_bound`, media filter** — `Option`s that are `None` where the axis does not apply.
- **`budget.expired()`** — history builds `SearchBudget::new(0)`, and
  `zero_deadline_is_treated_as_disabled_not_instantly_expired` pins that a zero deadline
  never expires. Adding the check to history is provably inert.

The remaining two — error text and the per-page log — are not decision logic and stay in
the callers.

So: **one walk, five config fields.**

## Section 1 — `MessageWalk`

New file `src/telegram/client/walk.rs`. It owns the `PageAccumulator` and `SearchBudget`
for the duration of a walk and hands both back at the end.

```rust
pub(super) enum Flow { Continue, Stop }
pub(super) enum BelowCutoff { Stop, Skip }

pub(super) struct WalkConfig<'a> {
    cutoff_time: DateTime<Utc>,
    to_date: Option<DateTime<Utc>>,
    after_bound: Option<i32>,               // None on global
    media_filter: Option<&'a MediaFilter>,  // Some only on history
    below_cutoff: BelowCutoff,              // Stop for history/channel, Skip for global
}

pub(super) struct Fetched<'p> {
    raw: tl::enums::Message,
    entities: Arc<EntityLookup>,
    peer: Option<&'p Peer>,  // always Some on peer-scoped paths
}

impl<'a> MessageWalk<'a> {
    fn new(cfg: WalkConfig<'a>, collapse_albums: bool, limit: usize, deadline_secs: u64) -> Self;
    fn expired(&mut self) -> bool;
    fn step(&mut self, fetched: Option<Fetched<'_>>, page_size: Option<usize>) -> Flow;
    fn into_parts(self) -> (PageAccumulator, SearchBudget);

    // Read-only, for the global path's per-page debug log (see the deltas below).
    fn pages_fetched(&self) -> u32;
    fn messages_scanned(&self) -> u64;
    fn kept(&self) -> usize;
}
```

Global search maps its three-tuple into `Fetched` at the call site
(`(raw, entities, chat_peer)` → `peer: chat_peer.as_ref()`); the peer-scoped paths pass
`peer: Some(&peer)`.

`step` runs one fixed order, and that order is the thing currently untested:

1. `record_page(page_size)` when `Some` — **before** anything can return `Stop`, so a round
   trip that came back empty is still counted.
2. `fetched == None` → `Stop`.
3. `to_date` present and the message is newer → `Continue`.
4. Timestamp below `cutoff_time`, or absent → apply `below_cutoff`.
5. `after_bound` present and `raw.id() <= after` → `Stop` (exclusive bound).
6. Media filter present and not matched → `Continue`.
7. `peer` absent → `Continue`.
8. `convert_raw_message` returns `None` → `Continue`.
9. `PageAccumulator::push` returns `false` → `Stop`.

Each of the three loops reduces to:

```rust
loop {
    if walk.expired() { break }
    let next = pager.next().await.map_err(|e| Error::TelegramApi(format!("{CTX}: {e}")))?;
    let page_size = pager.take_last_page_size();
    if matches!(walk.step(fetched_from(next), page_size), Flow::Stop) { break }
}
let (page, budget) = walk.into_parts();
```

The only ordering left caller-side is `next()` before `take_last_page_size()`, which the
borrow checker already forces.

### Deliberate behavior deltas

Both are behavior-preserving; both belong in the PR description, the way stage 3's three
deltas did.

1. **History gains an `expired()` check.** Inert — `SearchBudget::new(0)` never expires
   (pinned by an existing test).
2. **Global search's per-page `debug!` moves from before `step` to after it.** The logged
   values are identical, because `record_page` has run in either ordering. The caller
   gates the log on `page_size.is_some()` and reads the counters back through accessors.

## Section 2 — extractions outside the loop

Five pure functions, each pinning an invariant currently at 0%:

| extraction | file | invariant |
|---|---|---|
| `assemble_search_result(..)` | shared | `channels_in_results` unique-count, `timed_out`/`partial` pairing, counter passthrough |
| `dialog_fallback_target(channel_id, identifier)` | `ops_history` | AD-2 — a username reference carrying no numeric id hard-errors rather than walking dialogs by an id we never had |
| `partition_batch(ids, by_id, peer, entities)` | `ops_message` | every requested id lands in exactly one of `messages` / `missing_ids` |
| `ChannelPageBuilder { offset, limit }` | `channels` | B6 — `total` counts every channel while the page is cut out in passing |
| `classify_search_hit(chat, subscribed_keys)` | `channels` | `Chat::Empty` skipped; subscribed-vs-discovered split; `chats`-overshoots-`limit` truncation |

**Sorting stays in the search caller.** `search_messages_impl` sorts by timestamp
descending; `get_recent_messages_impl` does not. Folding the sort into the shared assembly
would silently start sorting history results.

`resolve.rs` needs no new extraction — `username_to_resolve` is already pure and the
numeric/username branch is a single `parse::<i64>()`.

## Section 3 — tests

TDD throughout: failing test first, per `docs/conventions.md`. All plain `#[test]`, no
async, no new mocks. Fixtures already exist: `raw_tl_message`, `raw_tl_channel`,
`raw_tl_user`, `raw_tl_messages_slice` in `src/test_helpers.rs`, and the no-I/O `Peer`
construction shown by `community_peer()` in `converters/channel.rs`.

`walk.rs` tests — one per `step` branch, plus the ordering cases that motivate the work:

- an empty round trip still counts a page (step 1 before step 2);
- `BelowCutoff::Stop` vs `Skip` over the same input sequence;
- an absent timestamp takes the below-cutoff path;
- `after_bound` exclusivity at `id == after`;
- an album is not split when its siblings straddle the limit;
- `has_more` is true only when a qualifying message was refused by limit or budget.

Extraction tests — one per invariant in the Section 2 table. `partition_batch` covers all
three routes to `missing_ids`: absent entry, `MessageEmpty`, and convert-failure.

Test files follow the established `#[path]`-included sibling pattern
(`src/telegram/client/tests/walk_tests.rs`); no `mod.rs`.

### Additional coverage

`config.rs` is at 69.6% behind 861 test lines — the gap is file-loading error branches.
Add tests for: missing file, unreadable file, malformed TOML, and failed `${VAR}`
expansion. Env-mutating cases go through `EnvGuard` (`ENV_LOCK`), which cannot be nested.

`serde_helpers.rs` (81.5%) is left alone — the residue is defensive arms with no
behavioural claim attached.

## Section 4 — stage-3 follow-ups

All three, all small:

- `#[must_use]` on `PageAccumulator::push` — its `false` return is a control-flow signal.
- A `collapse=false` album-sibling `into_messages` test.
- Rename the global-search tracing field `page` → `page_no`. It collides with the `page`
  accumulator local; Section 1 puts the two in the same scope, so this stops being cosmetic.

## Section 5 — hygiene backlog

Re-verified against the tree at `5013672`. The audit spec's list had two stale entries and
three that had drifted; those corrections land in the audit spec as part of this change.

| item | location | action |
|---|---|---|
| `ProjectDirs…expect()` | `config/defaults.rs:14,56` | return an error, as `config.rs:426` does |
| `auth_credentials()` `expect()` | `config.rs:117,121` | return `Option` |
| init errors all swallowed | `logging.rs` — `result.or(Ok(()))` | swallow double-init only |
| `process::exit(0)` skips destructors | `main.rs:69` | return, or document why not |
| base64 size underflow | `impl_status.rs:105` — `data.len() / 4 * 3 - padding` | `saturating_sub`; reachable on a malformed 2-char payload |
| trait docs leak internal names | `trait_def.rs:57,64` | the leak is `raw_fetch::fetch_messages_by_id`, **not** `raw_pager` as the audit spec says |
| tool doc-comment numbering | `server.rs:426` | Tool 16's comment sits between Tool 10 (`:405`) and Tool 11 (`:447`) |
| `parse_args` wrapper | `cli.rs:35` (audit spec says `:33`) | drop the wrapper |
| `tokio features = ["full"]` | `Cargo.toml:28` | narrow to what is used |
| `redact_phone` ≤6 threshold | `logging.rs:85` | a 7-char phone renders `1234***567` — every character. Raise the threshold and update the test that documents the current behaviour |
| Vec-element scalars get no coercion | — | deliberate; document in the coercion design notes |

**Stale — remove from the audit spec, do not action:**

- `rate_limiter.rs:81,114,124,129` `lock().unwrap()` and the duplicated `available_tokens`.
  Fixed in `2072a67`; one `.lock()` remains at `:91`, with poison recovery.

## Risks

- **`WalkConfig` grows an eighth axis.** The design rests on the seven-axis table above
  being complete for the three in-scope loops. If a fourth caller later needs a knob that
  is not an `Option`-or-two-value-enum, the abstraction is wrong and should split rather
  than accrete booleans.
- **`assemble_search_result` unifying two callers that differ more than they appear to.**
  The sort is already carved out; `channels_in_results` is equivalent across both only
  because history fetches from exactly one peer. That equivalence is worth an explicit
  test rather than a comment.

## Sequencing

`chore: release v0.22.3` is unmerged on `chore/audit-stage3-cleanup`; master requires PRs,
so it goes up first. Stage 4 branches from master afterwards.

`docs/memory.md` needs two corrections in the same change: the v0.22.2 open item
(`Cargo.toml` is now 0.22.3) and the stage-3 status.

Delete-on-merge: this file and the stage-4 plan go away once stage 4 lands.
