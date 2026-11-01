# Rust manual and S1–S6 adoption acceptance — 2026-11-01

## Scope and conclusion

This completes the scoped Rust-policy assessment requested by original roadmap Task 10. Source review
uses `4ce5c80` on `chore/final-modernization-acceptance`, including integrated Tasks 1–8
and the later production-media coverage/classification change, followed by the integrator's
four narrow documentation/annotation corrections and related crate-header clarification
in the working tree. It does not close Task 9,
the complete roadmap, release publication, or platform/live-account acceptance.

The adopted policy is `docs/rust-style.md`, including its scoped exceptions, rather than
literal conformance to every external Rust style guideline. The reviewed implementation
preserves S1–S6's intended architecture and safety changes. The archived plan's temporary
invalid-domain-Deserialize exception has been retired in code: all five validating wrappers
now deserialize through their constructors (invariant fix `ef91e86`). No Critical or Important
adoption defect was
found. Four narrow Minor documentation/ergonomics discrepancies were reported to the
integrator and source-verified as corrected; their disposition and fresh checks are recorded
below.

Reviewed inputs: current `AGENTS.md`, `docs/memory.md` Current state/Open items and relevant
bounds/security decisions, the full authoritative manual, `docs/rust-api.md`, development
conventions, agent setup, the active original roadmap and existing Task 8/media reports.
The archived alignment plan was located with `git show --stat 6b4a9fb` and read with:

```sh
git show 6b4a9fb^:docs/superpowers/plans/2026-10-31-rust-standards-alignment.md
```

That archived plan explicitly separated S6's reviewed implementation from the then-pending
serde invariant fix and parent whole-branch acceptance. Its historical green checks are
historical implementation evidence, not newly executed acceptance checks.

## S1–S6 assessment

| Stage | Required outcome and current evidence | Assessment |
| --- | --- | --- |
| S1 — authoritative policy/format | `AGENTS.md` and `docs/conventions.md` reference the manual. `rustfmt.toml` retains edition/style edition 2024, width 100, module import grouping, lowercase hex and macro-matcher formatting. Toolchain selection remains explicit and separate from language features. Archived policy/format commit is `e338fe7`; later behavior changes have separate history. | Adopted. Current formatter check and stricter import-policy checks are separate evidence below. |
| S2 — failure/safety first | `rate_limiter.rs:104–108,174–192` maps poison to `StateUnavailable` and refuses acquisition/refund recovery. Buffer/metrics use fallible snapshots; status retains unavailable reasons. `runtime/process.rs:29–43` propagates signal registration/listener errors. Config expansion has injected lookups; real loader and global subscriber tests use subprocesses. | Adopted with the documented conservative legacy wrappers. Production never reads poisoned interiors to resume work. |
| S3 — deliberate public API/docs | Facades retain public binary/embedding paths; `docs/rust-api.md` distinguishes supported, transitional and internal surfaces and separately records Rust/wire migrations. Crate examples use inert values and generic composition; `examples/inert_server.rs` supplies an example-local client. `missing_docs` remains on completed leaves. | Adopted; no silent visibility withdrawal or blanket `non_exhaustive` restriction. Minor stale crate/API wording corrected as recorded below. |
| S4 — explicit dependencies | Config qualifies `defaults::...`; production client operation modules use explicit imports. The remaining `pub use types::*` is the documented facade export, and test globs stay test-local. Literal inspection did not find a new production import prelude. | Adopted. No blanket wildcard-lint suppression was added. |
| S5 — focused modules | Config representation/loading/validation live in private siblings with stable facade reexports. Client retains private state and a thin complete trait dispatcher; operations, cursor/result helpers, resolution and storage remain focused siblings. MCP router methods stay visible in one attribute-macro impl, delegating operation bodies. No `mod.rs` exists under `src`/`examples`. | Adopted with the two architectural exceptions; no field widening is needed for the new media snapshot/collector. |
| S6 — useful enforcement | Selected Cargo lints remain exactly wildcard imports, missing error docs and consuming-Self return annotations. `AlbumMode` and `CursorPolicy` retain named internal choices without changing public JSON booleans. Established fallible constructors keep compatible names; new `Runtime::try_new`/`MediaDeadline::try_new` validate explicitly. The six original builders and the seventh output-mode builder now carry useful `must_use` messages. | Adopted after the narrow Minor builder correction below. Advisory lints remain advisory; the serde exception is resolved rather than grandfathered. |

