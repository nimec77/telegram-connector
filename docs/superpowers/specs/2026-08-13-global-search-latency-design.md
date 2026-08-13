# Global Search Latency — Design

**Date:** 2026-08-13
**Status:** APPROVED
**Work order:** `docs/telegram-connector-work-orders.md` — Work Order B
**Baseline:** v0.20.0

## Problem

`search_messages` without `channel_id`/`channel_ids` has unpredictable latency.
The work order reports a 60x spread on one code path, worst case 35.3 s — long
enough to exceed an MCP client timeout and fail an entire workflow rather than
degrade. The spread matters more than the peak: a caller who cannot predict the
cost of a call cannot budget calls.

## Measurement

The work order mandates instrumentation before any change. Measured 2026-08-13
against the deployed binary (`/Applications/telegram-mcp`, v0.20.0, live
authenticated session) via a JSON-RPC stdio probe. `wall` is client-observed
round trip; `srv` is the server's own `search_time_ms`.

### Reproduction

| case | wall | srv (ms) | returned | `has_more` |
|---|---|---|---|---|
| global `url` 24h, limit 20 | 0.46 s | 452 | 20 | **true** |
| global `video` 72h, limit 20 | 0.49 s | 483 | 20 | **true** |
| global `voice` 72h, limit 20 | 8.07 s | 8064 | 2 | false |
| global `video_note` 24h, limit 20 | 12.93 s | 12923 | 1 | false |
| global `document` 24h, limit 20 | **44.86 s** | 44859 | 6 | false |

The shape reproduces, worse than reported. One invariant holds across every
row: **fast calls returned exactly `limit` with `has_more: true`; slow calls
returned fewer than `limit` with `has_more: false`.** Fast means the
accumulation loop broke early. Slow means the pager ran to exhaustion.

### The discriminating experiment

Same filter, same window, only `limit` varied:

| case | wall | returned |
|---|---|---|
| `document` 24h, **limit 1** | **0.27 s** | 1 |
| `document` 24h, limit 20 | 44.86 s | 6 |
| `document` 24h, limit 100 | 37.00 s | 6 |
| `document` **72h**, limit 20 | 34.88 s | 20 |

Two results rule out the upstream-is-slow explanation:

1. `limit=1` is **more than 160x faster** than `limit=20` on a byte-identical
   query.
   A single page of document-filtered global search costs 273 ms. Telegram is
   not slow here.
2. Widening the window 24h → 72h made the call **faster** (34.88 s vs 44.86 s)
   while returning more results (20 vs 6). No upstream-cost theory predicts
   that; a client-side walk does — a wider window qualifies more messages, so
   `limit` fills and the loop breaks sooner.

The cost is ours, and it is proportional to how far back the global index gets
walked before `limit` fills. For a rare media type in a narrow window that is
the whole index.

## Root cause

The work order asks whether the path "fans out across dialogs client-side or
issues a single searchGlobal". **It issues a single `messages.SearchGlobal`,
paginated** — there is no client-side dialog fan-out on this path. The defect
is in how the window is enforced.

`RawGlobalSearchPager::new` (`src/telegram/client/raw_pager.rs:364`):

```rust
request: tl::functions::messages::SearchGlobal {
    q: String::new(),
    filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
    min_date: 0,
    max_date: 0,
    ...
    limit: PAGE_LIMIT,   // 100
}
```

Both date bounds are hardcoded to `0`. The TL schema in the pinned grammers
checkout defines them as real, non-flag parameters:

```
messages.searchGlobal#4bc6589a flags:# broadcasts_only:flags.1?true
    groups_only:flags.2?true users_only:flags.3?true folder_id:flags.0?int
    q:string filter:MessagesFilter min_date:int max_date:int
    offset_rate:int offset_peer:InputPeer offset_id:int limit:int
```

So the time window is enforced entirely client-side, in `ops_search.rs:178-185`:

```rust
if let Some(to) = params.to_date
    && timestamp_from_raw(&raw_msg).is_some_and(|t| t > to)
{
    continue; // newer than the requested window; keep iterating toward it
}
if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff_time) {
    continue; // Skip old messages but keep searching
}
```

