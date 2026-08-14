# Media batch review fixes — design

**Date:** 2026-08-14
**Status:** DESIGN
**Target release:** 0.22.1 (patch)

## Why

The whole-branch review of the media-throughput work (`d8d298a^1..HEAD`, shipped as
v0.22.0) produced ten findings that survived adversarial verification. One is a
user-visible contract defect; the rest are a rate-limit arithmetic asymmetry, a
latency opportunity, two module-layering inversions, and five duplication/clarity
hazards. None is a crash or data-loss bug, which is why v0.22.0 shipped — but the
contract defect misleads a client that branches on the failure reason, and the
layering inversions are the kind of edge that silently spreads.

This change fixes all ten on one branch, because findings 1 and 10 share a single
restructuring, and 4/5/7 are three faces of the same "constant in the wrong module"
problem.

## Decisions taken before design

| Question | Choice |
|---|---|
| Scope | All ten findings, one branch |
| Cap-exhaustion signal | New typed `Error` variant, no string matching |
| Layering | Move constants to the owning layer **and** open `mod defaults` so the duplicated `DEFAULT_*` constants die |
| Delivery | Feature branch, patch release 0.22.1 |

## Group A — the failure-reason contract (findings 1, 10)

### The defect

`get_messages_media_batch` advertises four per-id failure reasons: `not_found`,
`no_visual_media`, `payload_cap_reached`, `download_failed: <detail>`. Today
`payload_cap_reached` is produced in exactly one place — when `Base64Budget::allowance()`
returns `None` because the remaining budget fell below the floor
(`impl_media.rs:168`). A second, equally cap-driven outcome is mislabelled: when an
image *downloads successfully* but `process_image_with_cap` cannot shrink it under its
allowance within `MAX_CAP_ITERATIONS`, the id is reported as
`download_failed: image could not be reduced below the N-byte payload cap`
(`impl_media.rs:179`).

A client branching on the token takes the wrong action — `download_failed` invites a
per-id retry, which will fail identically, where `payload_cap_reached` invites raising
the cap or splitting the batch.

**What the review did not catch:** `process_image_with_cap` returns
`Error::DownloadFailed` for *both* cap exhaustion and genuine decode/encode failures
(`image.rs:90-92`). So the fix cannot be a string swap at the call site — the two cases
are indistinguishable at the type level today. That is the actual work in this group.

### The fix

1. Add a typed variant to `src/error.rs`:

   ```rust
   #[error("image could not be reduced below the {limit}-byte payload cap")]
   PayloadCapExceeded { limit: usize },
   ```

<<<<<<< HEAD
   The `Display` text is deliberately identical to the string the shrink loop
   formats today, so the single-message path's user-facing error message does not
   change — only its type does.
=======
   **Corrected during execution (was wrong as first written).** The original spec claimed this
   `Display` text made the single-message path's message identical to before. It does not:
   `DownloadFailed(String)` renders with its own `"media download failed: "` prefix, so the old
   rendered text was `"media download failed: image could not be reduced below the N-byte payload
   cap"` and the new variant drops that prefix.

   That change is kept deliberately. Preserving the prefix would make a variant named
   `PayloadCapExceeded` announce a download failure when the download succeeded — the exact lie
   this change exists to remove. The affected string is human-facing prose returned by the
   single-message path, not a machine-readable token (the batch `reason` tokens are the contract,
   and they are fixed properly in Group A). It is recorded as a user-visible change in the
   0.22.1 CHANGELOG.
>>>>>>> origin/master

2. `process_image_with_cap` returns `Error::PayloadCapExceeded { limit: max_base64_len }`
   on loop exhaustion. Every other failure path keeps its current variant.

3. Collapse the two copy-pasted failure arms (`:174-182` and `:194-203`, finding 10)
   into one classification helper beside the existing `failure_reason`:

   ```rust
   fn post_download_failure_reason(error: &Error) -> String {
       match error {
           Error::PayloadCapExceeded { .. } => "payload_cap_reached".to_string(),
           other => format!("download_failed: {other}"),
       }
   }
   ```

   The two arms have different error types — `process_image_with_cap` yields `Error`,
   `json_response` yields `String` — so they cannot be merged into a single `and_then`
   chain without inventing a conversion. They stay two arms, but each becomes one line
   delegating to the helper (or, for the serialization arm, to the token below), which
   is what removes the duplicated body and the duplicated comment.

