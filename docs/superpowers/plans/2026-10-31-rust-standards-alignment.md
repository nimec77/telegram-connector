# Rust Standards Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Adopt the repository-specific Rust manual with measured, reviewable changes.
**Architecture:** Preserve Cargo/Tokio, domain types, the generic client/limiter seams,
and the macro-constrained MCP router; improve boundaries and enforcement incrementally.
**Tech Stack:** Rust 2024, nightly rustfmt/Clippy/rustdoc, existing test framework.
**Spec:** [Rust coding manual](../../rust-style.md).
**Parent:** [Development roadmap](2026-10-31-development-roadmap.md).
**Status:** S1–S5 are implemented and independently approved. S6 is implemented and
validated; its independent alignment review is pending. Parent owns final whole-branch
review and integration.

## Global constraints

- This is an adaptation, not literal adoption of platform-specific Google rules.
- Keep feature and style changes in separate commits. Never hide behavior changes in reformatting.
- Preserve public Rust paths until their compatibility policy is explicit; preserve MCP schemas.
- Nightly tooling does not imply unstable language features. Keep `async`/`await` and Cargo layout.
- Existing `Arc<T>` ownership, trait mocks and the one tool_router impl are justified.
- Tests may retain local glob imports; generated dependency code is not an alignment target.
- Behavior changes require failing regressions. Formatting/import-only edits need the quality
  gate, not new tests that inspect source text.

## Review focus

1. Poisoned admission control must not admit new RPCs (S2).
2. Diagnostics failure must not break MCP transport or fabricate successful recovery (S2).
3. Test environment safety must include noncooperating readers, not only guarded writers (S2).
4. Public visibility/constructor changes must not silently break the binary or Rust consumers (S3).
5. Module/import movement must not change tool registration or schemas (S4–S5).

## Historical measured baseline

Pre-alignment read-only checks on 2026-10-31, same source baseline as the main audit:

| Check | Result | Meaning |
| --- | --- | --- |
| Existing fmt/Clippy/test gate | Pass; 758 library tests | Existing rules are met, not all proposed rules. |
| rustfmt with proposed import/hex/macro settings | Exit 1; diffs in 8 files | Small mechanical formatting change, no files written. |
| Additional selected Clippy warnings | Exit 0; 171 library warnings, 174 test warnings (158 duplicates) | Candidates for review, not 345 independent defects. |
| Production globs | 13: config defaults and 12 client leaf files | Concrete explicit-import work. |
| Unsafe blocks | 4, all in test EnvGuard; no local SAFETY comments | Safety review before documentation assertions. |
| Unstable feature gates | None found in src | No reason to change toolchain policy for language-feature compliance. |

Diagnostic commands (do not use `--fix` during analysis):

```sh
cargo fmt --all -- --check --config imports_granularity=Module,hex_literal_case=Lower,format_macro_matchers=true
cargo clippy --offline --locked --all-targets -- -W clippy::wildcard_imports -W clippy::missing_errors_doc -W clippy::must_use_candidate -W clippy::return_self_not_must_use -W clippy::exhaustive_enums -W clippy::exhaustive_structs
```

The diagnostic includes 6 consuming-builder warnings and 30 missing Errors-section
warnings. Exhaustiveness suggestions are not blanket requirements for application DTOs.
No lint was added to project configuration by that historical audit. S6 enforcement and
current measured results are recorded below; do not read the old warning counts as current.

## S1: Adopt policy and reproducible formatting

**Files:** `docs/rust-style.md`, `docs/conventions.md`, `AGENTS.md`, create `rustfmt.toml`;
format only the files reported by the check. **Produces:** one authoritative style policy.

- [x] Link the manual from project instructions and replace duplicated/conflicting style
  prose with concise references. Record exceptions for extracted test globs, router layout,
  established fallible `new` constructors, and existing consuming `with_*` builders.
- [x] Add the manual's formatter settings after validating them on the selected nightly.
  Keep toolchain dating coordinated with parent Task 9 rather than changing it twice.
