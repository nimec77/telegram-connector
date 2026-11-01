# Final modernization acceptance — Tasks 9 and 10

Date: 2026-11-01. Platform: native macOS 27.2, arm64. Scope: the original ten-task
roadmap, including the scoped production media-test work. This is engineering acceptance,
not release publication, hosted CI qualification, or a claim of live-account testing.

**Candidate status:** Task 9 criteria are verified; Task 10 evidence below is complete
except the independent final combined review. Do not mark Task 10 closed until that
verdict is recorded and any blocking findings are resolved.

## Task 9 — continuous maintenance

All six requirements have concrete evidence in the
[workflow acceptance report](2026-11-01-task9-workflows.md):

1. Official checkout v7.0.1 and Rust installer SHAs verified; contents:read everywhere,
   persist-credentials:false, Node24/runner requirements checked against pinned sources.
2. Required nightly-2026-10-31 matches local/CI/security; floating nightly remains separate.
3. PR/push/scheduled fresh vulnerability, unsoundness and yank checks are configured,
   with no approved advisory exceptions. Native fresh execution passed below.
4. Dependabot schema and 11 semantic routing cases prove rmcp grouping, manual grammers
   revisions, compatible groups and individual incompatible/0.x minor updates.
5. Sources are restricted to crates.io and reviewed Codeberg revisions; complete metadata
   license policy, exact scoped exceptions and hard yank policy pass on the full graph.
6. Checksum-verified macOS actionlint, parsed YAML, Bash syntax and policy checks pass.
   The real independent Python media client now joins CI's process/probe checks.

`cargo-deny` was previously unavailable. That gap is now resolved for acceptance using
the exact pinned **0.20.2 Darwin arm64** binary in temporary task storage, not a global
installation. Official release-asset SHA256 and companion checksum both match
`fe67d82a10d8597a3549364cb733a3f9cc1bfff9031b7ae46384a9f2a72090c3`.
`cargo-deny --locked check sources advisories` exits 0: advisories ok, sources ok.
Fresh `cargo audit --deny unsound --deny yanked` exits 0, scanning 341 locked identities
against 1,395 advisories after fetching the database. Full declaration/source policy
passes 340 external identities and all 25 policy regression fixtures.

Existing Linux CI configuration is preserved and statically reviewed. No Linux tooling,
VM/container, execution or workflow dispatch was arranged. Hosted runner execution is
separate evidence and is not a gate under the explicit macOS ownership policy.

## Task 10 — dependency disposition

The [complete machine-readable inventory](2026-11-01-dependency-inventory.json) records
all 340 external identities, all 31 direct declarations, source/revision/license metadata,
current stable registry versions and parent constraints/removal paths for old lines.

- All 316 registry package names (332 resolved identities) were queried against primary
  crates.io metadata with no lookup errors. Every direct registry dependency matches the
  current stable version; there are no older versions within the same compatibility line.
- Fifty-three resolved identities remain on older incompatible lines. This is explicit,
  not concealed by a global latest-version claim. Examples: jsonschema 0.60.2 requires
  fancy-regex^0.19; fancy-regex 0.19.2 requires bit-set^0.8; fraction/num require num-bigint
  ^0.4; ahash/jsonschema require getrandom^0.3; libsql 0.10 requires thiserror^1 and its
  libsql-sys requires zerocopy^0.7. Full parent/version/range rows are in the inventory.
- Removal requires an upstream parent migration accepting the newer line. No incompatible
  override or broad lockfile regeneration was used. Windows/WASI-specific identities are
  inventory entries, not a claim those platforms were built or executed.
- All eight Git package identities use the same reviewed grammers repository/revision.
  The three direct pins remain 1e31e73cd9797790905bbea82cd75adcbe6f1242, still upstream HEAD.
- Cargo.toml and Cargo.lock remain unchanged by this acceptance work. Root licensing stays
  unassigned under the existing explicit policy; no distribution license grant is invented.

## Quality and protocol evidence

The required toolchain is nightly-2026-10-31, rustc 1.101.0-nightly (92b5b8c87), LLVM 23.1.4.
Existing-format verification is separate from stricter policy verification:

