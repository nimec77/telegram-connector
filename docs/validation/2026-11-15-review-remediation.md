# Review remediation — final local validation

Date: 2026-11-15. Branch: `fix/review-remediation`. Package: **0.26.0**.
Specification baseline: `aeaceab43e3403d1d9e18486c5f229a18ee82d74`.
Implementation source baseline: `f4375a6ce90844a0467ae82e8e4dcb81cb83eef7`.
Final validated source: `13fda27e58c2d422cc7518b2e098f4de8bd2f037`.

All 15 implementation tasks have independent scoped approval in the controller's task
ledgers. The final combined macOS matrix below passes on that source. **Fresh independent
whole-branch review approved all 19 dispositions and this evidence artifact on 2026-11-15**,
reviewing `f4375a6..66706b0`, with no Critical/Important findings and one nonblocking
test-helper minor below. This report records pre-integration local acceptance;
hosted CI execution remains separate. The completed
[specification](https://github.com/nimec77/telegram-connector/blob/dab4de3bb32e8487a9835df8de7c1f5d9831e071/docs/superpowers/specs/2026-11-15-review-remediation-design.md) and five
[implementation plans](https://github.com/nimec77/telegram-connector/tree/dab4de3bb32e8487a9835df8de7c1f5d9831e071/docs/superpowers/plans/)
are archived in Git history.

## Evidence and scope

Local raw evidence is retained in the ignored workspace
`.superpowers/sdd/2026-11-15-remediation-maintenance-performance/task-4-logs/` (abbreviated
`final-logs` below). `gates.json` records each exact command, UTC start/end and exit;
`branch.diff`, `lock-comparison.json`, tool/platform/features logs and measurement comparison
JSON preserve the inspection inputs. These local logs are not committed portable artifacts;
this report records their results and the committed regression locations.

Per-task RED/GREEN detail and independent review dispositions are in
`.superpowers/sdd/2026-11-15-remediation-{runtime-sdk,read-correctness,sending-input,configuration,maintenance-performance}/`:
`task-N-report.md`, sibling logs and `progress.md`. The runtime report's Fix round 1 and
read Task 4's Fixround1 supersede the original unresolved findings/incorrect minimal-creator
policy. Earlier failed attempts remain in those reports; they are not counted as green gates.
Some task reports retain their original review-pending handoff status; the controller's
later `progress.md` approval entries are authoritative for completed scoped reviews.
This final sweep had no failed command, suppressed rerun, new ignore or weakened assertion.

The complete pre-integration diff contains 104 files, 8,784 insertions and 739 deletions,
including five active plans and test fixtures. Final integration adds documentation only.
Source/tests/dependencies were unchanged during this sweep. Initial working tree was clean.
The full Cargo lock comparison against `f4375a6` proves 341 package records before/after,
with **339 records exactly unchanged**. The only changes are rmcp and rmcp-macros
3.8.0→3.9.0 versions/checksums and rmcp's edge to **already-locked sha2 0.11.1**.
No lock regeneration or new package identity was accepted. Manifest floor is `rmcp = "3.9"`.
All three direct grammers dependencies retain Codeberg revision
`1e31e73cd9797790905bbea82cd75adcbe6f1242`; nightly remains `2026-10-31`.

Fresh `cargo tree --offline --locked -e features -i rmcp` shows exactly `base64, default,
macros, schemars, server, transport-async-rw, uuid`. HTTP client transports remain disabled.
The baseline/current feature comparison is retained in runtime-sdk Task 2; only the required
SHA-2 subtree joins the dependency tree. Both MCP 2026-07-28 and 2025-11-25, `json_v1` and
explicit legacy reads, 16 default/18 sending-enabled tools, string source/user IDs, integer
message IDs, direct Rust adapters and self-contained advertised schemas remain covered.

## All 19 review dispositions and acceptance cases

Each row is implemented and scoped-review approved unless explicitly qualified. The final
full test/process gates rerun every normal regression; RED history belongs to its owning
report, with concrete counterevidence distinguished from behavior defects.

| Review / case | Disposition, production change and regression evidence | Commit(s); owning report |
| --- | --- | --- |
| R01 / A01 | Immutable admitted operation handles and private SDK correlation prevent old writes/queued errors/progress from binding to reused IDs/tokens; original wire identity, metrics and keyed recovery stay separate. One-byte/queued matrices, numeric/string IDs, both protocols, cancellation, EOF and shutdown release storage. Followup RED reproduces normal sequential reuse under withheld flush and retired-writer EOF drain, then GREEN with terminal observation. Real permit contention reproduces the paused-media fixture failure; manual virtual-time coordination preserves its original deadline/refund/partial assertions. | `9f42b59`, `d17ddaf`; runtime-sdk Task 1. `src/mcp/runtime/tests/transport.rs`, `src/mcp/tests/protocol_{progress,recovery,budgets}.rs`. Rulings/minor below. |
| R02 / A02 | Separate envelope decoding and scope exhaustion; global slices stop only when empty or terminal-unsliced. RED actual mixed 100-message TL page starting at ID 9 now fetches the later match. Real pager/walker tests pin final raw peer/id/rate, 101 raw items/two pages, short/missing-rate continuation, exact-limit vs extra qualifying result and one total deadline. Existing single-peer policy and ±1-second server bounds stay intact. | `447951f`; read-correctness Task 1. `src/telegram/client/tests/{raw_page_tests,raw_pager_tests}.rs`, `src/mcp/tests/search_shaping.rs`. |
| R03 / A03 | Common `(Option<namespace>, channel_id, grouped_id)` album key in counting, buckets and ordered slots. RED equal User/Chat/Channel bare IDs no longer merge; same-source siblings/representative/caption and admitted raw overflow remain. Real TL walker and canonical/legacy output exercise the production path; unknown namespace remains `None`. | `bb61839`; read Task 2. `src/telegram/albums.rs`, client `walk_tests.rs`, MCP `search_shaping.rs`. |
| R04 / A04 | Only TL `Peer::User` creates `UserId`; names resolve independently. RED Channel/Chat IDs now null, known User IDs survive missing names, absent sender stays absent in direct TL and canonical/legacy tests. | `bb61839`; read Task 2. `src/telegram/tests/message_tests.rs`, MCP `search_shaping.rs`. |
| R05 / A05 | Validate all inputs before atomic acquisition; locally normalize optional @/ASCII case/whitespace/numeric spelling, preserving identifier kinds and first original error spelling. First complete full message identity wins before sort/truncate. RED charge/duplicate/source-count tests cover both routes, actual numeric-plus-username attempts, no failed-attempt refunds, namespaced returned-source counts and no duplicate-only `has_more`. Distinct surviving compact-header conflicts remain errors. | `d37fbfd`; read Task 3. `src/mcp/tools/fanout.rs`, `src/mcp/tests/multi_channel.rs`, response/backend count tests. |
| R06 / A06 | Capture raw membership before forbidden SDK normalization; all minimal username observations are unconfirmed/false, even creator. Full nonminimal/nonleft flags establish membership independently of hash; Dialog/my_results remain positive. Original and fix-round REDs cover all pinned Chat variants through actual basic/full/unmatched-batch paths, typed entire-envelope cache behavior, one RPC, safe cache failure and shared deadline; wire booleans match both modes. | `1721557`, `024db41`; read Task 4 + Fixround1. `src/telegram/client/tests/subscription_tests.rs`, MCP `channels.rs`/`resolve.rs`. Earlier minimal-creator exception is superseded. |
| R07 / A07 | URL-only raw predicate sees Url/TextUrl entities and nonempty Page/Empty/Pending URLs before media conversion, with literal HTTP(S) fallback. RED real TL converter and production history walker now admit entity/metadata-only links; empty/NotModified/display-only/non-URL negatives and unrelated filters stay intact. | `696aa41`; read Task 5. `src/telegram/tests/converters_thumb_forward_tests.rs`, client `walk_tests.rs`. |
| R08 / A08 | Separate preparation-read classification: I/O/DC/decode/unknown provider → safe unavailable/not_sent; definitive absence/auth/flood/deadline retain their categories. RED cached-recipient/reply/topic paths exercise real helper seams and safe diagnostics. UploadFailed and post-dispatch delivery_unknown/no-resend remain unchanged. | `3880914`; sending-input Task 1. Client `recipient_tests.rs`, `send_target_tests.rs`, `send_transport_tests.rs`, `send_media_tests.rs`. |
| R09 / A09 | Bounded snapshot/deadline reader skips checked JPEG APP/COM and ID3 metadata, restores original position on success/failure, and preserves caller rewind/readonly handoff. RED large valid JPEG/MP3, cursor, marker/read/snapshot/deadline and malformed-after-SOF cases now pass. Real staging/MCP/process checks assert MIME, size and cleanup. JPEG independently decodes with existing image codec; synthetic checked-in 1,252-byte MP3 and large-tag form decoded with mpg123/FFmpeg during fixture generation, not production. | `b44e325`; sending Task 2; `src/sending/tests/{probe_tests,reader_tests,media_fixtures}.rs`, fixture provenance README, MCP `sending_media.rs`, `scripts/test_mcp_probe.py`. |
| R10 / A10 | Actual interactive-password orchestration forwards owned reader String unchanged. RED captured byte submission now preserves ASCII/Unicode whitespace, tabs and CRLF data; reader failure prevents submit and async-local diagnostic assertions exclude credentials/false success. Receiving wrapper's bytes/timeout/sanitization remain unchanged; no real terminal/live auth claim. | `4aca08a`; sending Task 3. `src/telegram/tests/auth_tests.rs`. |
| R11 / A11 | Value-position masking plus native spanned DeTable and same-tree two-pass storage expands string data once without raw TOML injection; declared numeric/bool paths alone coerce whole scalars. Ten RED cases plus unknown/string compatibility RED, original physical escape/folding/CRLF locations, native u64 limits, preserved unknown-field policy/Sending denial and safe Display/Debug/source chains pass. Actual loader children use Command.env/env_remove; no process mutates its environment. | `bfe7bc5`; configuration Task 1. `src/config/tests/{env_tests,load_tests,loading_tests}.rs`. Bare-value ruling below. |
| R12 / A12 | Reviewed rmcp/macros 3.9.0 fixes RUSTSEC-2026-0462 without ignores or HTTP feature enablement. Baseline fresh audit/deny RED on 3.8.0, reviewed lock/feature diff and current fresh audit/deny/policy GREEN. Entire final protocol/full suite, all four clients and release build pass. Earlier SDK integration failures were explicitly fixed in `d17ddaf`, not waived by reruns. | `15871ba` + runtime followup; runtime-sdk Task 2. `Cargo.toml`, `Cargo.lock`, final matrix below. |
| R13 / A13 | Existing process/client CI step now invokes artifact script against the same built fixture on both ubuntu-latest/macos-latest. Pinned actions/read permissions/45-minute timeout unchanged. Rebuilt local modern/legacy matrix and all-workflow actionlint pass; hosted runs not performed. | `3c5deec`; maintenance-performance Task 1. `.github/workflows/ci.yml`, `scripts/test_artifact_clients.py`. |
| O01 / A14 | Qualified exact namespace+ID returns immediately from real dialog traversal. RED first match followed by would-fail/pending page succeeds without polling later page; absent/mismatch remains bounded. Unqualified numeric read ambiguity scan stays intact. Private owned-state FnMut traversal accommodates Send without public API change. | `3880914`; sending Task 1. Client `recipient_tests.rs`. |
| O02 / A15 | Explicit wire/direct fitting policy removes redundant raw pass and metadata body clones, retains accepted rendering, and safely skips impossible prefixes by actual required-row lower bound. Real route underfill RED (six expected/one returned) fixed. Exact frame/ID parity includes formats/protocols/escaped IDs/Unicode/compact/cursors/omissions/floors/metadata overflow; direct adapter behavior retained. 144 fresh work samples match baseline frame lengths/retained IDs and saved after counters exactly. | `3a7edc5`; maintenance Task 2. `src/mcp/tests/{protocol_budgets,search_shaping}.rs`, `src/mcp/tools/{wire_budget,wire_work}.rs`; measurements below. |
| O03 / A16 | One borrowed path map under original mutex, ownership lock inserted last; same fresh filesystem identity validation, unavailable transition, pending exclusion, missing-file semantics, expiry/eviction/pins/cleanup. Four real 1000-entry characterizations passed before/after; actual work measurements establish expected/amortized linear map operations. No invented behavior RED for a pure optimization. | `13fda27`; maintenance Task 3. `src/mcp/artifacts/tests.rs`, existing MCP artifact/wire tests; measurements below. |
| O04 / A17 | Explicit positive capacity and finite-positive refill validation through public API/loader. Broad premise partly disproved: NaN and nonpositive refill already rejected. Four actual REDs expose +infinity/zero-capacity-all-costs-zero; full invalid matrix now rejects while finite-positive/zero read costs and enabled-send stricter rules remain. Constructor compatibility unchanged. | `9fe0a37`; configuration Task 2. Config `validation_tests.rs`, `load_tests.rs`, `sending_tests.rs`. |
| O05 / A18 | Current memory replaces removed environment guard/lock/serial guidance with injected lookups and child-only environment configuration; inspect against actual parent/child fixtures. Historical reports preserved. Prose/YAML changes validated by inspection/execution rather than wording unit tests. | `3c5deec`; maintenance Task 1. `docs/memory.md`, config test fixtures. |
| D01 / A19 | Current README/tool descriptions/request schema/contract/memory describe available account global index, possibly private conversations/groups/channels, without exhaustive-public coverage claims. Converter comment corrected. Same RPC flags/eligibility, User/Chat/Channel results and canonical namespaces remain; global/fan-out have no message-ID cursor. Scoped subscribed-dialog resolution unchanged. | `447951f`, `bb61839`; read Tasks 1–2. Same actual pager/walker/output tests as A02–A04. |

## Fresh final command matrix

Native macOS **27.2.1 (26B131), arm64**. Selected
`nightly-2026-10-31-aarch64-apple-darwin`; rustc **1.101.0-nightly
(92b5b8c87 2026-10-29)**; cargo **1.101.0-nightly (31d3246e0 2026-10-27)**.
Default package features; cached locked dependencies; mock/inert endpoints only.
Commands ran sequentially on 2026-11-15, **08:43:48–08:44:44 UTC**.
Security tools: cargo-audit **0.22.0**, cargo-deny **0.20.2**, actionlint **1.7.12**.
The latter two use already-present temporary tools, with no new global install.

| Exact command | Exit / actual result | Log in final-logs |
| --- | --- | --- |
| `cargo fmt --all --check` | 0 | `fmt.log` |
| `cargo clippy --offline --locked --all-targets -- -D warnings` | 0, warning-free | `clippy.log` |
| `cargo test --offline --locked` | 0; **1,188 passed, 0 failed, 12 ignored; binary 0 tests; 3 doctests passed** | `tests.log` |
| `RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps` | 0, warning-free | `rustdoc.log` |
| `cargo build --offline --locked --release` | 0; final source release rebuilt | `release.log` |
| `cargo build --offline --locked --example offline_mcp_fixture` | 0; current source fixture checked/built before scripts | `fixture.log` |
| `python3 -B scripts/test_stdio_process.py target/debug/examples/offline_mcp_fixture` | 0; idle/pending EOF, signals and saturated bootstrap | `stdio.log` |
| `python3 -B scripts/test_mcp_probe.py target/debug/examples/offline_mcp_fixture` | 0; modern/legacy and genuine metadata-heavy JPEG/MP3 sending | `probe.log` |
| `python3 -B scripts/test_media_clients.py target/debug/examples/offline_mcp_fixture` | 0; typed/legacy × modern/legacy, previews, keyed replay, mixed batches | `media.log` |
| `python3 -B scripts/test_artifact_clients.py target/debug/examples/offline_mcp_fixture` | 0; six modern/legacy groups, bytes/indices/errors/cache/EOF, expiry/tamper, disabled/legacy-output rejection | `artifact.log` |
| `python3 -B -m unittest discover -s scripts -p test_dependency_policy.py` | 0; **25 tests** | `policy-fixtures.log` |
| `python3 -B scripts/check_dependency_policy.py --offline` | 0; **340 external packages**; root licensing remains unassigned | `policy.log` |
| `cargo audit --deny unsound --deny yanked` | 0; fetched advisory DB/updated crates.io index; **1,509 advisories, 341 dependencies**, no findings | `audit.log` |
| `/private/tmp/review-remediation-tools/bin/cargo-deny --locked check sources advisories` | 0; advisories ok, sources ok | `deny.log` |
| `/private/tmp/review-remediation-actionlint/actionlint -shellcheck= -pyflakes=` | 0; all workflows, no diagnostics; external shellcheck/pyflakes disabled as specified | `actionlint.log` |

Fresh audit network access ran **08:44:31–08:44:40 UTC**; deny **08:44:40–08:44:44 UTC**.
These are advisory snapshots, without new ignores. The primary advisory/release/source
review is recorded in runtime-sdk Task 2 and [dependency status](../dependency-status.md).
No Linux/Windows provisioning, live Telegram/authentication/account sends, hosted Actions,
model/desktop acceptance or global/private configuration change was performed. Scripts
use their disposable explicit roots and inert endpoints, without account sessions.

The 12 ignored tests are **seven subprocess child entrypoints**, exercised by ordinary
parent tests; **two original opt-in cached-transfer latency/media RSS measurements**; and
**three new opt-in O02 serialization/O03 timing/O03 work measurements**. The fresh two work
runs below are additional explicit passes, not additions to the 1,188 default-pass count.
No default test was newly skipped to obtain this result.

## Optimization evidence

### Wire fitting (O02)

Baseline instrumented real fitting and final source use the same deterministic harness:

```sh
cargo test --offline --locked mcp::tools::wire_budget::work::o02_serialization_work -- --ignored --exact --nocapture --test-threads=1
```

Fresh final run: exit 0, one explicitly selected ignored measurement passes, **144 samples**.
`final-logs/o02-work.log` and `o02-comparison.json` recompute against the owning Task 2
`task-2-measurements.json`: **zero retained-ID/frame-byte mismatches** and **zero current
counter differences from the saved optimized samples**. Search/history sizes 1/20/100,
public batch sizes 1/20/50; both protocols, output formats, full/compact, budgets 1,000,000
and 14,000. Batch full/compact repeats are controls because batch has no format request;
144 samples do not mean 144 distinct public request shapes.

| Actual counted fitting work, all samples | Before | Final |
| --- | ---: | ---: |
| Preliminary DTO serializations / input rows | 1,616 / 88,440 | 0 / 0 |
| Preliminary serialized bytes | 54,341,392 | 0 |
| Required-row serializations / input rows | 0 / 0 | 4,960 / 2,480 |
| Required-row serialized bytes | 0 | 3,617,056 |
| Metadata renders / rows copied solely for metadata | 584 / 10,224 | 2,056 / 0 |
| Candidate renders / input rows | 584 / 10,224 | 240 / 6,192 |
| Accepted final rerenders | 144 | 0 |
| Complete-frame serializations / output bytes | 1,168 / 13,824,984 | 2,296 / 9,525,536 |

Tracked preliminary + row + frame output bytes total **68,166,376→13,142,592**.
Metadata/frame event counts increase; bound setup is included separately. Render-internal
serialization, fixture construction/cloning, RPC/mock work, transport logging/recovery
and allocator behavior are excluded. Harness uses SDK response scope/original ID and
negotiated fields, without custom-runtime identity; separate route tests cover actual
runtime identity and full-frame equality. This is serialization-work evidence, with no
allocation, RSS or end-to-end/universal speed claim.

### Artifact verification (O03)

Fresh unchanged-source command, exit 0, one explicit measurement pass:

```sh
cargo test --locked --offline artifact_verification_work_measurement -- --ignored --nocapture --test-threads=1
```

`final-logs/o03-work.log` confirms the optimized work below. Before/after real filesystem
counts and clean timing logs are in maintenance Task 3. Each n fixture seeds create-new
0600 owned files in O(n) inside one real admitted image job and state mutex, and closes
explicitly after each case. Actual verify_contents includes the ownership lock; the
index is constructed afresh inside each verification. No copied algorithm or cached
filesystem observation is used.

| Tracked files n | Before entry comparisons | Final map insertions / queries | Directory items, both | Fresh validations, both |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 1 | 2 / 2 | 2 | 3 |
| 100 | 5,050 | 101 / 101 | 101 | 102 |
| 1000 | 500,500 | 1001 / 1001 | 1001 | 1002 |

Before lock comparisons were 2/101/1001; final separate lock/entry comparisons are zero.
Map counts are logical insert/get events, not internal hash probes; the complexity claim
is expected/amortized linear lookup plus the same linear filesystem validation.

Historical clean debug timings from Task 3 (one warmup/seven measured calls, median;
min..max in parentheses):

| n | Before | After |
| ---: | --- | --- |
| 1 | 38.208 µs (34.458..41.833) | 42.541 µs (36.958..51.625) |
| 100 | 1.172125 ms (1.117166..1.212) | 695.542 µs (653.166..855.291) |
| 1000 | 77.124917 ms (76.431708..77.316167) | 7.505667 ms (6.871..9.328166) |

Timing covers verification under an already-held mutex/admitted worker, including map
construction, excluding seeding/admission/mutex acquisition/printing/cleanup. Warm caches,
random hashing and OS iteration vary. Small n was slower; no timing pass threshold,
release/end-to-end guarantee, worst-case collision or RSS claim is made. Clean optimized
timing temporarily stripped only five cfg(test) observation calls, then restored them
byte-for-byte before Task 3 final gates; the temporary build emitted an expected unused
observer warning. Final source retains test hooks and passes strict Clippy. This Task 4
**did not strip source or rerun that clean timing**; it freshly confirms work counts and
explicitly cites those historical actual timing logs (`task-3-before-timing.log`,
`task-3-after-timing.log`) rather than misrepresenting timing reproducibility.

## Rulings, compatibility costs and remaining review

All current `Ruling:` entries in the five controller progress ledgers are retained here:

1. **Subscription context:** subscriptions/listen alone retains private SDK lifecycle
   `RequestContext.id`, because the pinned SDK's private sink/ack constructor derives its
   correlation there. Original ID is in public nonserialized
   `rmcp::service::OriginatingRequestId` and OPERATION.response_id; tools/all other contexts
   retain external context.id, and adapter wire metadata/replay is restored. Cost: custom
   generic Rust subscription handlers inspecting context.id see the internal ID and must
   read the public extension for the original. Product subscriptions are not advertised.
   This is documented in [runtime guidance](../stdio-runtime.md) and regression-tested.
2. **Paused fixture coordination:** deterministic manual clock coordination was included
   after the existing deadline regression repeatedly obstructed integration. Forced real
   two-permit contention demonstrated its mechanism; successful synchronization preserves
   timing/refund/partial assertions and production limits. Cost if wrong: extra test-only
   coordination, with the deferred watchdog failure path below; no production deadline
   or byte/image-budget change is authorized.
3. **Terminal-frame admission:** one observation slot under the write gate and a short
   synchronous poll_write/admission barrier retires only a successfully accepted final
   framing newline before duplicate/cap checks. It preserves SDK encoding, partial-frame
   refusal and nonwaiting admission without tombstones. Cost if wrong: premature admission
   or stale correlation; withheld-flush, max-in-flight=1, partial-frame, metrics/replay and
   EOF tests pin this boundary. AsyncWrite acceptance can precede physical flush/delivery;
   at most one committed writer remains owned with settlement/metrics/drain accounting.
4. **Membership:** every minimal username Channel/Community observation remains
   unconfirmed/false, including creator. Official minimal-field rules do not authorize
   creator/left membership inference. Full !min&&!left is hash-independent; Dialog/my_results
   stays positive. Cost: a joined peer seen only minimally reports false/unknown until
   authoritative evidence, without extra RPC/cache inference. The original Task 4
   minimal-creator-positive ruling is **superseded**, not current policy.
5. **Bare config values:** whole placeholders at string/unknown value destinations retain
   exact string data; only declared numeric/bool paths coerce a valid single scalar.
   Comments/keys/tables/general invalid syntax remain unchanged. Cost: safe broadened
   acceptance of formerly invalid bare strings, and environment values relying on injected
   TOML quotes now retain those quotes as data; this intentional correction is documented.
6. **Required-row bound:** after an actual oversized rendered candidate, canonical raw row
   JSON plus its actual escaped fallback contribution (legacy escaped once) is a strict
   lower bound. Separators/metadata/identity/IDs are omitted. Only prefixes whose row bound
   alone exceeds budget skip candidate rendering; every pop still checks real metadata and
   full-frame accounting remains authoritative. Missing arrays/conversion errors use exact
   loop fallback. Cost if proof wrong: discarding a fitting prefix. Bound-vs-actual and
   exact retained-ID/threshold tests cover formats/protocols/escaping/compact/omissions;
   setup/event increases are measured, without whole-size monotonicity assumptions.

**Deferred minor M1, confirmed nonblocking by whole-branch review:** ManualClock's watchdog panics inside
its blocking task, but that result is awaited only at finish. A missing processing_complete
notification can leave the main notification wait hanging even after watchdog failure.
This is a test-helper failure-path limitation in `src/test_helpers.rs` and the paused media
fixture in `src/mcp/tests/protocol_budgets.rs`, inherited from the scoped runtime review;
no failing production path is asserted. The final full suite passes. It was deliberately
not expanded into an unauthorized source fix here, and the watchdog is not claimed to
fully bound missing-notification coordination.

Author integration self-review reconciled all 19 rows, committed regressions and owning
reports/rulings, complete lock scope, feature output, default/ignored counts, all gate logs,
work aggregates and unchanged documentation links. `git diff --check` and final scoped
prose diff inspection pass; historical validation reports are unchanged. All 15 completed
implementation tasks' plan steps and integration Task 4 are complete. No unavailable local
gate remains. The fresh reviewer read the full 105-file package, inspected all 15 gate logs,
independently recomputed the 144 O02 comparisons/counters, checked O03 work/timing evidence,
and approved the six compatibility decisions. The full local report remains at
`.superpowers/sdd/2026-11-15-remediation-maintenance-performance/final-review-report.md`.
No source changed after the validated `13fda27`; subsequent commits only record evidence
and approval. M1 can delay diagnosis/consume a CI timeout if a future regression suppresses
the notification; it is not an identified production defect or a failed current gate.

The reviewer explicitly set aside live account/real-terminal outcomes, Linux/Windows and
hosted execution, and allocator/RSS/end-to-end performance guarantees. The controller
confirmed those limits against the authorized offline macOS scope: operational/platform
acceptance remains unclaimed, and performance conclusions stay limited to the supplied
serialization/map work and qualified debug timing. No extra account, provisioning,
publication or unsupported performance claim is needed for this acceptance.