- [x] Run formatter, inspect an exclusively mechanical diff, then `just check`.
- [x] Commit policy and formatting separately from all behavioral changes.

**Verification:** `e338fe7` adopts the manual and exact six formatter settings.
Rust edits are exclusively import grouping and equivalent hexadecimal casing in nine
files. Selected nightly accepted all settings without warnings; `just check` passed
791 tests (5 ignored, 1 doctest ignored). Independent review approved spec and quality.
At that stage, S2/S4 violations were explicit transitional exceptions, not claimed compliant.

## S2: Fix failure/safety policy before cosmetic restructuring

**Files:** `src/rate_limiter.rs`, `src/error.rs`, `src/mcp/observability/{metrics,buffer,transport}.rs`,
`src/main.rs`, `src/config/tests.rs`, `src/config/tests/{env_tests,load_tests}.rs`,
relevant status/limiter/transport tests. **Produces:** explicit poison/signal/test safety behavior.

- [x] Share the actual error-erasure fix with parent Task 2; do not create a competing
  lifecycle rewrite. Preserve typed source/classification where useful and safely redacted.
- [x] Inject poison in a controlled unit-test thread and assert no further RPC acquisition
  succeeds. Return a typed internal failure; refunds must not recover a full usable bucket.
  Define unavailable status explicitly instead of reading tainted contents as valid numbers.
- [x] Test poisoned replay/metrics paths: transport send still succeeds, unavailable
  recovery reports its condition, and no code reads the poisoned interior. Preserve usable
  independent atomic counters; avoid recursive diagnostic logging.
- [x] Replace fallible signal-registration `expect` at `main.rs:160,166` with propagated
  startup errors or documented fallback, integrated with parent Task 5 shutdown tests.
  Keep `link.rs:34`'s proven split invariant unless changing that algorithm.
- [x] Review every environment reader as well as writer in config tests. Prefer child
  process tests using `Command::env` and pure expansion helpers. Remove EnvGuard and
  all self-process environment mutation; a cooperative mutex cannot prove process-wide
  safety, so no false SAFETY comment is added.
- [x] Preserve panic-restoration test intent and make test-only unwinding an explicit
  exception if still needed. Run normal parallel `cargo test --locked` and the quality gate.
- [x] Commit each independently tested behavior change; update obsolete project guidance.

**Verification:** S2 `11c0a71..d340217` independently approved (spec/quality, no
Critical/Important); gate 866 passed, three subprocess entries invoked by parents, existing
ignored doctest carried explicitly to S3. RED/GREEN covers limiter refusal/no RPC,
independent poisoned metrics/replay, unavailable status, non-Unicode secret chains and
stalled authentication RPCs; full parallel gate passed. Task5 already provided propagated
signal errors/shutdown, so no duplicate rewrite. Actual sign-in uses a tested private
decision helper because upstream LoginToken fields prevent a real inert token fixture.
The safe legacy transport-recorder Minor was resolved and reviewed in S3.

## S3: Define supported Rust APIs, then document them

**Files:** `src/lib.rs`, `src/mcp.rs`, `src/mcp/tools.rs`, `src/telegram.rs`,
`src/error.rs`, `src/config.rs`, public type/trait modules and rustdoc examples.
**Produces:** a deliberate public surface and meaningful docs.

- [x] Inventory exports used by the binary and any documented external Rust consumers.
  Classify each as supported API, transitional compatibility reexport or internal.
  Particularly review helpers/image functions under `mcp/tools.rs` and public timeout modules.
- [x] Preserve required external paths; narrow internal visibility without expanding
  private fields just to move code. Treat public removals and non_exhaustive changes as
  compatibility work with release notes, even if MCP JSON remains byte-identical.
- [x] Document retained APIs with error/timeout/partial-result contracts. Add crate docs
  and runnable examples for domain construction and server configuration using mocks or
  inert data. Avoid live-auth examples as required doctests.