4. The `json_response` failure arm is neither a download failure nor a cap failure; it
   is a serialization failure, unreachable today (`GetMessageMediaResponse` has no map
   keys or floats — the only things that make `serde_json::to_string` fail) but not a
   compile-time guarantee. It gets its own token, `internal_error: <detail>`.

   **Judgment call, flagged for review:** this adds a fifth token to a documented
   vocabulary, which is itself a contract change, for a path that cannot currently be
   reached. The alternative is leaving it labelled `download_failed`, which perpetuates
   exactly the class of mislabel this group exists to fix. Honesty wins; the cost is one
   README line and one sentence in the tool description.

### Behavior deliberately unchanged

A cap-exhausted image still leaves the budget untouched and still lets the batch
continue to later ids — a smaller image may well fit in the remaining allowance. Only
the label changes.

## Group B — rate-limit arithmetic (finding 2)

The charge at `impl_media.rs:113` uses `saturating_mul`; the matching refund at `:218`
uses unchecked `*`. `RateLimitConfig` is the one config section with no `validate()`, so
an operator can set `media_download_cost` above `u32::MAX / 10` and the refund panics in
debug or wraps in release.

Two changes, both cheap:

1. `:218` becomes `saturating_mul`, matching `:113`. Parity is the point — two spellings
   of the same arithmetic in one function is how the asymmetry survived review once
   already.
2. `RateLimitConfig` gains a `validate()` in the same shape as `SearchConfig::validate`
   and `LimitsConfig::validate`, rejecting a `media_download_cost` or `transcription_cost`
   that exceeds `max_tokens`. Such a config is not merely overflow-prone, it is
   *guaranteed* to fail every call of that kind — the bucket can never hold enough
   tokens. Catching it at startup beats discovering it per call.

   The bound is expressed against `max_tokens`, entirely inside `config`, so it
   introduces no dependency on `MAX_MEDIA_BATCH_IDS` (which lives in the MCP layer —
   reaching for it here would recreate the very inversion Group D removes).

## Group C — CPU offload (finding 3)

A ten-image batch runs up to ten sequential Lanczos3 resize + JPEG encode passes (each up
to five cap-fitting iterations) inline on the async task. The diff parallelized the I/O
half of the pipeline and left the CPU half fully serial on a tokio worker.

**Constraint the obvious fix violates:** encodes run in request order *deliberately*, so
budget allocation is deterministic regardless of which download finished first
(`impl_media.rs:160-161`). Parallelizing the encodes would make the payload cap
allocate against whichever image happened to finish first — a real behavior change, and
a nondeterministic one.

So: keep the loop sequential and in request order, and move each encode into
`tokio::task::spawn_blocking`. This frees the worker without touching allocation order.
`MediaDownload::bytes` is moved into the closure via `std::mem::take`, leaving the rest
of the struct available for `media_metadata`. The single-message path gets the same
treatment for consistency — it is the same one-call change, and it is where the pattern
was copied from.

This is behavior-neutral by construction, so it ships no new assertions of its own; the
existing media tests passing unchanged is the evidence.

## Group D — layering and duplicated constants (findings 4, 5, 7)

Three symptoms, one cause: a constant living in the wrong module, forcing either an
upward dependency or a hand-copied duplicate.

1. **`telegram` → `mcp` (finding 4).** `ops_media.rs:113` reaches up to
   `crate::mcp::tools::fanout::FANOUT_CONCURRENCY` — the only `crate::mcp` reference
   anywhere under `src/telegram/`. Media downloads (multi-hundred-KB binary transfers)
   and search fan-out (small JSON RPCs) have different flood characteristics and no
   reason to share a tuning knob. Fix: `pub(crate) const MEDIA_DOWNLOAD_CONCURRENCY: usize = 4;`
   in `ops_media.rs`, with a comment recording that it currently matches the fan-out
   value but is tuned independently.

2. **`config` → `mcp` (finding 5).** `config.rs:1` imports `MIN_IMAGE_BASE64_BYTES` from
   `crate::mcp::tools::media_budget` to validate `media_batch_max_total_bytes`. Fix:
   move the constant to `src/config.rs`, directly alongside `MAX_SEARCH_DEADLINE_SECONDS`
   — which is already exactly this: a validation-bound constant owned by config.
   `media_budget.rs` then imports it downward (`mcp` → `config`), matching how every
   other config value reaches the MCP layer.

3. **Duplicated defaults (finding 7).** `server.rs` hand-copies
   `DEFAULT_RESPONSE_BYTE_BUDGET` and `DEFAULT_MEDIA_BATCH_MAX_TOTAL_BYTES`, and inlines
   `media_download_cost: 3`, `transcription_cost: 5`, and the two transcription timeouts —
   all because `config.rs` declares `mod defaults;` privately, even though every function
   inside is already `pub(crate)`. Fix: `pub(crate) mod defaults;`, then have
   `McpServer::new` call the real default functions (casting `u64` → `usize` where the
   struct field demands it). Six hand-copied numbers collapse to zero.

   This is the finding with the clearest regression story: change a shipped default and
   any construction path that skips the corresponding `with_*` builder silently keeps the
   old value, with no compiler or test signal. Group F adds the test that closes it.