## Manual sections 1–9: current implementation evidence

### 1. Formatting and imports

The checked-in formatter settings match the adopted manual exactly. Source navigation
found explicit production imports in config, client operations, runtime, storage and new
media helpers. Generated TL families remain qualified where their origin matters.
`VisualMediaMessage` and `collect_download` use ordinary private Rust types/functions;
no new parent prelude or handler mini-language was introduced.

Formatting compliance is the formatter's result. Explicit imports and selected Clippy
warnings are stricter adopted-policy evidence. Neither establishes conformance to every
optional pedantic/restriction or library-advisory suggestion.

### 2. File structure and visibility

`src/lib.rs` exports the intentional library surface and `src/main.rs` remains a separate
binary consumer. `config.rs` exposes selected stable types from private siblings.
`telegram/client.rs` owns private client state and delegates its trait operations;
`server.rs` keeps the single `#[tool_router]` impl visible to rmcp. Source outlines show
operation implementations in `client/ops_*.rs` and `server/impl_*.rs`.

Task 8's `runtime/{state,transport,processing,process}.rs`, `session_storage.rs`,
`media_pipeline.rs`, `wire_budget.rs` and observability recovery helpers each have a named
responsibility. The new `VisualMediaMessage` snapshot (`ops_media.rs:177–195`) extracts only
public upstream media/text/peer accessors; it neither exposes `PeerMap` nor widens upstream
or connector private fields. Its generic collector remains private. Existing public DTO
fields and documented compatibility paths are preserved, with Task 7/8 additions called
out in the Rust API inventory.

### 3. Names, constructors and conversions

`ids.rs:30–35,70–75,116–121` deserialize `ChannelId`, `MessageId` and `UserId` through their
positive-ID constructors. `names.rs:18–23,88–93` do the same for `Username` and `ChannelName`;
the latter trims whitespace and rejects blank values. Constructor/serde invariant tests
cover rejected values as well as unchanged valid scalar forms. This supersedes the archived
plan's temporary derived-deserialization exception; it is not an accepted remaining gap.

New finite media budgets use `MediaDeadline::try_new` with zero/overflow rejection; runtime
construction similarly uses `try_new`. Existing fallible `new` signatures remain documented
compatibility commitments. No fake valid ID/credential default, resource `Copy`, unused
constructor alias or gratuitous conversion trait was introduced. `MediaDeadline` is a cheap
copied value whose copies preserve the same absolute instant; media resource ownership
instead remains explicit.

### 4. Ownership and domain types

Flexible scalar coercion stays in MCP serde/validation helpers, above strict domain types.
Message IDs remain channel-scoped and are range-checked before RPC. Named `AlbumMode`,
`CursorPolicy` and compact scope remain separate choices rather than ambiguous positional
flags. Shared server/client/limiter state still uses `Arc` and substitutable generic traits.

The media pipeline uses an ordered stream rather than retaining the whole production MCP
batch. Raw bytes and the admission lease move together into `process_download`'s blocking
closure (`impl_media.rs:414–431`). `MediaDownload` equality intentionally ignores lease
identity. Existing collected/cloned Rust batches are explicitly caller-owned memory and
are not claimed covered by the production raw-retention guarantee. The clone in the bounded
image downscale helper preserves an unchanged decoded image for repeated cap-fitting
attempts; it is bounded by source limits and is not an unbounded batch retention path.

### 5. Errors, panics and secrets