| Check | Exact scope | Result |
| --- | --- | --- |
| Existing format | cargo fmt --all --check | exit 0 |
| Stricter lints | cargo clippy --offline --locked --all-targets -- -D warnings -D clippy::wildcard_imports -D clippy::missing_errors_doc -D clippy::return_self_not_must_use | exit 0 |
| Library/binary tests and doctests | cargo test --offline --locked | 940 library tests  + 3 doctests pass; 6 normal-suite ignores: four subprocess entrypoints plus two explicit benchmarks |
| Public documentation | RUSTDOCFLAGS=-D warnings cargo doc --offline --locked --no-deps | exit 0 |
| Actual production features | cargo build --offline --locked --release, separately from examples | exit 0; binary reports0.24.0 |
| Native examples | offline_mcp_fixture and inert_server | exit 0 |
| Independent Python media client | real sixteen-tool routes, both output modes × both protocol versions | exit 0 |
| Python probe | modern, direct-modern, legacy | exit 0 |
| Native process lifecycle | idle/pending EOF, SIGTERM and saturated bootstrap output | exit 0 |

The raw Rust NDJSON harness and independent Python process client are two client
implementations. Together they exercise modern 2026-07-28 and legacy 2025-11-25, schemas,
metadata/capability isolation, argument faults, media association, keyed recovery,
cancellation/progress and binary omission. Returned JPEG bytes were locally rendered.
A configured third-party host UI and live Telegram/account behavior are not claimed.
The roadmap requires a live smoke only if later requested; none was requested here.

## RPC/conversion coverage and performance

The [scoped media report](2026-11-01-media-download-coverage.md) records production policy
coverage in the original operations file, not relocated code: 39/246 lines (15.85%) to
143/277 (51.62%). Whole-library unfiltered coverage is 9,301/11,263 (82.58%); inline test
code may be included. Live resolution/fetch and upstream Message accessor execution
remain explicitly unexercised. Coverage is evidence of the named decisions, not proof
of every live RPC path.

A fresh debug cached-transfer benchmark uses the real grammers iterator, 100 samples,
a 505,774-byte 1280×720 JPEG and no sender runner: nearest-rank p50=50,625ns,
p95=58,792ns, max=1,038,292ns. It measures selection/transfer/assembly after fixture
construction, excluding live RPC, upstream Message extraction and MCP image processing.
The benchmark is explicit/ignored in the normal suite and was run separately (exit 0).

The existing two-call, twenty-image near-cap benchmark remains representative for the
unchanged raw/image/worker subsystem: 40 MiB completed raw peak, 488,390,656-byte process
RSS, 59,505 ms processing elapsed, 11,794,240 encoded bytes, using 4096×2048 sources padded
to 20 MiB. Relevant production/fixture/helper files are byte-identical to the measured
revision a61de2c; the source collector/classifier change does not participate in that
benchmark. The [earlier evidence](2026-11-01-task8.md) retains the full context. This is
not a hard RSS ceiling or a live latency claim. Raw admission, decoded allocation,
resize temporaries, final encoded strings and allocator memory remain separate.

## Finding-to-source-to-test traceability