Telegram streams the global index backwards at 100 messages per round trip and
the client discards everything outside the window, forever, until the index is
exhausted. Only filling `limit` stops it early. Estimated from observed
per-page latency, the 44.86 s case is roughly **180–450 round trips
(18k–45k messages walked and discarded to return 6)**. That estimate is
replaced by a measured count in Task 2.

Conversion time is not a factor: six messages were converted.

### Why `continue` and not `break`

The channel-scoped path at `ops_search.rs:98` uses `break` on the identical
condition. The global path deliberately does not. That asymmetry is defensible
— `break` assumes `searchGlobal` returns strictly date-descending results
across all dialogs, and if that ordering ever interleaves, `break` silently
truncates a result set while reporting success. That is the same
downgrade-that-looks-like-success failure shape Work Order A was written to
eliminate.

**This design does not change `continue` to `break`.** It removes the need to
walk at all.

### Scope evidence — the channel path is not affected

| case | wall | returned | `has_more` |
|---|---|---|---|
| channel `photo` 24h, limit 20 | 1.06 s | 20 | true |
| channel `photo`, 12 h window shifted 60 h into the past | 1.30 s | 20 | true |
| channel `voice` 72h, limit 20 (exhausts) | 0.97 s | 2 | false |
| **global** `photo`, 12 h window shifted 60 h into the past | **6.05 s** | 20 | true |

The channel path stays under 1.31 s even when it exhausts, because one
channel's history is a bounded walk. It is out of scope on evidence, not
assumption.

The last row is the same bug on the `max_date` axis: a global search for a
window in the past spends 6.05 s walking from *now* down to the window's upper
edge, discarding as it goes. **Both** date bounds are wired, not just
`min_date`.

## Design

### 1. Push the window to the server

`RawGlobalSearchPager` gains:

```rust
pub(super) fn window(mut self, from: DateTime<Utc>, to: Option<DateTime<Utc>>) -> Self
```