Library failures remain typed `thiserror` variants; setup/configuration retain `anyhow`
context at their application boundary. Canonical `ToolError::from(Error)` matches variants
to fixed safe messages/codes rather than recovering meaning from display strings.
The legacy string API remains explicitly documented. An `Error` string payload is not
assumed automatically safe: authentication discards arbitrary provider payloads, and
config parsing discards TOML error excerpts/messages/keys and non-Unicode raw environment
values before adding safe position/context. Credential fields remain `SecretString`.

Production `unwrap()` was not found in the source-wide literal review after distinguishing
inline/extracted tests. Reviewed production `expect` sites have concrete invariants:
`link.rs:44` receives at least one element from `split`; request schemas describe known
object request structs; private CPU/raw semaphores have no close path. Signal and network
operations propagate typed failures instead of asserting success. Remaining defaults are
ordinary absent-option choices, reviewed best-effort metadata, or the compatibility wrappers
listed below; status retains reasons before converting fallible observations to optional
wire fields. The batch missing-ID conversion operates on the validated positive requested-ID
set returned by the production partitioner, rather than erasing a transport failure.

Stdout output in `main.rs` is confined to separate interactive setup; serving uses MCP
frames and tracing/stderr diagnostics. The new media source snapshot does not expose
captions in tracing; its transfer error retains the established non-auth download-error
path rather than adding credential/context chains.

### 6. Concurrency and bounded work

Sampled tool paths and shared validation/fanout helpers preserve local validation → account
acquisition → RPC ordering; cached account status is local. Fanout charges every deduplicated
attempt without failed-channel refunds. Media's distinct per-item and shared-fetch refund
rules remain explicit in `impl_media.rs` and the API inventory.

Synchronous mutexes protect short limiter/buffer/metrics/recovery sections; their guards do
not cross `.await`. Poison maps to fixed typed unavailable identities and is never bypassed
with `PoisonError::into_inner`. Runtime async locks protect operation maps/progress and
serialize output intentionally: the writer gate spans a cancellable/bounded async frame
write to prevent interleaving. It is not a synchronous-lock-across-await regression.

Separate bounds remain in place: input frame size; protocol in-flight admission; fanout
width; whole dialog/search traversal timeouts; cooperative search deadline/partial metadata;
absolute transcription deadline; message wire size; media overhead/base64 caps; raw slots/
bytes; decoded dimensions/pixels/allocation; and CPU worker count. Task 8 source inspection
specifically verifies:

- `wire_budget.rs:12–73` measures complete rendered JSON-RPC with actual response context
  and both JSON forms, keeps real cursor/omission metadata while testing metadata-only
  overflow, then trims messages. Its one-message floor does not exempt metadata.
- `media_contract.rs:185–259` checks the independent 65,536-byte non-binary overhead cap;
  image output budgets stay separate. `image.rs:26–58,98–148` checks encoded length before
  base64 allocation, source dimensions/area/decoder allowance and bounded shrink attempts.
- `media_pipeline.rs:13–15,78–113,204–246` enforces four process-wide slots and 80 MiB raw
  reservations, one deadline for waits/download work and ordered bounded production.
  `runtime/processing.rs:3–25` keeps two process-wide CPU permits inside started closures,
  so dropping an awaiter cannot release a running worker's capacity.
- `ops_media.rs:23–63,107–173` establishes one media deadline before resolution/shared
  fetch and carries it into lazy outcomes. `impl_media.rs:164–197,414–431` applies that
  original deadline to output-stream waits and image work. The private collector
  (`ops_media.rs:316–339`) lazily constructs its iterator, checks reported/observed sizes,
  uses fallible exact reservation and one transfer timeout. Progress does not renew time.
- Recovery reserves keys before routing, rejects duplicate/pending-capacity misuse,
  bounds diagnostic/keyed stores separately, expires completed state after ten minutes
  and removes image data by default. Cancellation tombstones distinguish unavailable from
  success. Recovery does not buffer its own output or promise authentication.
