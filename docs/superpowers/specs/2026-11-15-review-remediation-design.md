# Project review remediation specification

Date: 2026-11-15. Baseline: `aeaceab43e3403d1d9e18486c5f229a18ee82d74` (0.26.0).
Status: specification for implementation planning; no production changes included.

Repair the correctness and maintenance defects identified in the comprehensive project
review, then simplify the identified hot paths without changing their contracts. Existing
tests pass at the baseline, so acceptance requires new regression evidence, not merely
another green run of the old suite.

The user explicitly chose to **preserve global search's current scope, including private
conversations, and correct its documentation**. Other design choices below are proposed
engineering defaults for this repair effort, not additional user-selected features.

## Scope and approach

Include all 13 numbered review findings and the five optimization/cleanliness items.
The traceability table and acceptance cases below are the completion checklist. Source-based
findings must first gain a failing behavior test; if a premise is disproved, record the
counterexample and disposition instead of implementing an unnecessary change.

Use focused changes grouped by subsystem, with correctness work preceding
performance refactoring. Three approaches were considered:

| Approach | Tradeoff | Decision |
| --- | --- | --- |
| Focused repairs with regression tests and shared identity rules | Small compatibility surface; independently reviewable changes | Recommended design used here |
| Rewrite transport, conversion and configuration layers together | Could unify abstractions, but greatly expands failure modes and review scope | Excluded |
| Fix only the cancellation race and dependency audit | Fastest first delivery, but leaves known result and input defects | Useful first stage, insufficient completion |

This is a behavior specification, not a command-by-command implementation plan. Private
helper names, file splits within the existing layout, and test fixture mechanics are left
to the implementer. New public API shapes or changed behavior beyond the explicit decisions
below require revisiting this specification.

## Constraints and compatibility

- Retain the single `#[tool_router]` implementation, thin entry points, generic client and
  limiter seams, file-as-module layout, and the Rust manual's scoped exceptions.
- Keep `json_v1`, matching text fallback, explicit legacy reads, the 16/18 tool catalog,
  and both MCP 2026-07-28 and legacy 2025-11-25 behavior. No output schema version bump.
- Preserve string channel/user IDs, integer message IDs, nullable fields, namespaced
  source identity, compact-mode ambiguity refusal, and direct Rust compatibility methods.
- Validate locally before acquisition; perform every RPC after acquisition. Do not refund
  failed fan-out attempts. Preserve existing media refunds, including per-item accounting.
- Preserve total operation deadlines, search partial results, cancellation, bounded input,
  admission, memory, image processing and shutdown. No new application retries.
- Preserve global search's widened server date bounds and exact client-side filtering.
  Global and fan-out results still have no message-ID continuation cursor.
- Preserve sending opt-in, source-root restrictions, staging leases, secret redaction,
  nonretryable unknown final delivery and recovery's non-idempotency semantics.
- Keep all three direct grammers dependencies at the same existing Codeberg revision.
  Only the scoped rmcp update and unavoidable coupled lockfile changes are authorized by
  this design; do not regenerate the dependency graph or change the pinned nightly.
- Add no dependency unless existing libraries demonstrably cannot meet a requirement.
  Do not add production codec decoding, FFmpeg, a transport framework, or a configuration
  framework. Existing test-only codecs may validate generated fixtures.
- Validate locally on macOS. Existing Linux CI remains; provisioning Linux and Windows
  validation are outside this work. No live Telegram sends/authentication are required.
- Do not change private configuration, sessions, global agent configuration, package
  version, release tags, remote branches, or GitHub Releases as part of these repairs.

## Traceability

