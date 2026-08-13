# Media Throughput — Design

**Date:** 2026-08-13
**Status:** APPROVED
**Work order:** `docs/telegram-connector-work-orders.md` — Work Order C
**Baseline:** v0.21.0

## Problem

`get_message_media` returns exactly one image per call, and a digest run
covering 10–15 visual posts issues 10–15 separate round trips.

The work order attributes the cost to the rate limiter. That is only the
visible half, and its arithmetic is based on stale defaults.

### Correcting the work order's premise

The work order states "capacity 30 refilling at 1 token/sec", giving "6 images
immediately, then one per 5 seconds". The actual defaults are `max_tokens = 50`
and `refill_rate = 2.0` (`src/config/defaults.rs:34-40`), so the real behaviour
at `media_download_cost = 5` is **10 images immediately, then one per 2.5 s** —
about 12.5 s of blocking across 15 images, not "over a minute".

The rate limiter is therefore a smaller contributor than the work order
believes. That does not make the retune wrong, but it does mean the retune
alone would not have delivered the throughput the work order is after. The
dominant cost is the per-call overhead below.

## What the cost actually is

Reading the existing path (`src/telegram/client/ops_media.rs:9`), one
`get_message_media` call performs:

1. `resolve_peer(channel_ref)` — for a **numeric** channel id this walks the
   entire dialog list (`src/telegram/client/resolve.rs:17-22` →
   `find_dialog_peer`, which iterates `iter_dialogs()` to completion looking for
   a matching bare id). There is no cache.
2. `get_messages_by_id(peer_ref, &[message_id])` — one RPC for one message.
3. `iter_download` of the selected size variant.

So N images fetched the obvious way cost **N dialog walks + N message-fetch RPCs
+ N downloads**, all serialized. Steps 1 and 2 are pure per-call overhead: every
id in a batch shares one channel, so both are performed N times to produce
information that one call could have produced once.

A batch built only at the MCP layer — looping the existing tool with bounded
concurrency — would parallelize that overhead without removing it. The design
below removes it.

This does not contradict the work order's "reuse that code path; do not fork
it". The rule it protects is that photo-vs-thumbnail selection must not drift
between the two tools. That is preserved by extraction (§1), which is stronger
than reuse-by-calling: the rules end up in exactly one function that neither
tool can bypass.

## Design

### 1. Client layer — resolve once, fetch once, download concurrently

Add to `TelegramClientTrait`:

```rust
async fn download_messages_media(
    &self,
    channel_ref: &str,
    message_ids: &[i32],
    max_dimension: u32,
) -> Result<Vec<MediaFetchOutcome>, Error>;

pub struct MediaFetchOutcome {
    pub message_id: i32,
    pub result: Result<MediaDownload, MediaFetchError>,
}

pub enum MediaFetchError {
    NotFound,
    NoVisualMedia { media_type: String },
    Failed(Error),
}
```

The **outer** `Result` fails only on channel-level failures — empty reference,
channel not found, resolve or fetch RPC error — cases where no id could have
succeeded, so failing the call is honest rather than a batch-wide downgrade.
Per-id failures live in the **inner** `Result`. The shape mirrors
`fanout::ChannelFetchOutcome`, which already models exactly this
partial-success case for the `channel_ids` search fan-out.

The inner error is a **typed enum, not `Error`**. §4 requires a stable
machine-readable reason token per failed id, and not-found is not a distinct
`Error` variant: `guard::not_found` (`src/telegram/client/guard.rs:18-27`)
returns `Error::InvalidInput` carrying the reason only in its message text.
Mapping that to a `not_found` token would mean string-matching prose, coupling
the wire contract to a log message. `MediaFetchError` discriminates at the
client layer, where the distinction is structural — `require_found` already
separates the case, covering both an absent slot and Telegram's `MessageEmpty`
placeholder. The MCP layer's mapping is then a total match, so a future variant
is a compile error rather than a silent fall-through.