- `session_storage.rs` uses trusted canonical ancestry, current/root ownership,
  sticky-root handling, owned single regular DB/sidecars, 0700 new directories and 0600
  files. macOS directory grants are refused and file ACLs removed. Utility execution is
  captured, kill-on-drop and individually deadline-bounded. Prior ACL/ancestor/cursor
  findings have corresponding regressions and current code fixes.

These bounds are not a hard RSS ceiling or forced abortion of already-started CPU work.
Constructor filesystem/storage work has no aggregate timeout, as documented; interactive
input has no timer. Custom client/bootstrap implementations and collected/cloned embedding
inputs retain their documented responsibilities. The audit does not silently strengthen
those contracts.

### 7. Unsafe code, toolchains and macros

Cargo forbids unsafe code for project targets. Source-wide literal checks found no unsafe
blocks, `#![feature]`, `set_var`/`remove_var`, poisoned-interior recovery or
`filter_map(Result::ok)` in project Rust sources/examples. Nightly remains the chosen tool
suite, not permission for new unstable language gates. Dependencies are outside the local
unsafe prohibition. rmcp/mockall generated output is not indexed; compilation evidence
rather than text search is required for current expansion coverage.

Environment expansion tests inject lookups; loader/path/non-Unicode tests configure child
process environments through `Command::env`/`env_remove`. The intentional child panic is
checked by its parent. Neither process mutates its own environment. Global logging
initialization runs in an ignored child entrypoint invoked by normal parent tests; the
separate file-layer failure test returns before any global initialization.

Existing serde/schemars/thiserror/rmcp/mockall and Tokio macros retain real integration
purposes. No handler macro mini-language, unsafe accessor, third-party private-field
workaround or new application DSL was introduced by Task 8 or the coverage changes.

### 8. Documentation and returned values

The API inventory explains supported/compatibility paths, errors, stage/deadline behavior,
charges, cursor exclusivity, partial results, storage/desktop side effects and caller-owned
memory. Crate/domain/inert composition examples avoid credentials and live networking.
Completed leaves retain scoped `missing_docs`; the audit does not equate warning-free
rustdoc with exhaustive documentation of every legacy public field.

Six S6 consuming builders have useful `must_use` messages. The later seventh builder,
`with_output_format`, initially had only a plain annotation: its behavior is safe but the
useful-message requirement needed the narrow correction below, now source-verified. No mass getter annotations,
new exhaustive restrictions or public `const fn` commitments are needed. `len`/`is_empty`
companions and best-effort cleanup's retained always-Ok `Result` contract remain deliberate.

### 9. Behavior tests, checks and review

Archived S1–S6 evidence identifies independent spec/quality approvals and RED/GREEN tests
for actual failure-policy changes, rather than source-text annotation counts. Current tests
preserve mockable RPC boundaries, paused-time deadline tests, converter/raw fixtures and
property tests. The latest media changes exercise the real grammers cached-byte iterator,
selection/assembly and a private generic transfer collector; animation classification has
both attribute orders covered. The reviewed classifier checks Animated before iterating
Video attributes, preserving metadata extraction. Representative inspected behavior tests
include `transfer_rejects_streamed_oversize_without_reading_another_chunk`,
`transfer_progress_does_not_restart_the_total_timeout`,
`transfer_cancellation_drops_waiting_work`,
`visual_download_selects_the_smallest_adequate_cached_photo_and_keeps_metadata`,
`visual_download_bounds_the_real_remote_iterator_without_polling_its_sender` and
`animated_mp4_classification_does_not_depend_on_attribute_order`. Storage acceptance
retains `review_session_rejects_a_private_parent_below_an_unsafe_ancestor` and
`review_session_database_wal_and_shm_cannot_retain_read_granting_acls`; keyed expiry has
`task8_keyed_recovery_expires_exactly_ten_minutes_after_completion`. These assertions
exercise outcomes and ownership/deadline effects, rather than matching source text.