| ID | Review item | Main production files | Acceptance |
| --- | --- | --- | --- |
| R01 | Cancelled response cancels reused request ID | `src/mcp/runtime.rs`, `runtime/state.rs`, `runtime/transport.rs`, observability correlation | A01 |
| R02 | Premature global pager exhaustion | `src/telegram/client/raw_page.rs`, `raw_pager.rs` | A02 |
| R03 | Albums merge peer namespaces | `src/telegram/albums.rs` | A03 |
| R04 | Channel sender converted to UserId | `src/telegram/converters/message.rs` | A04 |
| R05 | Equivalent channel aliases duplicate results | `src/mcp/tools/fanout.rs`, search/history handlers | A05 |
| R06 | Username lookup invents subscription | `src/telegram/converters/channel.rs`, `client/channels.rs`, `client/ops_resolve.rs` | A06 |
| R07 | URL filter ignores Telegram entities | `src/telegram/converters/media.rs` | A07 |
| R08 | Read failures become invalid send targets | `src/telegram/client/resolve_recipient.rs`, `send_targets.rs`, `send_transport.rs` | A08 |
| R09 | Large JPEG metadata rejected | `src/sending/probe.rs` | A09 |
| R10 | Authentication trims password | `src/telegram/auth.rs` | A10 |
| R11 | Environment substitution changes TOML values/comments | `src/config/env.rs`, `loading.rs` | A11 |
| R12 | rmcp advisory fails security policy | `Cargo.lock`, `Cargo.toml` if needed, current dependency documentation | A12 |
| R13 | Artifact process matrix absent from CI | `.github/workflows/ci.yml` | A13 |
| O01 | Qualified send lookup keeps scanning | `src/telegram/client/resolve_recipient.rs` | A14 |
| O02 | Duplicate response fitting and deep clones | `src/mcp/tools/shaping.rs`, `wire_budget.rs`, batch handler | A15 |
| O03 | Quadratic artifact path lookup | `src/mcp/artifacts.rs` | A16 |
| O04 | Nonfinite refill/zero bucket accepted | `src/config/validation.rs` | A17 |
| O05 | Stale environment-test guidance | `docs/memory.md` | A18 |
| D01 | Global search scope documentation | `src/mcp/server.rs`, request/API documentation, README | A19 |

## Required behavior

### R01 Request lifetime identity

Treat each admitted request as a distinct operation, independently of its externally
supplied request ID. Capture this identity before dispatch and retain it through handler
execution, queued results, errors, progress and subscription notifications, write completion,
metrics correlation and recovery association.

Cancellation removes the old operation from active admission and permits reuse of its
external ID. A frame already committed to output still finishes unless connection shutdown
interrupts it. Completing that frame must affect only its original operation. Queued output
that has not begun writing is suppressed after cancellation, even if a replacement uses
the same request ID or progress token. A pointer/generation check only in `finish` is
necessary but insufficient if queued output can still bind to a replacement on first poll.

Use an immutable operation handle/generation for internal correlation. The implementation
plan must explain how it survives the SDK queue, including error and progress paths; a
late lookup by external ID alone is not acceptable. Internal identifiers must not leak
into client-visible IDs, progress tokens, metadata, replay payloads or byte-budget accounting.
If SDK-facing private correlation IDs are used, map both directions at the adapter boundary
and preserve the original ID in application request context and size measurement.

Keep correlation storage bounded by owned active/queued work and release it on completion,
cancellation settlement and shutdown. Do not retain an unbounded set of every ID ever seen.
Actual writes may increment write counters, but an old write must not consume the new
request's metrics entry or overwrite its recovery record. Independent recovery keys retain
their existing reservation, expiry and duplicate-key behavior.

### R02 Global pagination

Separate page-envelope decoding from scope-specific exhaustion decisions. Single-peer
history/search can retain their tested policy. For global search, a low message ID, a short
nonempty slice, or a missing `next_rate` alone must not establish exhaustion. Preserve
`offset_peer`, `offset_id`, and `offset_rate` advancement from the actual last raw message.
An empty page or terminal unsliced `Messages` response terminates normally. Continue
nonterminal slices within the existing total deadline; do not introduce retries.

Preserve the meaning of `has_more`: a qualifying post was refused by a limit/budget, not
merely that a server supplied continuation information. Metadata counters count actual
pages/raw messages. An exact-limit first page requires walking far enough to distinguish
exhaustion from an additional qualifying result, subject to the search deadline.