| ID | Current source | Concrete executed tests/evidence |
| --- | --- | --- |
| A1 credential errors | src/config/loading.rs | src/config/tests/load_tests.rs::config_parse_error_redacts_literal_and_expanded_secrets |
| A2 status/startup/dialog bounds | src/telegram/client/lifecycle.rs; channels.rs; src/mcp/server/impl_status.rs | client/tests/lifecycle_tests.rs::startup_status_times_out, transport_failure_is_not_reported_as_unauthenticated; client/tests/channels_tests.rs::dialog_walk_has_total_deadline; mcp/tests/status.rs::unknown_cached_status_is_free_and_does_not_refresh |
| A3 charging/local validation | src/mcp/server/impl_channels.rs; impl_media.rs; rate_limiter.rs | mcp/tests/channels.rs::invalid_channel_requests_spend_no_tokens; transcription.rs::exhausted_transcription_budget_never_checks_cold_premium, cold_premium_refresh_is_charged_before_rpc; protocol_recovery.rs::task8_duplicate_and_invalid_recovery_keys_are_rejected_before_rpc |
| A4 transcription deadline | src/telegram/transcription.rs; client/ops_transcribe.rs | timeout_shorter_than_poll_interval_does_not_poll; pending_poll_returns_latest_partial_at_deadline; late_completion_does_not_override_deadline; stage_resuming_at_expiry_cannot_start_its_next_rpc; cancellation_stops_scheduling_polls_and_drops_inflight_work |
| A5 retention ownership | src/logging.rs | cleanup_preserves_foreign_logs_and_backups |
| A6 short names/attribution | src/telegram/types/names.rs; converters/channel.rs; converters/message.rs | username_accepts_received_short_names; channel_conversion_preserves_received_short_usernames; convert_raw_message_preserves_received_short_usernames |
| A7 typed contracts | src/mcp/tools/types/contract.rs; canonical.rs; media_contract.rs; server.rs | protocol_contract.rs::canonical_contract_all_sixteen_tools_in_both_modes_and_protocols; protocol_budgets.rs mixed indices and metadata/RPC-ID budgets; schema_integrity.rs |
| A8 protocol dispatch/isolation | src/mcp/runtime.rs; runtime/transport.rs | protocol_stdio.rs::sdk_modern_first_list_has_result_discriminator_and_cache_hints; protocol_matrix.rs::interleaved_protocol_metadata_controls_only_its_own_result; protocol_extensions.rs::request_identity_and_capabilities_do_not_leak_across_interleaved_calls; Python clients |
| A9 shutdown/cancellation | src/mcp/runtime/state.rs; transport.rs; process.rs | blocked_stdout_obeys_the_same_shutdown_deadline; zero_shutdown_budget_cancels_pending_work_without_advancing_time; signal_drains_completed_work_and_idle_shutdown_is_immediate; scripts/test_stdio_process.py |
| D1 constant-time dependency | Cargo.lock; grammers crypto dependency path | ctutils 0.4.7 resolved; fresh RustSec scan and complete source policy |
| D2 I/O-safety dependency | Cargo.toml/Cargo.lock | tokio 1.54.1 normal/dev declarations; fresh RustSec scan |
| M1 media bounds | src/telegram/media_pipeline.rs; mcp/tools/image.rs; client/ops_media.rs | real global-slot saturation, cancelled worker ownership, source pixel limits, source/streamed cap mutations, cached real iterator cases and explicit measurements |
| Storage privacy | src/telegram/session_storage.rs | DB/WAL/SHM private modes; linked-entry and unsafe-ancestor rejection; valid macOS read-ACL removal and granting-directory-ACL rejection |

All named regression tests are present in the completed full run. Some private modules
use #[path] extraction, so Rust module names differ from filenames; the file/name pairs
above identify their checked-in locations. This traceability supplements measured coverage.

## Rust manual and S1–S6 adoption

The [formal adoption assessment](2026-11-01-rust-adoption-acceptance.md) maps every archived
S1–S6 stage and current manual §§1–9 to source evidence, command results and exact scoped
exceptions/retirement conditions. No Critical/Important adoption issue remains. Four
narrow deltas were corrected: seventh-builder useful must_use message, seven-builder
count, constructor-backed domain Deserialize docs, and aggregate media-deadline wording.
Existing-format and stricter policy checks remain separate; no blanket pedantic/restriction
or missing-docs claim is made. The retired domain Deserialize exception stays retired.

## Review and integration boundary

The Task 8 review and scoped media review are preserved in their reports, including
regression fixes. The final combined independent review is pending at this candidate.
Tasks 9/10 are not both closed until that review finishes. Any Important/Critical finding
must receive its own failing regression and fresh green suite before closure.

Release publication still requires a subsequent explicit request. Plans/specs stay in
this unmerged branch until integration, and only completed corresponding files are
archived then. Optional M2/M3 remain unimplemented product proposals. Linux execution is
owner-managed and Windows ACL/runtime behavior is unverified; neither is falsely claimed.