`download_message_media_impl` splits at the fetch boundary. Everything after
`require_found` — the `msg.media()` match, the photo-vs-thumbnail rules, the
`size_candidates` / `select_size_candidate` selection, the `max_download_bytes`
pre-check, the streaming re-check inside `iter_download`, caption extraction and
`MediaDownload` assembly — moves verbatim into:

```rust
async fn media_download_from_message(
    &self, msg: Message, channel_ref: &str, message_id: i32, max_dimension: u32,
) -> Result<MediaDownload, Error>
```

Both entry points then become thin:

| | single | batch |
|---|---|---|
| resolve | `resolve_peer` ×1 | `resolve_peer` ×1 |
| fetch | `get_messages_by_id(&[id])` | `get_messages_by_id(&all_ids)` |
| per message | helper ×1 | helper ×N, `futures::stream…buffered(FANOUT_CONCURRENCY)` |

`FANOUT_CONCURRENCY` is the existing constant (`src/mcp/tools/fanout.rs:14`,
value 4), reused rather than duplicated per the work order.

A `None` slot in the `get_messages_by_id` result becomes a per-id not-found
outcome, matching how `get_messages_batch` already reports deleted ids.

**Testability limit, stated up front:** "resolve happens exactly once" is
enforced by construction, not by an assertion. `TelegramClient` *is* the mock
boundary — `MockTelegramClientTrait` replaces it wholesale, so there is no seam
below it against which to count `resolve_peer` calls. The property is
guaranteed by there being a single `resolve_peer` call site in the batch impl.

This is the codebase's existing situation, not a new gap:
`src/telegram/tests/client_tests.rs` exercises the *mock*, not
`TelegramClient`, so no `*_impl` method in `src/telegram/client/` carries a
unit test today. The new impl inherits that. Its verification is the compiler,
the MCP-layer tests against the mocked trait, and live acceptance.

### 2. MCP tool — `get_messages_media_batch`

`GetMessagesMediaBatchRequest` in `src/mcp/tools/types/requests.rs`, following
`GetMessagesBatchRequest` field-for-field:

```rust
pub struct GetMessagesMediaBatchRequest {
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,
    pub message_ids: Vec<i64>,
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub max_dimension: Option<u32>,
}
```

Validation order copied from `impl_message_batch.rs:13-44`: reject empty
`channel_id`, reject empty `message_ids`, dedupe silently preserving first-seen
order, reject more than `MAX_MEDIA_BATCH_IDS` (10) ids, then per-id
`parse_message_id` plus i32-range check.

`max_dimension` defaults to 1280 and clamps to 64–2048. The three constants
currently local to `get_message_media_impl` are hoisted to module scope in
`impl_media.rs` so both tools share one definition.

The tool lives in `src/mcp/server/impl_media.rs` next to the single-message
tool, with its `#[tool]` wrapper in `server.rs` like every other tool.

### 3. Payload budget

New pure module `src/mcp/tools/media_budget.rs` — no I/O, unit-testable in
isolation:

```rust
pub struct Base64Budget { remaining: usize }
impl Base64Budget {
    pub fn new(total: usize) -> Self;
    /// Bytes this image may occupy, or None once the budget is exhausted.
    pub fn allowance(&self) -> Option<usize>;
    pub fn consume(&mut self, actual: usize);
}
```

Downloads complete concurrently; **encoding then runs in request order**, so
allocation is deterministic regardless of which download finished first.

Each image is handed `min(image::MAX_BASE64_LEN, remaining)` as its cap and
processed by the existing `process_image_with_cap`
(`src/mcp/tools/image.rs:44`), whose loop already shrinks the target dimension
iteratively — by `sqrt` of the byte ratio, never less than 10% per round — until
the encoded payload fits. That loop *is* the work order's "progressively reduce
`max_dimension` for remaining images"; no new shrink logic is written. The only
change to `image.rs` is widening `process_image_with_cap` from private to
`pub(crate)`.

Two terminal cases:

- **Budget exhausted.** When `remaining < MIN_IMAGE_BASE64_BYTES` (32 KiB —
  below that an image is downscaled past usefulness), `allowance()` returns
  `None`; the current id and every subsequent id fail with reason
  `payload_cap_reached`. Downloads for those ids have already occurred, so they
  are refunded at the rate limiter (§5) but not returned.
- **Single image won't fit.** `process_image_with_cap` exhausts its 5 iterations
  and returns `Err`. That id fails individually with `download_failed: …`; the
  budget is left untouched so later ids still get their full allowance.

### 4. Response contract

Content blocks, in request order:

```
[ image, metadata, image, metadata, …, summary ]
```

Each `metadata` block is the existing `GetMessageMediaResponse`, byte-identical
to what `get_message_media` emits for the same message. Association between an
image and its metadata is positional and adjacent, which stays unambiguous when
some ids fail — an index into a parallel array would not.

The trailing summary is a text block:

```rust
pub struct MediaBatchSummary {
    pub channel_id: String,
    pub requested: usize,
    pub returned: usize,
    pub failed: Vec<MediaBatchFailure>,
    pub total_base64_bytes: usize,
    pub max_total_bytes: u64,
}
pub struct MediaBatchFailure { pub id: i64, pub reason: String }
```

Reason values: `not_found`, `no_visual_media`, `payload_cap_reached`,
`download_failed: <detail>`.

The field is named `reason`, not `error` as in `get_messages_batch`'s
`MissingMessageEntry`, because `payload_cap_reached` is a machine-readable token
a caller is expected to branch on, whereas `error` there carries prose. The
behavioural convention the work order names — per-id failures never fail the
batch — is followed exactly.

A batch of 1 therefore emits `[image, metadata, summary]` where the first two
blocks equal the single tool's output for that message. The quality gate asserts
that pair-equality; it cannot assert whole-array equality, because the summary
is genuinely new information.

### 5. Rate limiting

Add to `RateLimiterTrait`:

```rust
fn refund(&self, tokens: u32);
```

The bucket already clamps to `max_tokens` on every refill
(`src/rate_limiter.rs:29`), so an over-refund cannot inflate capacity — safety
is by construction, not by a check.

The tool acquires `media_download_cost × requested_ids` **before any network
work**, then refunds `media_download_cost × (requested − returned)` once results
are known. Net charge equals the work order's "cost × images actually returned,
failed ids cost nothing", while admission control remains real: the limiter can
still refuse a batch the bucket cannot afford, and the D5 deficit message
("requested N tokens, M available") stays accurate because the acquire is a
single atomic call for the whole batch — the same pattern the `channel_ids`
search fan-out already uses (`impl_search.rs:126-131`).

Config changes, all in existing tables:

| key | table | actual current | new |
|---|---|---|---|
| `max_tokens` | `[rate_limiting]` | **50** (not 30) | 60 |
| `media_download_cost` | `[rate_limiting]` | 5 | 3 |
| `refill_rate` | `[rate_limiting]` | **2.0** (not 1.0) | unchanged |
| `media_batch_max_total_bytes` | `[limits]` | — | `8_388_608` (8 MiB) |

Two notes on the work order's text. The existing key is `media_download_cost`,
not `media_download`. And its "30" baseline is stale — the current default is
50, so this is a 50→60 raise, not 30→60. `refill_rate` stays at 2.0; the work
order does not propose changing it and nothing here argues for it.

Post-retune, at cost 3 against capacity 60 refilling at 2.0/sec: 20 images
immediately, then one per 1.5 s. A 15-image digest fits entirely in the burst.

`media_batch_max_total_bytes` counts **bytes of base64 payload as sent to the
client** — the quantity that actually consumes context, and 4/3 the size of the
underlying JPEG bytes. It is validated `> 0` alongside `response_byte_budget`.

All three values are estimates, not calibrated against Telegram's flood
thresholds. `config.example.toml` says so explicitly. None is hardcoded at a use
site.