### R03–R05 Namespaced identities and duplicate results

Album identity is `(peer_namespace, channel_id, grouped_id)`. Use it consistently in
admission counting, grouping maps and ordered slots. Message identity is
`(peer_namespace, channel_id, message_id)`; bare IDs must never serve as cross-source keys.

Populate `sender_id: Option<UserId>` only when `from_id` is `Peer::User`. For a channel/chat
sender, retain its resolved sender name independently and emit null for `sender_id`. Do
not introduce a new sender schema in this repair. Absent sender and missing-envelope
behavior remains unchanged; a missing name must not erase a known user ID.

Before fan-out acquisition, normalize locally equivalent channel reference forms: trim,
remove one optional leading `@` for valid usernames, compare valid ASCII usernames
case-insensitively, and canonicalize accepted numeric spellings without conflating
different identifier kinds. Keep the first supplied spelling for error attribution.
Continue to reject invalid inputs locally. Numeric ID versus username equivalence requires
resolution and is not assumed before charging.

Deduplicate successful fan-out messages by full message identity before final sorting and
truncation. Resolve duplicate observations deterministically in input-channel order, keeping
the first complete message/album rather than merging conflicting payload fields. Preserve
per-attempt errors, real RPC/page counters, partial flags and charges. Duplicate rows alone
must not cause `has_more=true`; an upstream limited page still can. Do not use deduplication
to suppress compact-header conflicts among distinct surviving observations.

### R06 Subscription evidence

Replace the implicit assumption that every resolved peer came from a subscribed dialog
with explicit conversion context. Dialog membership is positive evidence. For username
resolution, use the pinned TL peer's membership flags (for example `left`, `deactivated`,
or forbidden variants) when authoritative. A missing/forbidden membership observation
must not be reported as a confirmed subscription.

Keep the existing boolean schema: true means membership is positively established from
the current evidence; false includes membership that cannot be established. Document this
conservative interpretation. A joined username-resolved peer must still return true when
its in-hand flags establish membership. Public discoverability alone proves neither state.
Preserve the public-search `my_results` membership handling and best-effort full-info
enrichment. Do not add a full dialog traversal just to fill this field.

Apply this rule to basic/full channel information and unmatched-username batch resolution.
Before implementation, enumerate the pinned TL variants and their usable flags in the
test fixture table; unsupported variants take the conservative false result.

### R07 URL filtering

For history's local URL filter, a Telegram `MessageEntityUrl` or `MessageEntityTextUrl`
establishes a match even without an HTTP scheme in displayed text. A nonempty URL in
available webpage metadata also establishes a match. Retain the current literal
`http://`/`https://` detection as a compatibility fallback. Do not introduce a heuristic
domain-name parser or fetch link targets. Non-URL entities and non-web media do not by
themselves match. Leave unrelated media filters and server-side search filters unchanged.

### R08 Read failures before sending

Use a read/preparation error mapping separate from final mutation classification. I/O,
driver/DC, decoding and unknown provider failures before dispatch produce a safe
`unavailable` error with `delivery_state=not_sent`. Preserve recognized authentication,
rate-limit and timeout categories. Emit `recipient_not_found` or `invalid_reply_target`
only for known definitive refusals or a successful response establishing absence.

Do not change retryability globally: preserve the canonical error policy, where unavailable
is not automatically retryable. Once final dispatch may have happened, retain the existing
`delivery_unknown` behavior and prohibition on automatic resend. Raw SDK/provider text
must not survive in errors, logs or error chains.

### R09 Bounded metadata-aware media inspection

Keep signature/container inspection without full codec validation. Read JPEG marker
headers incrementally from the staged verified file; skip APP/COM payloads using checked
seeks bounded by the snapshot's length. A valid SOF after 64 KiB of metadata is acceptable.
Reject truncated/overflowing segment declarations, missing dimensions, invalid marker
structure, and offsets outside the snapshot. Restore the reader position on success and
failure. File opening, staging identity and quota policies remain unchanged.