setting `min_date` from `from` and `max_date` from `to` (`0` when absent —
the protocol's "unbounded" sentinel). `ops_search.rs` wires it from
`params.window_start()` and `params.to_date`, values it already computes for
the client-side filter.

Timestamp conversion is `DateTime<Utc>::timestamp()` clamped into `i32`. A
pre-epoch or overflowing bound clamps to `0` (unbounded) rather than erroring:
a degraded bound costs latency, a rejected search costs the caller their
result.

**The client-side `continue` guards at `ops_search.rs:178-185` are retained
unchanged.** Once the server honors the bounds they filter nothing and cost
nothing; if it ever fails to, they keep the result correct. Defense in depth is
free here.

### 2. Deadline

New `SearchConfig::deadline_seconds`, default 20, `#[serde(default)]`,
validated `> 0` alongside the existing search fields.

**The default of 20 is an estimate, not a measurement.** The work order flags
it as such: it was chosen to sit below typical MCP client timeouts, not derived
from observed search durations. It ships as a configurable default and is
hardcoded nowhere; `config.example.toml` records that it is a conservative
starting point to be tuned against real usage.

It bounds the **accumulation loop**, not an individual round trip: checked at
the top of each iteration, and on expiry the loop breaks and returns what it
has gathered. A single hung MTProto call remains the domain of
`[telegram.timeouts] search_secs`.

The two interact and the ordering is deliberate:

| mechanism | default | on expiry |
|---|---|---|
| `[search] deadline_seconds` | 20 | partial results, `timed_out`/`partial` set, **no error** |
| `[telegram.timeouts] search_secs` | 120 | `Error::Timeout`, call fails |

The deadline must stay below the timeout to be reachable. This is documented in
`config.example.toml` rather than enforced as a cross-table validation
constraint — the tables are independently overridable and a hard coupling would
surprise anyone tuning one without the other.

Applied to **both** branches. A deadline guarding only the path we just fixed
is the wrong shape: the channel path is fast today, but "fast today" is what
the global path was before a rare filter met a narrow window.

Never returns an error for a slow-but-working search. Partial results are
strictly more useful than a failure.

### 3. Response contract

`QueryMetadata` (`src/telegram/types/params.rs:176`) gains four fields:

| field | type | serialization |
|---|---|---|
| `timed_out` | `bool` | omitted when false |
| `partial` | `bool` | omitted when false |
| `pages_fetched` | `u32` | always |
| `messages_scanned` | `u64` | always |

Omit-when-false for the flags matches this repo's established convention and
the work order's own examples, which show them only in the timeout case.

`timed_out` is the cause; `partial` is the consequence. They co-occur today.
They are kept distinct because `partial` is the field a caller checks
generically, which leaves room for byte-budget truncation to set it later
without falsely claiming a timeout.

`pages_fetched` / `messages_scanned` replace the work order's suggested
`dialogs_scanned`. The work order permits "or equivalent", and no dialog sweep
occurs on this path — a `dialogs_scanned` field would name work the code does
not do. Round trips issued and raw messages walked are the work actually
performed, and they are what makes an expensive call legible to its caller.
This follows the precedent the work order's own appendix sets for
`channels_scanned: null`: report the honest quantity, not a plausible one.

**No `next_cursor` on the global path.** `ops_search.rs:149` already rejects
`before_id`/`after_id` without a `channel_id`, because global search has no
per-channel offset to resume from. The work order asks for a cursor "where the
shape allows"; here it does not, and the tool description says so.

The `channel_ids` fan-out (`src/mcp/tools/fanout.rs:86`) builds its own merged
`QueryMetadata`. It must **sum** the two counters and **OR** the two flags
across per-channel results. Dropping them there would reintroduce Work Order A's
path-dependent-enrichment bug in a new field.

### 4. Instrumentation

A `search_global` tracing span with per-page debug events (page index, messages
in page, elapsed), plus separately accumulated MTProto time and conversion
time.

These ship permanently rather than as temporary scaffolding: the same counters
feed `pages_fetched` / `messages_scanned` in the response, so measurement and
reporting are one implementation, and the next regression of this kind is
visible in the response body rather than requiring a bespoke probe.

### 5. Tool description

`search_messages`'s description documents that scoping via `channel_id` /
`channel_ids` is cheaper than a global search, and that global searches cannot
be cursor-paginated.

It is written against post-fix behavior. Warning callers about a 35-second
cliff this change removes would be worse than saying nothing — it would steer
them away from a shape that is no longer expensive.

## Testing

- **Deadline** against a mocked slow client: partial results returned, both
  flags set, **no error propagates**.
- **Pager request construction**: `min_date` / `max_date` land in the TL
  request for an open-ended window, a bounded window, and both clamp cases
  (pre-epoch, `i32` overflow).
- **Counter arithmetic** across the `channel_ids` fan-out merge — sum, not
  drop.
- **Serialization**: `timed_out` / `partial` absent from JSON when false,
  present when true.
- Existing search tests are unmodified. Every change here is additive.

## Verification

**Task 1 of the plan is empirical verification that Telegram honors `min_date`
on `searchGlobal`** — set the bound, rebuild, re-run the probe, compare against
the table above. Nothing is built on top of the assumption before it is tested.
If the bound is not honored, the fallback is the `break` fix in §"Why
`continue` and not `break`", with its truncation risk accepted explicitly
rather than inherited silently.

The probe harness used for every measurement in this document is carried into
the repo so the before/after comparison is reproducible rather than anecdotal.

Acceptance target: `document` 24h limit 20 drops from 44.86 s to well under a
second, returning the same 6 results with `pages_fetched` in the low single
digits.

## Non-goals

- No caching of search results.
- No change to result ordering or filter semantics.
- No lowering of the default `limit` — the work order names this explicitly as
  a way of hiding the problem rather than fixing it.
- No change to the channel-scoped path's termination logic (measured fast).
- No `continue` → `break` change on the global path.

## Risks

| risk | mitigation |
|---|---|
| Telegram ignores `min_date` on `searchGlobal` | Verified first, before anything depends on it; documented fallback |
| Server-side bounds change which results come back, not just how fast | Client-side guards retained; result-set equality asserted against the pre-fix probe output |
| `partial` / `timed_out` read as an error by existing clients | Both omitted when false, so today's responses are byte-identical |