- [x] Consider non_exhaustive only for evolving supported external enums/structs. Supply
  constructors before forbidding external struct literals. Do not seal the intentionally
  substitutable client/limiter traits.
- [x] Run `cargo doc --offline --locked --no-deps` and `cargo test --offline --locked --doc`.
  Introduce `missing_docs` on completed modules first, not as an immediate repository-wide
  deny that triggers boilerplate. Keep the public API inventory in documentation.
- [x] Commit a focused API/docs change after the full gate and compatibility review.

**Verification:** S3 `d340217..1d07125` independently approved; no public paths,
field shapes or constructors were removed. `docs/rust-api.md` classifies supported and
transitional facades and intentional S2 DTO changes. `missing_docs` covers completed
leaves. Gate: 866 library tests, three active child entries, three runnable doctests,
warning-free rustdoc; inert external-crate server composition/status example passes.
The review's history fallback documentation Minor was corrected and verified in S4.

## S4: Make source dependencies explicit

**Files:** `src/config.rs`; 12 client leaf modules identified by `use super::*`
(`auth`, `channels`, `guard`, `lifecycle`, `ops_history`, `ops_media`, `ops_message`,
`ops_resolve`, `ops_search`, `ops_stats`, `ops_transcribe`, `resolve`);
other touched imports only. **Produces:** readable explicit dependencies.

- [x] Confirm the leaf-file inventory against the current checkout before editing.
  Replace production globs with explicit imports or module-qualified calls; config
  defaults may use `defaults::...` paths including serde attribute strings.
- [x] Scope traits locally when useful, retain generated TL family qualification, and
  avoid renaming public types or moving operational bodies during this mechanical pass.
- [x] Run formatter, Clippy with wildcard-import warnings, schema/tool-list tests and
  full gate. Allow extracted test globs narrowly; retain intentional facade reexports.
- [x] Commit import cleanup independently from structural refactors.

**Verification:** S4 `1d07125..66111af` independently approved without findings.
Confirmed and removed all 13 production globs; extracted tests and deliberate facade
reexports remain permitted. Only explicit imports/defaults qualification and the carried
history fallback documentation changed. Formatter, wildcard-enabled Clippy, schema/tool-list
checks and full gate passed (866 library, three active child entries, three doctests).

## S5: Split mixed responsibilities where the audit shows value

**Files:** `src/config.rs`; create focused `src/config/types.rs`, `validation.rs`,
`loading.rs`; `src/telegram/client.rs` and appropriate existing/new pure helper siblings.
**Produces:** clearer navigation with stable facade paths.

- [x] Preserve config field names, serde defaults/env expansion and error redaction
  from parent Task 1. Move definitions and logic without changing those contracts.
- [x] Retain root reexports so the binary and existing imports work. Keep client state
  and thin trait dispatch together; move pure cursor/result and focused session-resolution
  helpers only when the move clarifies responsibility. Never weaken field visibility just for file layout.
- [x] Leave the macro-bound server router together. Document its exception and the
  trait dispatcher exception rather than splitting the crate into tiny crates.
- [x] Run config/property/converter tests and the quality gate; compare advertised tool
  schemas before/after. Commit structural changes separately from new behavior.

**Verification:** S5 `66111af..54bccd8` independently approved without findings.
Config facade preserves every type/method/field/default while private types/loading/
validation own their responsibilities; client pure cursor/results and async resolution
helpers retain private state access without widening fields. Gate: 866 library tests,
four active subprocess entries, three doctests; focused config/converter/client/property
and schema/tool-list checks passed. Real tools/list captures for current/legacy/unversioned
protocols are exactly equal before/after key normalization: 16 tools, 62,153 bytes,
SHA-256 `4e88de47c5afcefd1dfb2d323ea18ecb2ff40b2ecd1fbfe43c922c48f9cec221`.

## S6: Add targeted API ergonomics and enforcement