## Group E — clarity cleanups (findings 6, 8, 9)

- **Finding 6 — duplicated id validation.** The dedupe + cap-check + `parse_message_id`
  loop in `impl_media.rs:78-102` and `impl_message_batch.rs:20-44` are byte-identical
  apart from the cap constant, and both error strings are already parameterized by that
  constant — so one helper reproduces both messages exactly, with no test churn.
  Extract `dedupe_and_validate_ids(ids: &[i64], cap: usize) -> Result<(Vec<i64>, Vec<i32>), String>`
  into `src/mcp/tools/helpers.rs`; both tools call it.

- **Finding 8 — dead padding.** `.chain(std::iter::repeat_with(|| None))` in
  `ops_media.rs:91` can never be pulled from: the pinned grammers rev returns exactly
  `message_ids.len()` slots. Replace with a plain `.zip()` plus a `debug_assert_eq!` on
  the two lengths and a comment citing the grammers guarantee. A bare `.zip()` alone
  would silently drop trailing ids from *both* `content` and `failed` if the guarantee
  ever broke — the assertion states the contract and catches a regression in dev and test
  builds without shipping dead production code.

- **Finding 9 — implicit success count.** `let returned = content.len() / 2;` derives the
  count from the content vector's shape, relying on an invariant (two blocks per success)
  that a future third block would break silently — and the number feeds both the summary
  and the refund. Replace with a counter incremented beside the two pushes.

## Group F — the regression test that pays for Group D

A test asserting that `McpServer::new`'s field values equal the `config::defaults`
functions. Today that test would pass trivially; its value is future — it fails the
moment someone changes a shipped default without the server-side copy, which is the exact
silent desync finding 7 describes.

## Contract and documentation changes

- README §16 (`get_messages_media_batch`) and the `#[tool(description = ...)]` string:
  document `payload_cap_reached` as covering both budget exhaustion *and* an image that
  cannot be shrunk to fit; add `internal_error: <detail>`.
- `config.example.toml`: note the new `[rate_limiting]` validation bound.
- CHANGELOG under a new `[0.22.1]` section; `Cargo.toml`/`Cargo.lock` version bump.
- `docs/memory.md`: the layering lesson (a constant's home determines the dependency
  direction; three symptoms traced to one misplacement) and the
  `process_image_with_cap` lesson (one error variant serving two distinct failure modes
  makes a caller-facing contract unimplementable — the review found the symptom at the
  call site, but the cause was in the callee's type).

## Testing strategy

TDD per repo convention — failing test first, in this order:

| Fix | Test |
|---|---|
| A: cap variant | Unit test in `image.rs`: a high-entropy image against a tiny cap returns `Error::PayloadCapExceeded`, not `DownloadFailed` |
| A: reason mapping | `src/mcp/tests/media_batch.rs`: a mocked download that cannot fit reports `payload_cap_reached`, not `download_failed: …` |
| A: internal_error | Mapping unit test on the classifier helper |
| B: refund | `media_download_cost` near `u32::MAX` refunds without panicking |
| B: validation | Config test: cost > `max_tokens` is rejected (serial, per config convention) |
| C: spawn_blocking | None new — existing media tests must pass unchanged |
| D: constants | Group F equality test; the two inversions are enforced by the compiler once the imports are gone |
| E: helper | Unit tests for `dedupe_and_validate_ids` (dedupe order, cap message, i32 range); both tools' existing tests must pass with strings unchanged |
| E: counter/assert | Existing batch tests cover |

Full gate before merge: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`,
plus `cargo test config -- --test-threads=1` for the new config test.

## Risks and non-goals

- **The `internal_error` token is a vocabulary expansion.** Flagged above; easy to drop in
  review if the fifth token is judged worse than the mislabel.
- **`spawn_blocking` per image adds a task-dispatch hop.** Negligible against a
  Lanczos3 resize, and it cannot change results — but it is the one change here that
  touches the runtime, so it is the one to watch in the live re-run.
- **Not in scope:** the `media_download_cost = 0` free-flood degeneracy (pre-existing and
  explicitly acknowledged in the code), overlapping encodes with downloads (breaks
  deterministic allocation, see Group C), and any change to the single-message media
  tool's response shape.
- **No live re-run is strictly required** — every change is offline-testable — but a
  short probe against a real session after merge is cheap insurance for Group C, and the
  harness is already in `scripts/`.