Retain a 65,536-byte cumulative inspection-read budget, with a separate maximum of 4,096
JPEG markers per probe. Seeking past metadata does not consume that byte budget; all
offsets remain bounded by the configured file cap and the existing total send deadline.
Exceeding these probe bounds returns the existing `unsupported_media/not_sent` response.
These are proposed implementation limits, not a promise to accept every valid JPEG.

Apply the same principle to supported MP3 ID3 headers: validate version-specific header
length/footer rules and synchsafe sizes, skip the declared tag within file bounds, and
inspect the MPEG header beyond it. Keep PNG/AAC/MP4/Opus recognition and accepted kinds
unchanged. Update the sending guide's fixed-prefix description. No generic-file fallback.

### R10 Password preservation

Pass the exact password returned by the terminal password reader to upstream authentication.
Do not trim or normalize spaces, tabs, Unicode or other password characters. Terminal
submission handling remains the input library's responsibility. Login-code handling,
credential storage and secret logging policies are unchanged.

### R11 Configuration interpolation

Define interpolation on TOML values, not arbitrary source-text replacement:

- Expand `${NAME}` in value strings, including embedded and multiline strings. Preserve
  replacement bytes as the resulting string value; quotes, backslashes, newlines, Unicode,
  `#`, dollar signs and TOML-looking text must not alter document structure.
- Preserve expansion inside literal-string values too. A quote in a replacement is data,
  not a request to terminate the surrounding TOML literal.
- Ignore whole-line and inline comments outside strings. `#` inside a string is data.
  Do not expand table/key names or perform recursive expansion of substituted values.
- Preserve documented numeric configuration forms, including `api_id = "${API_ID}"`
  and an unquoted whole-value placeholder for a numeric field. Coerce according to the
  destination scalar type; numeric-looking credentials, paths and other string fields
  remain strings. A typed whole-value replacement must be a single valid scalar, not
  an injected array, table, assignment or trailing document fragment.
- Keep missing/non-Unicode variable errors safe, with variable name and original line
  context. Discard parser excerpts, expanded values, keys and unsafe source chains.
  Incomplete `${...` syntax inside a valid string stays literal, as today.
- Do not expand tilde, change config path precedence, reject previously accepted unknown
  configuration fields, or mutate the process environment.

Use the existing TOML library plus a narrowly scoped syntax-aware interpolation layer;
the implementation plan may choose the representation, but must cover basic/literal and
single/multiline forms. A new line-based search-and-replace algorithm is insufficient.
Document the intentional corrections to accidental behavior: substitutions in comments,
key names, or strings that previously depended on TOML reinterpreting their raw contents.

### R12 Dependency security