**Files:** `src/mcp/server.rs`, `src/telegram/albums.rs`, `client/walk.rs`,
`src/mcp/tools/shaping.rs`, `Cargo.toml`, `justfile`, CI and manual as needed.
**Produces:** actionable compiler feedback without blanket lint churn.

- [x] Add must_use to the six flagged consuming builders; inspect other candidates
  individually. Do not annotate every getter solely to silence a diagnostic report.
- [x] Review positional bools in PostCounter/MessageWalk/shaping. Use a small named enum
  or options struct when it improves real call sites; retain JSON boolean fields and
  already clear predicates. Behavior tests must remain unchanged for pure refactors.
- [x] New fallible constructors follow the manual; existing public `new` methods remain
  compatibility exceptions or gain an alias/deprecation as a separate API migration.
  Add TryFrom/FromStr only when their callers benefit; preserve validation.
- [x] Enable selected cleaned lints (wildcard imports, missing error docs on retained APIs,
  consuming-builder must_use); keep other advisory candidates in review. Verify lint names
  on the selected toolchain and document any narrow exceptions with reasons.
- [x] Run full gate and rustdoc checks and update the baseline. No broad
  pedantic/restriction deny or automatic whole-crate fix.
- [ ] Obtain independent S6 review against the adopted manual (coordinator dispatch);
  final whole-branch review/integration belongs to parent.

**Implementation and validation (2026-10-31):** Six consuming server builders now have
useful `must_use` messages. `AlbumMode` names page/walk constructor mode and converts the
named domain flag once at each operation boundary; admitted trailing siblings and the
`PostCounter::admit` predicate stay intact. `CursorPolicy` names channel continuation
independently from compact layout, retaining global search's existing compaction behavior.
Public JSON/domain bools, constructor names, paths and all operational semantics remain.
No unused aliases/standard conversion traits or annotation-count tests were added.

Current measurements use selected `nightly-2026-10-31` and the shared Cargo target directory:

| Check | Current result / scope |
| --- | --- |
| Exact lint help | `clippy-driver -W help` confirms wildcard_imports, missing_errors_doc, return_self_not_must_use and advisory must_use_candidate. |
| Pre-S6 selected lint measurement | 23 library warnings: six builders and 17 missing error contracts (test output duplicates them). |
| Adopted selected lint gate | Clean across all targets; Cargo warning-level wildcard_imports, missing_errors_doc and return_self_not_must_use become errors through gate `-D warnings`. |
| Error docs | 17 retained API contracts completed, including always-Ok best-effort log cleanup; existing S3 contracts remain checked. |
| Advisory must_use_candidate | Opt-in measurement: 33 library suggestions; 49 test suggestions including 33 duplicates. Remains advisory, with no mass annotation or auto-fix. |
| Unsafe policy | Cargo forbids unsafe code for project targets; all-target Clippy and full tests compile current macro/test/example code. Dependencies are outside this scope. |
| Documentation policy | S3 module-scoped missing_docs retained; `RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps` passes; no blanket missing_docs deny. |
| Full gate | Formatting, selected Clippy, parallel full tests: 866 library passed, four child entrypoints invoked by parent tests; three doctests passed. |
| Local/CI wiring | `just check` includes warning-free rustdoc; full cargo test runs doctests. `just check-docs` also has explicit doc tests. Pinned CI actions and other jobs preserved. |

Independent S6 approval is pending; these are author-observed implementation checks.

## Integration and effort

S1 can follow the parent's urgent security patches. S2 belongs with parent Tasks 2/5;
S3–S5 should follow dependency/API stabilization and precede broad DTO migration where
practical. S6 follows their cleanup. One owner coordinates files shared with the parent plan.
Estimated incremental work: **3–6 engineering days**, with safety/API decisions the main
uncertainty; do not add estimates for shared parent tasks twice. Implementation evidence
is recorded above; the historical audit remains historical.
Adoption does not imply literal compliance with every Google rule, and no final parent
integration or whole-branch approval is claimed.