The scoped media report records mutations disabling reported/streamed caps and animation
classification, each failing the relevant behavior test before restoration. Its 82.58%
library and 51.62% original `ops_media.rs` line figures are scoped coverage measurements,
not substitutes for manual adoption review or complete RPC/network-path acceptance. Thin
live resolution/fetch, upstream Message extraction and defensive branches remain limited.

## Accepted exceptions and retirement conditions

| Existing narrow scope | Accepted reason/current boundary | Retirement or continuing condition |
| --- | --- | --- |
| Extracted/inline test globs | Fixtures can exercise parent internals without duplicate preludes. Production import dependencies remain explicit; intentional facade reexports are separate. | Intentional test-only exception; retain offline behavior protection. No automatic retirement or new production prelude. |
| Complete MCP router impl | rmcp scans methods before declarative macro expansion. Entry points remain thin and bodies live in operation siblings. | Revisit only if the supported macro registration contract changes; tool-list and self-contained schema tests protect it. Cosmetic file length is not a reason. |
| Client state + complete dispatcher | Descendant operation modules use private client state without widening visibility; focused helpers remain separate. | Revisit when an actual responsibility/API change justifies it, while preserving trait behavior and private fields. |
| Established fallible `new` APIs | IDs, names and Telegram client retain published names/signatures. | Explicit Rust API migration with compatibility notes; new fallible APIs prefer `try_new`. No gratuitous aliases. |
| Legacy diagnostic getters/recorders | Conservative zero/empty/None values suppress untrusted poisoned contents but lose failure information; refund refuses unavailable state. | New/internal consumers use fallible `try_*`/independent observations. Remove wrappers only via explicit Rust API migration after consumers move; unavailable status/replay must never masquerade as healthy empty state. |
| Controlled poison/isolation panics | Joined unit threads and checked child exits exercise interrupted state and isolation. | Remain confined to tests; never catch production panics to resume poisoned state. |

The archived domain-derived-Deserialize exception is **retired** by `ef91e86`, not included
above; the current source and constructor/serde regressions were inspected.
The raw-pager compatibility seam, custom-client responsibility, collected media-memory
ownership, non-abortable started workers and same-UID filesystem threat boundary are explicit
architectural/API limits rather than permissions to bypass validating constructors or other
manual requirements. Raw pagers should collapse back to high-level iterators if upstream
provides the required public peer resolution; no hidden/private API is used meanwhile.

## New findings and disposition

No Critical or Important finding. The audit identified these Minor, targeted corrections:

| Finding at reviewed baseline | Consequence and targeted correction | Disposition |
| --- | --- | --- |
| `docs/rust-api.md`, Visual media operation row | Says no aggregate download-batch deadline although production now shares `MediaDeadline` across setup/stream/processing. Correct the row while retaining custom-client/collected-memory qualifications. | Integrator corrected it; current working-tree text source-verified. |
| `src/lib.rs:16–17`, domain example prose | Historical derived-serde warning obscures the five wrappers' now-validated deserialization. Describe constructor-backed Deserialize and retain distinction from direct config validation. | Integrator corrected it; current working-tree text source-verified. |
| `docs/rust-style.md` §8, builder count | Six is the historical S6 count; output-format selection adds a seventh. Update current count without rewriting archived evidence. | Integrator corrected it; current working-tree text source-verified. |
| `src/mcp/server.rs:102`, output-mode builder | Plain `must_use` warns but lacks the manual's useful builder-return message. Add the same useful message as the six existing builders. | Integrator corrected it; current working-tree text source-verified. |

## Verification: existing format versus adopted stricter policy

The adoption auditor ran read-only source/history/navigation inspection, not duplicate Cargo
build/test/coverage runs. The integrator ran the fresh command gate against `4ce5c80` plus
the verified documentation/builder-message corrections and the CI client-step addition.
Every command below exited 0; the auditor inspected the format, selected-lint, full-test and
rustdoc logs in `/private/tmp/telegram-final-{format,strict-lints,tests,rustdoc}.log`.
The integrator's aggregate final-acceptance report owns release/client/security evidence.