`retry_after_seconds` is **already implemented** — `Error::RateLimit` carries it
(`src/error.rs:12-18`) and `TokenBucket::try_acquire` computes it
(`src/rate_limiter.rs:44`). No work is required; a regression test pins the
behaviour so it is not lost, and the CHANGELOG does not claim it as new.

### 6. Status reporting

`StatusResponse` gains an additive sibling of `rate_limiter`:

```rust
pub struct MediaLimits {
    pub batch_max_ids: usize,        // MAX_MEDIA_BATCH_IDS
    pub max_total_bytes: u64,        // configured cap
    pub per_image_max_bytes: usize,  // image::MAX_BASE64_LEN
    pub default_max_dimension: u32,  // 1280
    pub max_dimension_limit: u32,    // 2048
}
```

Purely additive — no existing field renamed, retyped, or removed — so a caller
can plan a run instead of discovering the limits by hitting them.

## Testing

TDD order, each layer failing before its implementation exists:

1. **`media_budget` (pure).** Allowance clamps to per-image cap; allowance
   shrinks as budget is consumed; `None` once below the floor; a zero-cost
   failure leaves the budget unchanged.
2. **Request validation.** Empty `channel_id`; empty `message_ids`; 11 ids
   rejected with the count in the message; duplicates collapsed preserving
   order; `max_dimension` clamped at both ends; flexible-scalar coercion of
   `channel_id` and `max_dimension` (numeric string, JSON number).
3. **Tool behaviour, `MockTelegramClientTrait`.**
   - Mixed batch — two photos, one video thumbnail, one no-visual-media id, one
     deleted id — yields 3 image blocks, 3 adjacent metadata blocks, and a
     summary whose `failed` holds exactly the two ids with `no_visual_media` and
     `not_found`.
   - Payload cap: a small configured cap returns the first image at full
     allowance, the second visibly downscaled, and the third and later ids as
     `payload_cap_reached`.
   - Channel-level failure (channel not found) propagates as a tool error, not
     as ten per-id failures.
   - Batch of 1: image and metadata blocks equal `get_message_media`'s for the
     same fixture.
4. **Rate limiter.** `MockRateLimiterTrait` expects `acquire(15)` then
   `refund(6)` for 5 requested / 3 returned; `refund(0)` when all succeed; a
   rejected `acquire` performs no download (mock client expects zero calls).
   Separately, `RateLimiter::refund` clamps at capacity.
5. **Status.** `check_mcp_status` reports the configured cap rather than a
   literal, proving nothing is hardcoded.
6. **Regression.** Rate-limit error string still carries `retry after N
   seconds`.

Gate: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`.

## Verification

Manual acceptance requires an authenticated session (see the standing note in
`docs/memory.md`). When one is available: a batch of 10 visual posts from one
channel against 10 sequential `get_message_media` calls, comparing wall time and
confirming a single dialog walk in the logs.

## Documentation

README tool reference (new tool + the throughput guidance),
`config.example.toml` (three keys, flagged as conservative estimates),
CHANGELOG, `docs/tasklist.md`, `docs/memory.md`.

## Non-goals

- No full video download — thumbnails only, same rule as the existing tool.
- No disk caching of downloaded media.
- No changes to transcription.
- No peer-resolution cache. Resolving once per batch is in scope; a
  cross-call cache is a separate concern with its own invalidation questions.
- No multi-channel batching. `channel_id` is singular, per the work order; the
  resolve-once win depends on one channel per call.

## Risks

- **Extraction regression.** Moving the selection rules into a shared helper
  touches the single tool's behaviour. Mitigated by the byte-identical
  batch-of-1 test and by the existing `get_message_media` test suite, which must
  pass unchanged.
- **Memory during a batch.** Up to 4 concurrent downloads are buffered in
  memory, each bounded by `max_download_bytes` (20 MiB default). Worst case
  ~80 MiB transient. Real thumbnails and 1280px variants are far smaller, but
  the bound is the bound.
- **Uncalibrated defaults.** 60 / 3 / 8 MiB are estimates. Documented as such;
  every one is config-driven so a deployment can correct them without a rebuild.
