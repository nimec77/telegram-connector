# Development audit and proposed direction

**Status note (2026-11-01):** Restored as the active audit/acceptance reference.
Tasks 1–8 are integrated in 0.24.0; Task 9/10 acceptance is now complete on the final
acceptance branch, as recorded in the [roadmap](../plans/2026-10-31-development-roadmap.md)
and [final report](../../validation/2026-11-01-final-acceptance.md). Findings below describe
the original audited baseline; they are not a claim that fixed defects remain present.

Date: 2026-10-31. Baseline: `40f7b114f7024822ee1c77445d46e0ee4ab733b2`, package 0.22.6.
Status: analysis and proposal only; no implementation authorized by this document.

Follow-up requests are incorporated through the [Rust coding manual](../../rust-style.md)
and [media delivery analysis](2026-10-31-media-delivery-design.md). The completed standards
alignment plan is archived in Git history.

## Scope and decisions

The user requested current MCP support, one consistent data format, current dependencies,
and a correctness/security/improvement audit. They explicitly selected **local stdio,
current core MCP, optional features assessed separately**. Remote HTTP is outside this plan.

The official protocol version is **2026-07-28**, not a semantic version named 2.0.
JSON-RPC 2.0 is the wire envelope. Use the dated version in requirements and tests.
[Official versioning](https://modelcontextprotocol.io/docs/2026-07-28/learn/versioning).

Recommended approach: incremental modernization of the existing Rust server. Keep the
trait seam, 16 tools, domain types, raw Telegram envelope handling, and bounded responses.
Fix confirmed defects first; update dependencies in separate groups; prove protocol behavior
at the wire; then publish consistent typed JSON contracts. Alternatives considered:

| Approach | Tradeoff |
| --- | --- |
| Staged modernization (recommended) | Reviewable migrations and useful releases at each stage; temporary compatibility adapters. |
| One coordinated breaking rewrite | A cleaner immediate contract, but couples SDK, Telegram, error, and schema regressions. |
| Framework/language replacement | No demonstrated benefit sufficient to justify replacing the existing tests and Telegram integration. |

These implementation choices are recommendations, not claims of user approval. Keep TOML
configuration: the single-format proposal concerns the MCP application data contract.
Converting configuration, logs, protocol envelopes, and media into one identical schema
would conflate different interfaces without improving the tool contract.

## Baseline evidence and limits

- `cargo fmt --all --check`: passed.
- `cargo clippy --offline --locked --all-targets -- -D warnings`: passed.
- `cargo test --offline --locked`: 758 library tests passed, 5 ignored; binary has 0
  tests; 1 doctest ignored. This is a baseline, not proof of production-path correctness.
- Current source has input-schema integrity tests, pure tool-list/cache tests, mock tool
  tests, and transport decorator tests. No repository-owned end-to-end modern stdio
  conformance suite was found. `scripts/mcp_probe.py` uses legacy initialization.
- Historical coverage in memory is 78.33% library lines; it was **not remeasured**.
  Production RPC glue is frequently replaced by mocks, leaving an important blind spot.
- `ast-index stats` and `update` failed on a sandbox-protected cache lock. Used `rg` and
  targeted reads as permitted. No live Telegram account, authentication, private messages,
  or real secrets were used. No production/source/dependency changes were made.
- The audit is scoped source review, existing checks, and advisory matching; it is not
  exhaustive penetration testing, fuzzing, or proof that no other vulnerabilities exist.

## Findings

P1 means address before feature work; P2 means the next correctness/hardening stage.
Dependency advisory matches establish affected versions, not exploitability in this app.

| ID | Priority / confidence | Evidence and impact | Required result |
| --- | --- | --- | --- |
| A1 | P1, reproduced | `src/config.rs:358–361` expands environment variables then retains TOML diagnostics; `src/main.rs:18` propagates them. A malformed `api_hash` line prints its value to stderr. | Diagnostics retain location and safe context without source excerpts or secret values, including error chains/debug formatting. |
| A2 | P1, source-confirmed | `src/telegram/client/lifecycle.rs:75,83`: `is_authorized` and `get_me` lack timeout bounds. `src/main.rs:57,106` awaits them before serving. `is_authorized` is an RPC, and errors become false/authentication failure. `client/channels.rs:13` walks all dialogs without an overall timeout. | Bounded startup/status/channel traversal, accurate transport versus authentication errors, nonfatal Premium warmup. |
| A3 | P2, source-confirmed | `src/mcp/server/impl_channels.rs:21,57`: list/basic info unmetered; `impl_status.rs:16,45`: status/account RPCs unmetered; `impl_media.rs:204`: Premium lookup precedes acquisition. | Local validation → one defined operation charge → network work, including cache-miss paths. Prefer a zero-RPC cached status tool. |
| A4 | P2, source-confirmed | `src/telegram/transcription.rs:77–79` checks deadline before sleeping, then polls without remaining-budget enforcement; `client/ops_transcribe.rs:81` applies a separate RPC timeout. A 1-second polling budget can start a poll at 2 seconds. | Absolute deadline across the tool operation; return latest partial after timeout once any transcription state exists. |
| A5 | P2, source-confirmed | `src/logging.rs:150–159` deletes old files whose names contain `.log`, including other applications' logs/backups in a shared configured directory. | Cleanup selects only this application's supported rolling filename pattern. |
| A6 | P2, existing known defect | `src/telegram/types/names.rs:17` rejects short usernames accepted by request parsing; `converters/channel.rs:24` discards rejected values. | Preserve existing valid short Telegram usernames through conversion and forward attribution. |
| A7 | P2, contract gap | `src/mcp/server.rs` returns `Result<String,String>` for 14 tools and content blocks for 2. `helpers.rs:183` serializes success JSON; errors are strings. Output structs derive schemas but endpoints do not publish them. | Typed `structuredContent`, per-tool `outputSchema`, shared error contract, explicit media association. |
| A8 | P2, verification gap | `src/mcp/tests/server_core.rs` tests helpers, not actual request dispatch; source inherits discovery and version behavior from rmcp. | Offline modern/legacy wire tests before claiming full conformance or upgrading SDK. |
| A9 | P2, source-confirmed | `src/main.rs:129–151` reads/logs `shutdown_timeout_seconds`, then sleeps a fixed 100 ms after the select; it does not use the configured value to drain work. | Apply the configured drain deadline, then cancel remaining work; verify signal handling and no detached continuing RPCs. |

Safe A1 reproduction used the existing debug binary and fake data only:

```sh
printf '[telegram]\napi_id = 1\napi_hash = "FAKE_AUDIT_SECRET";\n' |
  ./target/debug/telegram-mcp --config /dev/stdin
```

It exits with a parse error containing the fake hash. No network is reached.

Further investigations, **not demonstrated vulnerabilities**:

- Verify new session directories/database/WAL permissions with disposable offline files.
  `create_dir_all` and the current SQLite open path do not explicitly establish a private
  permissions policy. Do not inspect or rewrite a user's real session for this test.
- Add explicit decoded image pixel/memory bounds and a process-wide encode concurrency
  bound. `image::load_from_memory` uses library defaults (currently a 512 MiB allocation
  limit, no width/height cap), so calling decoding unlimited would be inaccurate.
- Bound incoming stdio frame size and in-flight work; output/download limits do not bound
  input allocation. Verify cancellation cleanup in metrics and background/blocking work.
- Review raw search text, identifiers, and links logged at info level in `server.rs` for
  privacy. Prefer counts, duration, and sanitized errors by default.
- Numeric peer resolution uses bare IDs; test user/group/channel same-number fixtures
  before asserting a live collision or changing the identity model.
- Recovery currently returns process-wide recent responses. A shared stdio process is
  not a conversation boundary. Require explicit correlation keys for routine recovery;
  keep unfiltered recovery explicitly diagnostic. This is context-mixing risk within the
  configured account, not evidence of cross-account access.

Preserve streamed and reported download caps, response-shaping budgets, album semantics,
per-channel cursors, no global message-ID cursor, partial/per-item failures, zero-extra-RPC
forward attribution, secret newtypes, and deterministic time tests.

## MCP coverage and target

Current rmcp **3.1.2** already implements modern metadata dispatch, inherited
`server/discover`, result discrimination, and legacy initialization. Source inspection of
its `service/server.rs` and `handler/server.rs` confirms these paths; the presence of
`InitializeResult` in this project is **not** evidence that modern support is missing.
The SDK's default discovery currently supplies zero/private cache hints.

| Area | Current project evidence | Planned acceptance |
| --- | --- | --- |
| Discovery and versions | SDK inherited; project advertises tools only | Modern discovery and first direct request both work; unsupported version gives the specified error; advertise only tested versions. |
| Per-request metadata | SDK inherited | Interleaved requests do not inherit another request's version/capabilities; malformed/missing required metadata is rejected appropriately. |
| Result envelopes | SDK inherited | Modern `resultType`, identity metadata, error codes, notification handling and correlation tested on actual stdio framing. |
| Legacy compatibility | Supported by SDK and probe | Keep tested `2025-11-25` handshake; modern-only fields follow version policy; older versions remain advertised only if tested. |
| Tool listing/caching | 16 stable tools; one-hour/private hints gated by version | Actual `tools/list` and `server/discover` cache fields satisfy current specification; no accidental list-change capability. |
| Schemas/content | Input refs tested; JSON text and image blocks | Self-contained JSON Schema 2020-12 input/output; structured successes/errors; image blocks remain usable. |
| Cancellation/progress | SDK mechanisms; wrappers lack progress context | Cancel stops new RPCs and cleans accounting; progress only for provided tokens, increasing and stopped at completion. |
| MRTR and subscriptions | SDK mechanisms; no product use | Verify dispatch and unsupported-capability behavior. Do not advertise product elicitation or change feeds without a real implementation. |
| Removed/deprecated paths | No product dependency identified | Modern removed methods rejected; keep only necessary legacy behavior; do not add deprecated sampling/roots/protocol logging. |
| HTTP-only requirements | No HTTP transport | Header routing, HTTP sessions/SSE changes and OAuth issuer checks are not applicable to this stdio scope. |

Version/lifecycle rules: [compatibility specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning).
Result, metadata, state and schema rules: [base specification](https://modelcontextprotocol.io/specification/2026-07-28/basic).
Changes and deprecations: [release changes](https://modelcontextprotocol.io/specification/2026-07-28/changelog).
Caching includes discovery: [caching specification](https://modelcontextprotocol.io/specification/2026-07-28/server/utilities/caching).
Cancellation and progress: [cancellation](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/cancellation),
[progress](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/progress).

## One JSON contract

Recommend **JSON + JSON Schema 2020-12**. A prose/structured-text dialect loses typed
validation and makes errors/pagination harder to consume. One schema system means shared
conventions and a schema per tool, not identical fields for unrelated operations.

Proposed shared application envelope, distinct from MCP's own JSON-RPC/result envelope:

```json
{
  "schema_version": "1",
  "data": {"messages": [], "has_more": false},
  "error": null
}
```

Define `ToolResponse<T>` with success `{schema_version:"1",data:T,error:null}` and
failure `{schema_version:"1",data:null,error:ToolError}`. `ToolError` has a stable string
`code`, safe `message`, boolean `retryable`, and optional integer `retry_after_seconds`.
Tool execution failures set MCP `isError:true`. Protocol faults remain JSON-RPC errors;
do not wrap them as successful tool data. Batch partial failures remain successful data
with per-item errors, rather than converting every partial result into total failure.
Test typed-argument decoding failures at the router as well as validation inside handlers:
actionable argument errors use the tool error contract; malformed CallToolRequest envelopes
and unknown tools remain protocol errors.

- Use one serialized typed value to produce `structuredContent` and any matching JSON
  text fallback. Publish the corresponding success/error union as `outputSchema`.
- Keep dates RFC3339 UTC, snake_case application keys, and documented omission/null rules.
  For new canonical outputs use decimal strings for channel/user IDs, integer message
  IDs, and explicit channel association. Keep input coercion at the existing boundary.
- Media retains standard MCP image blocks. Its `data` contains an ordered items array
  with channel/message identifiers and `content_index`, plus the batch summary; it does
  not duplicate base64 bytes in JSON. A failed item has no image index.
- Existing text consumers need migration. Ship an explicitly documented temporary
  `legacy` output mode retaining old text shapes and no conflicting output schema; make
  canonical `json_v1` the new default in a documented breaking release. Do not silently
  change the meaning of existing fields within a mode. Both modes retain JSON text.
- For search, recent-history (including fan-out), and message-batch results, measure the
  complete serialized MCP response against `response_byte_budget` (default 40,000),
  including fallback duplication. Preserve the one-message floor; metadata-only overflow
  returns a bounded `response_too_large` tool error rather than an unbounded success.
  The 40,000-byte budget does **not** apply to media. Preserve existing per-image/batch
  base64 caps; additionally cap serialized non-binary media overhead at 65,536 bytes.
  Metadata-only overflow produces a small error and no images, with existing no-image
  refunds. Recovery retains its own buffer count/per-entry bounds; test replay overhead.
- Proposed canonical recovery: optional `recovery_key` (1–128 printable ASCII characters)
  on recoverable tool calls, supplied by the caller before dispatch; recommend unique
  random values. Reserve the key before work, reject duplicate pending/retained keys
  without RPCs, and let `get_last_responses` select explicit `recovery_keys` in JSON mode.
  This works when the original result is lost; JSON-RPC request IDs alone do not because
  they may be reused. Results expire after 10 minutes, capacity eviction, or restart;
  unknown/expired keys return `response_unavailable`, pending keys `response_pending`.
  Oversized entries remain explicit unavailable-payload stubs. Keys prevent accidental
  selection mistakes, not authenticated conversation isolation. Calls without a key
  have no targeted recovery guarantee; unfiltered last-N remains legacy diagnostic only.

The envelope and migration policy are proposed decisions for the later implementation;
the requested analysis does not enable them. MCP's native structured output and content
blocks are described in the [tool specification](https://modelcontextprotocol.io/specification/2026-07-28/server/tools).

## Optional feature assessment

These are separate from the stdio core acceptance gate. The [official extension catalog](https://modelcontextprotocol.io/extensions/overview)
distinguishes Stable, Beta and Experimental and requires explicit opt-in.

| Feature | Fit / recommendation |
| --- | --- |
| Tasks (Beta) | Best next candidate for long transcription. First fix deadlines; later design task lifetime, handles, quotas, cancellation, expiry and persistence. |
| MCP Apps (Stable) | Optional future media/search viewer; requires UI assets and compatible hosts, no need for current tools. |
| Skills over MCP (Beta) | Potential channel research workflows; defer until clients and workflows justify it. |
| Server Cards (Beta) | HTTP discovery; defer with remote transport. |
| OAuth client credentials / enterprise authorization (Stable) | Remote deployments; outside the selected local scope. |
| Resources / prompts / elicitation | Optional core capabilities. Add only for an explicit browsing/workflow/input use case. Keep authentication in interactive setup; never solicit Telegram passwords in model-visible tool results. |
| Experimental extensions | No default adoption; reassess and pin revisions if a concrete need emerges. |

## Completion definition for the future implementation

All confirmed findings addressed with behavior regressions; all direct dependencies at
the latest non-yanked stable release when implemented; transitive dependencies at their
latest compatible releases with any upstream blockers documented; fresh advisory scan
clean or narrowly justified exceptions; core stdio matrix proven for modern and retained
legacy clients; all 16 canonical tool outputs validate; relevant macOS checks pass.
For the remaining work, Linux setup and validation are owner-managed if needed and are
not an agent acceptance gate, per the user's [validation ownership instruction](../plans/2026-10-31-development-roadmap.md#validation-ownership-for-remaining-tasks).
No feature is advertised merely because its SDK type exists. No broad rewrite, remote
deployment, live account mutation, or release publication is part of this analysis.

## Rust-standard alignment findings

The requested Google-guide analysis is translated into a project manual rather than
imposing Chromium/Fuchsia rules on a Cargo/Tokio service. The manual distinguishes
required rules, preferences and architecture/compatibility exceptions.

| Finding | Evidence / disposition |
| --- | --- |
| Explicit imports | 13 production globs: `config.rs:8` plus 12 Telegram client leaf files; replace without touching test globs/facade reexports. |
| Poisoned state | `rate_limiter.rs:92`, observability metrics/buffer recover interior state. The limiter has a written rationale; replace deliberately with tested admission refusal/unavailable diagnostics, not panicking locks. |
| Fallible OS setup | `main.rs:160,166` expects signal-handler setup to succeed; handle errors with lifecycle/shutdown work. |
| Broad API and missing contracts | `lib.rs:1–8`, `mcp/tools.rs:7–11`, config/error/server types expose implementation details; classify visibility before broad doc/exhaustiveness changes. |
| Consuming builders | Six selected Clippy findings in server configuration; add must_use where discarding the returned server discards settings. |
| Mixed module responsibilities | Config types/loading/validation and client pure helpers merit selective extraction; preserve the macro-bound tool_router and private client dispatch. |
| Unsafe test assumptions | Four environment-mutation unsafe blocks in test EnvGuard; assess concurrent readers before claiming a safety proof, preferably use subprocess/pure tests. |
| Mechanical formatting | Proposed formatter settings produce diffs in 8 files. Existing default-format gate still passes. |

Additional style Clippy checks exited successfully with warnings: 171 library and 174
test diagnostics (158 duplicates). They are review candidates, not a defect count.
No unstable language feature gates were found in `src`; nightly selection is not itself
a style violation. Public non_exhaustive attributes and constructor renames require
compatibility planning, not an automatic fixer. Full task/file mapping is in S1–S6.

## Media conclusion

Inline base64 image blocks are the portable stdio representation. The current bytes-to-image
pipeline is reasonable, but collecting all batch raw downloads can retain about 200 MiB
at default per-file caps, before decoded/output buffers; that is an inferred ceiling,
not a measured memory peak. Prioritize backpressure, global processing bounds, and encoding
only accepted candidates. Resource links defer retrieval; MCP blob reads still use base64.
Direct binary file access needs a separately verified shared-filesystem client adapter.
The media proposal supplies the protocol references, budgets and phased acceptance checks.

## Dependency snapshot

Registry checked at 2026-10-31T04:43:02.964070+00:00. Values below compare Cargo.lock with the highest non-yanked stable semver, excluding prereleases. Manifest requirements are not installed versions. Recheck on implementation day.

| Crate | Locked | Latest stable | Scope |
| --- | --- | --- | --- |
| [anyhow](https://crates.io/api/v1/crates/anyhow) | 1.0.104 | 1.0.104 | normal |
| [async-trait](https://crates.io/api/v1/crates/async-trait) | 0.1.92 | 0.1.92 | normal |
| [base64](https://crates.io/api/v1/crates/base64) | 0.22.1 | 0.23.1 | normal |
| [chrono](https://crates.io/api/v1/crates/chrono) | 0.4.45 | 0.4.45 | normal |
| [clap](https://crates.io/api/v1/crates/clap) | 4.6.6 | 4.6.7 | normal |
| [dialoguer](https://crates.io/api/v1/crates/dialoguer) | 0.12.0 | 0.12.0 | normal |
| [directories](https://crates.io/api/v1/crates/directories) | 6.0.0 | 6.0.0 | normal |
| [filetime](https://crates.io/api/v1/crates/filetime) | 0.2.29 | 0.2.29 | dev |
| [futures](https://crates.io/api/v1/crates/futures) | 0.3.33 | 0.3.34 | normal |
| [image](https://crates.io/api/v1/crates/image) | 0.25.10 | 0.25.10 | normal+dev |
| [mockall](https://crates.io/api/v1/crates/mockall) | 0.14.0 | 0.15.0 | dev |
| [proptest](https://crates.io/api/v1/crates/proptest) | 1.11.0 | 1.12.0 | dev |
| [rmcp](https://crates.io/api/v1/crates/rmcp) | 3.1.2 | 3.8.0 | normal |
| [schemars](https://crates.io/api/v1/crates/schemars) | 1.2.2 | 1.2.3 | normal |
| [secrecy](https://crates.io/api/v1/crates/secrecy) | 0.10.3 | 0.10.3 | normal |
| [serde](https://crates.io/api/v1/crates/serde) | 1.0.229 | 1.0.229 | normal |
| [serde_json](https://crates.io/api/v1/crates/serde_json) | 1.0.151 | 1.0.154 | normal |
| [tempfile](https://crates.io/api/v1/crates/tempfile) | 3.27.0 | 3.27.0 | dev |
| [thiserror](https://crates.io/api/v1/crates/thiserror) | 2.0.20 | 2.0.21 | normal |
| [tokio](https://crates.io/api/v1/crates/tokio) | 1.53.1 | 1.54.1 | normal+dev |
| [toml](https://crates.io/api/v1/crates/toml) | 1.1.4+spec-1.1.0 | 1.1.8+spec-1.1.0 | normal |
| [tracing](https://crates.io/api/v1/crates/tracing) | 0.1.44 | 0.1.44 | normal |
| [tracing-appender](https://crates.io/api/v1/crates/tracing-appender) | 0.2.5 | 0.2.5 | normal |
| [tracing-subscriber](https://crates.io/api/v1/crates/tracing-subscriber) | 0.3.23 | 0.3.23 | normal |

11 of 24 distinct registry direct dependencies are behind; 13 are already current. `image` and `tokio` also have dev feature declarations.

The three direct grammers dependencies share revision
`9fef0bae1e59b6138ae7777c783983934a80e129` (0.10 series). Upstream HEAD checked on this
date is `1e31e73cd9797790905bbea82cd75adcbe6f1242` (2026-10-27), with all three at
0.11.0 and 41 intervening commits. Upgrade all three together to one reviewed immutable
revision. Changes include new Telegram layers, peer-cache fixes, sender/transport changes,
and error API changes. `Message.peers` is still `pub(crate)` there: retain the raw pagers.
[Upstream comparison](https://codeberg.org/Lonami/grammers/compare/9fef0bae1e59b6138ae7777c783983934a80e129...1e31e73cd9797790905bbea82cd75adcbe6f1242).

rmcp migration needs targeted wire tests: 3.2 fixed legacy initialization, 3.5 separated
latest protocol from latest handshake protocol, and 3.6 added incoming frame bounds,
EOF cancellation, and malformed-message checks. Review upstream changes through 3.8.0,
particularly custom transport instrumentation and cache hints.
[SDK release history](https://github.com/modelcontextprotocol/rust-sdk/releases).

Fresh `cargo-audit 0.22.0` scan used RustSec DB commit
`2295466c4e9df26647fd67d8fa1c705ac2fc7f81` (2026-10-30), 1,395 advisories and 303 locked
packages. It exited **1**, with one vulnerability and one separate unsoundness warning:

| ID | Locked dependency | Result / upgrade |
| --- | --- | --- |
| D1 / P1 | `ctutils 0.4.2` | [RUSTSEC-2026-0385](https://rustsec.org/advisories/RUSTSEC-2026-0385.html): constant-time guarantees fail in some generated operations; patched >=0.4.4, latest checked 0.4.7. Production crypto dependency path exists through digest and grammers; exploitability not demonstrated. |
| D2 / P1 | `tokio 1.53.1` | [RUSTSEC-2026-0386](https://rustsec.org/advisories/RUSTSEC-2026-0386.html): `AsyncFd` constructors violate I/O safety; patched >=1.53.3 (or the 1.51.6 maintenance branch). Target 1.54.1. No direct project AsyncFd use found; transitive reachability not established. |

The audit must include transitive, build, dev and platform-specific dependencies. Updating
direct requirements alone is insufficient. This session did not calculate an exhaustive
latest-version comparison for all 303 packages or attempt a new resolution; use targeted
updates followed by a full lockfile review during execution. Incompatible transitive
major releases require an upstream migration, not an arbitrary forced override.

CI uses `actions/checkout@v4`; latest checked is
[v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1). Review runner/runtime
requirements, then pin the reviewed release SHA. `dtolnay/rust-toolchain@nightly` and
`rust-toolchain.toml` track floating nightly: choose a tested current dated nightly for
reproducibility, with a separate floating-nightly compatibility check. No security scan,
dependency update bot, or explicit dependency/source/license policy exists in current CI.
Do not silently switch to stable Rust or drop required test-util features.