Update the locked rmcp version to a reviewed compatible release at least 3.9.0 that fixes
[RUSTSEC-2026-0462](https://rustsec.org/advisories/RUSTSEC-2026-0462). Recheck current
advisories at implementation time; the version floor alone is not security acceptance.
Update coupled rmcp macros/lock entries only as required by that selection. Retain the
current server/stdio feature scope; do not enable the affected HTTP client transports.

The baseline audit fails even though the affected HTTP client features are disabled.
Do not add an advisory ignore as the repair. Test both negotiated protocol versions,
advertised schemas, recovery, progress, errors and exact wire budgets after the update.
Refresh current-state SDK version references; retain historical validation reports as
historical evidence. No project package-version bump is included.

### R13 CI coverage

Add `scripts/test_artifact_clients.py target/debug/examples/offline_mcp_fixture` to the
existing offline process/client matrix on both configured runners. Reuse the built fixture
and existing job timeout, read-only permissions and pinned actions. The script must use
disposable explicit roots and must not read an account session or create global artifacts.

### O01 Qualified sending lookup

For an uncached fully qualified recipient, stop dialog traversal when the first exact
`(namespace, id)` peer is found and continue ordinary metadata/recipient validation.
Absent matches still traverse under the original deadline. No extra page is fetched after
the match; a later page failure cannot invalidate it. Do not apply this optimization to
unqualified numeric read resolution, which must scan for namespace ambiguity.

### O02 Response shaping

Preserve complete serialized JSON-RPC accounting as the authoritative wire budget, including
external request ID, negotiated fields, duplicated text fallback, cursors, omitted IDs and
the metadata-qualified one-message floor. Avoid deep-cloning message bodies just to clear
them for metadata measurement, and avoid redundant preliminary fitting on wire paths.

Direct Rust/legacy adapters retain their documented budget behavior. Keep the additional
serialization in `InstrumentedTransport`: it serves byte logging and recovery. A counting
serializer or borrowed projection may replace allocations, but it must agree with actual
serialization. Do not replace exact accounting with estimated character counts or assume
size is monotonic when omission metadata grows.

### O03 Artifact verification

Replace the per-directory-entry linear search through stored artifacts with a path-indexed
lookup built once per verification or maintained consistently with the store. A temporary
lookup under the existing lock is the preferred minimal change. Lookup work becomes O(n)
for n tracked paths; full filesystem identity/ownership/foreign-entry checks remain.

Preserve fail-closed tamper handling, logical expiry, eviction, response pins, deletion
ownership, quotas, CPU-worker bounds and mutex atomicity. Do not move validation outside
the lock or trust a cached filesystem observation solely to improve speed.

### O04 Configuration bounds

Require `max_tokens >= 1` and a finite positive `refill_rate`. Keep zero read media/download
or transcription costs accepted for compatibility; retain the existing enabled-send
positive-cost rules and cost-versus-capacity validation. Apply the rule through the config
loader and documented validation API; do not silently clamp or change `RateLimiter::new`
compatibility semantics for callers that bypass validation.

### O05 and D01 Documentation

Remove obsolete `EnvGuard`/`ENV_LOCK` guidance and describe subprocess-only environment
tests consistently. Update current documentation for the repaired behaviors and dependency
version. Record durable decisions in `docs/memory.md`, not execution transcripts.

Describe unscoped global search as searching Telegram's available global index for the
authenticated account, which may include private conversations, groups and channels. It
does not guarantee an exhaustive search of every accessible or public peer. Preserve its
current RPC flags and peer eligibility; add no new scope switch. Remove claims that every
message-conversion path excludes private users. Preserve existing single-channel subscribed
dialog resolution; expanding it to public nonmembers is outside this repair.

## Acceptance cases

Each behavior-changing requirement needs a failing test on the baseline, followed by a
passing test on its fix. Prefer existing pure seams, production converters and mock RPCs;
do not test a copied algorithm while leaving the production decision unexercised. Use
deterministic synchronization and paused Tokio time for races/deadlines.

| Case | Required evidence | Primary test locations |
| --- | --- | --- |
| A01 | Block an old response after its first byte; cancel; reuse ID; drain old bytes; replacement completes and remains cancellable. Repeat with output queued before first poll, reused progress token, errors, string/numeric IDs, both protocols, and separate recovery keys. No stale progress or metrics/replay cross-association; storage returns to baseline after completion/shutdown. | `src/mcp/runtime/tests/transport.rs`, `src/mcp/tests/protocol_failures.rs`, `protocol_progress.rs`, `protocol_recovery.rs` |
| A02 | A real TL slice with 100 mixed-peer messages, first ID 9, later IDs above 1900 and a continuation fetches a second page. Low IDs, short slices and absent next_rate do not stop prematurely. Empty/terminal pages stop, filtering still finds later matches, exact-limit/extra-match has_more and deadline partials remain correct. | `src/telegram/client/tests/raw_page_tests.rs`, `raw_pager_tests.rs`, `ops_message_tests.rs`, `walk_tests.rs` |
| A03 | Equal bare source/grouped IDs in user/chat/channel namespaces remain separate posts; same-source album siblings still collapse and can extend the admitted raw count beyond the post limit. | `src/telegram/albums.rs` tests, MCP search shaping tests |
| A04 | Real TL User/Channel/Chat/absent sender fixtures produce the correct nullable user ID and independent name in canonical/legacy output; missing envelope names preserve known user IDs. | `src/telegram/tests/message_tests.rs`, MCP parity/contract tests |
| A05 | Optional @, ASCII case and whitespace variants incur one local-normalized attempt; numeric-plus-username duplicates incur actual attempted charges but return unique messages. Equal message IDs across different namespaces/sources remain distinct; duplicate-only overflow does not invent has_more. | `src/mcp/tools/fanout.rs` tests, `src/mcp/tests/multi_channel.rs` |
| A06 | Joined and left username-resolved TL peers, dialog peers, forbidden/unknown variants and public my_results use the evidence policy across basic/full/batch paths. Confirm no new membership RPC and unchanged enrichment degradation. | `src/telegram/client/tests/channels_tests.rs`, converter tests, `src/mcp/tests/channels.rs`, `resolve.rs` |
| A07 | TextUrl with display text only, Url entity on example.com, webpage URL and literal HTTP(S) match; unrelated entities, plain prose and non-web media without links do not. | `src/telegram/tests/converters_thumb_forward_tests.rs`, `src/telegram/client/tests/walk_tests.rs`, history filter tests |
| A08 | Inject I/O and unknown 500 failures in cached-recipient/reply/topic reads: unavailable/not_sent. Known missing target, auth, flood wait and deadlines retain their distinct classifications; identical post-dispatch uncertainty remains delivery_unknown. | `src/telegram/client/tests/recipient_tests.rs`, `send_target_tests.rs`, `send_transport_tests.rs` |
| A09 | A generated valid JPEG with SOF beyond 64 KiB succeeds through the inert send fixture; verify bytes actually decode in a test-only codec. A real valid MP3 fixture after a large ID3 tag is accepted. Truncated/oversized segments, malformed ID3 sizes, 4096-marker boundary, inspection budget, file cap and cursor restoration are tested. | `src/sending/tests/probe_tests.rs`, `reader_tests.rs`, `src/mcp/tests/sending_media.rs` and inert fixture |
| A10 | Capture authentication input below the terminal seam: leading/trailing ASCII and Unicode whitespace remains byte-for-byte intact; no credential enters captured diagnostics. No live auth needed. | `src/telegram/auth.rs` test seam or sibling auth tests |
| A11 | Real Config loader round trips quote/backslash/newline/Unicode/hash values and numeric/string destinations. Whole/inline comments never look up variables; multiline basic/literal strings and key nonexpansion work. Missing/non-Unicode errors and malformed TOML remain secret-free across Display/Debug/source chains. Injection-like replacement cannot create tables or alter adjacent fields. | `src/config/tests/env_tests.rs`, `load_tests.rs`, `loading_tests.rs` |
| A12 | Fresh cargo audit and source/advisory/declaration policy pass without new ignores; review targeted lock diff and feature graph. Entire protocol matrix, client scripts, Rust gate and release build pass on the selected patched SDK. | Existing schema/protocol suites, dependency-policy fixtures and security commands |
| A13 | The artifact matrix passes locally and is configured for both CI matrix runners; workflow syntax validation passes. Record hosted runs only if actually performed. | `scripts/test_artifact_clients.py`, `.github/workflows/ci.yml` |
| A14 | Match on first dialog page followed by a would-fail/would-timeout next page succeeds without polling that page. Missing recipient still consumes the bounded traversal; namespace mismatch never selects. | `src/telegram/client/tests/recipient_tests.rs` |
| A15 | Exact actual wire-size agreement across output/protocol modes, long IDs, escaping/Unicode, compact/full, omitted IDs, one-message floor and metadata-only overflow. Direct adapter contract parity retained. Record before/after representative allocation or serialization-work measurements. | `src/mcp/tests/protocol_budgets.rs`, `src/mcp/tools/wire_budget.rs` and shaping tests |
| A16 | Foreign/tampered/missing files, expiry, quota eviction, pins and failed cleanup retain behavior. Measure verification at 1/100/1000 entries; establish linear lookup work and report timing without a flaky wall-clock pass threshold. | `src/mcp/artifacts/tests.rs`, `src/mcp/tests/media_artifacts.rs` |
| A17 | Loader rejects zero capacity and NaN/positive infinity/negative infinity/nonpositive refill; finite positive refill and supported zero read costs pass. Enabled sends still reject zero costs. | `src/config/tests/validation_tests.rs`, `load_tests.rs`, `sending_tests.rs` |
| A18 | Current docs contain no instructions to mutate parent process environment or use removed EnvGuard/ENV_LOCK; check links and guidance against current fixtures. | Documentation inspection, no source-text unit test |
| A19 | Global fixtures retain user/chat/channel results, explicit namespaces and no global cursor; descriptions make private conversation scope clear without claiming exhaustive public search. | `src/telegram/client/tests/ops_message_tests.rs`, MCP search contract tests, documentation review |

## Delivery sequence and review boundaries

1. Repair R01 and add its full lifetime/correlation regressions. Treat it as the first
   independently reviewable change because it can silently cancel another operation.
2. Update rmcp under R12 and rerun those regressions and the complete protocol matrix.
   Do this before later wire/serialization optimization, so they target the final SDK.
3. Repair read correctness together: R02–R07 and D01. Share identity rules, but keep
   pager, conversion and fan-out commits independently testable.
4. Repair sending/input behavior: R08–R11; include O01 with qualified recipient tests.
   Keep config interpolation separate from authentication and file probing.
5. Apply O04, R13 and O05. Update durable docs alongside the behavior they describe.
6. Optimize O02 and O03 in separate changes after correctness tests are green. Supply
   measurements, preserve exact behavior, and avoid broad style-only churn.
7. Run final verification, inspect the complete diff and obtain independent review.

These are implementation groups, not authorization to begin implementation in the
specification-writing task. Detailed plans should be split by these subsystem boundaries;
all consume this shared specification and its compatibility constraints.

## Validation and completion

Baseline evidence from the review: formatting, strict all-target Clippy, 1,078 passing
library tests (9 ignored entrypoints/measurements), 3 doctests, warning-free rustdoc,
release build, all four Python clients, 25 policy fixtures and metadata policy for 340
external packages passed. Fresh audit failed on the rmcp advisory. Baseline evidence is
context, not proof that a repair passes. cargo-deny and actionlint were unavailable in
that local review and must be supplied for their final checks, or reported as explicit
remaining validation gaps without declaring those gates passed.

Final local commands on the pinned macOS toolchain:

```sh
cargo fmt --all --check
cargo clippy --offline --locked --all-targets -- -D warnings
cargo test --offline --locked
RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps
cargo build --offline --locked --release
cargo build --offline --locked --example offline_mcp_fixture
python3 -B scripts/test_stdio_process.py target/debug/examples/offline_mcp_fixture
python3 -B scripts/test_mcp_probe.py target/debug/examples/offline_mcp_fixture
python3 -B scripts/test_media_clients.py target/debug/examples/offline_mcp_fixture
python3 -B scripts/test_artifact_clients.py target/debug/examples/offline_mcp_fixture
python3 -B -m unittest discover -s scripts -p test_dependency_policy.py
python3 -B scripts/check_dependency_policy.py --offline
cargo audit --deny unsound --deny yanked
cargo deny --locked check sources advisories
actionlint -shellcheck= -pyflakes=
```

Fetch locked dependencies before offline checks if the scoped SDK update is not cached.
Fresh advisory checks require network access. Use the security-tool versions recorded in
`docs/maintenance.md`; no global installation is required if temporary tools suffice.
Capture the actual toolchain, SDK version, feature graph, test totals, audit date and
optimization measurements. Do not claim Linux, Windows, live-account or hosted-CI
acceptance from local tests or workflow configuration.

Completion requires a disposition and evidence for every R/O/D item, all behavior
regressions passing, compatibility checks passing, current documentation updated, and
independent review resolved. A disproved finding may close with a test and explanation;
an unavailable required check remains an explicit gap. Update the changelog's Unreleased
section during implementation. Delete this active spec and its corresponding plans when
the repair change merges; Git history is the archive.