| Evidence class | Exact check and scope | Current result |
| --- | --- | --- |
| Existing-format baseline | `cargo fmt --all --check` using checked-in formatter settings | Pass, exit 0; no formatter diff or warning. |
| Established Clippy gate | All-target `-D warnings` is included in the explicit stricter invocation below. | Pass, exit 0; one superset run, not a claim of a separate duplicate baseline run. |
| Explicit stricter adopted lint check | `cargo clippy --offline --locked --all-targets -- -D warnings -D clippy::wildcard_imports -D clippy::missing_errors_doc -D clippy::return_self_not_must_use` | Pass, exit 0, no selected-lint warnings. No broad pedantic/restriction group is claimed. |
| Documentation | `RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps` | Pass, exit 0; warning-free rustdoc. |
| Behavior | `cargo test --offline --locked`, including executable doctests and subprocess parents | Pass, exit 0: 940 library + 3 doctests; 5 ignores at this gate. |
| Source-policy inspection | Full manual/S1–S6 assessment above; targeted new Task 8 and media code review | No Critical/Important findings; all four Minor corrections source-verified. |
| Historical S6 | Archived all-target selected lint/full parallel test/rustdoc results | 866 library + 3 doctests at S6, explicitly historical and not a current count. |

A later test-only ignored cached-transfer latency measurement was added after this production
code gate. Its diff was inspected: 100 cached inert download samples, no sender polling or
credentials, no process-environment mutation, and explicit timing output rather than a flaky
performance threshold. It changes no production code or adoption outcome. Its execution and
the resulting final ignore count are the integrator's aggregate report's responsibility;
the 5-ignore figure above describes the precise completed gate, not that subsequent snapshot.

Navigation began with `ast-index stats`; the sandbox initially refused its external cache
lock, then authorized cache access succeeded. `ast-index update` reported zero changed files
before indexed inspection. Stats reported 177 indexed supported files; the source inventory
query contained all 171 Rust paths under `src`/`examples`. Large relevant files were outlined
before targeted reads. Literal/config/comment checks used `rg`; source inspection checked
test-versus-production context before absence claims. No symbol graph was needed. No source,
HEAD or branch mutation was performed by this auditor.

## Limits and declined areas

This is a scoped standards-adoption assessment, not a second fresh dependency/advisory audit
or independent full security review of the combined branch. Task 9 workflow acceptance and
A1–A9/D1–D2 RPC/conversion traceability are handled separately. Existing reports supply
historical runtime/RSS/coverage and prior review context, without being relabelled as newly
performed measurements here. No Cargo gate, coverage rerun, OOM injection or live download
was independently executed by this auditor.

Validation ownership remains macOS-only for agent work. Linux setup/execution is
owner-managed and does not block this assessment. No Linux tooling/VM/container/run was
arranged. Windows ACLs, configured third-party host rendering, live Telegram/account behavior,
DC migration and live session upgrade acceptance remain unverified. Same-UID hostile storage
mutation remains outside the documented boundary. Optional media M2/M3 remain product
choices; this report provides no release/merge/publication authorization.

## Final post-review integration

The independent combined review's shared-sticky-parent finding was fixed narrowly: the
immediate canonical session parent always excludes group/other writes, and the sticky
exception is limited to trusted ancestors. Disposable negative/positive cases pass; no
new style/API exception was introduced. A test-only portability correction honors TMPDIR.
Fresh final format, explicit selected/all-target Clippy, 942 library tests + 3 doctests and
warning-free rustdoc all exit 0; six normal ignores include two separately executed
measurements and four subprocess entrypoints invoked by parents. Production release and
client/process checks also pass. [Final acceptance](2026-11-01-final-acceptance.md) records
this later gate separately from the source-audit production-gate snapshot above.
