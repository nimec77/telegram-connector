# Production Media Download Coverage Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline, with one final review.

**Status:** This scoped media-test work package is complete; original Tasks 9 and 10 remain open.

**Goal:** Cover real production media-download decisions without network credentials.
This is scoped work under Task 10 of the original development roadmap; Task 9 acceptance
and complete Task 10 closure remain separate open checklist items.
**Architecture:** Private message snapshot plus a generic chunk-stream collector; cached
TL media exercises the actual grammers download iterator. Preserve existing call contracts.
**Tech Stack:** Rust nightly, Tokio paused time, futures Stream, pinned grammers.
**Spec:** ../specs/2026-11-01-media-download-coverage-design.md

## Global Constraints

- macOS only; no live Telegram or polled sender runner.
- Keep public APIs, schemas, rate charging, image limits and dependency pins unchanged.
- Keep policy code in ops_media.rs so original-file coverage remains interpretable.
- No new dependency, retries, test-only production methods or parent environment mutation.

## Review Focus

1. A cap violation must be rejected before more chunks are consumed or retained.
2. A transfer timeout covers the whole traversal; successful chunks do not reset it.
3. Cancellation/timeout drops the input stream and schedules no subsequent transfer work.
4. Selection uses real variant tags and preserves source/largest metadata and namespaces.
5. The snapshot adapter must preserve original message/caption/error semantics.

## Task 1: Stream policy and deterministic transfer regressions

**Files:** src/telegram/client/ops_media.rs; tests/media_pipeline_tests.rs.
**Interfaces:** private collect_download<S>(chunks, DownloadBounds {max_bytes, timeout_secs}) -> Result<Vec<u8>, Error>.

- [x] Write tests for exact bytes/caps, oversize short-circuit, upstream errors, timeout,
  total traversal budget, and cancellation/drop; run them before extraction.
- [x] Extract the existing loop and route the real DownloadIter through try_unfold.
  Run: cargo test --offline --locked telegram::client::ops_media (all pass).

## Task 2: Real cached media and source metadata

**Files:** src/telegram/client/ops_media.rs; tests/media_pipeline_tests.rs;
new tests/media_download_fixtures.rs if fixture size warrants separation.
**Interfaces:** private VisualMediaMessage {media, caption, peer_id}, From<grammers Message>.

- [x] Write raw TL fixtures and policy tests for photos, video-like thumbnails, missing/
  unsupported media and sizes, reported cap before transfer, identity and metadata.
- [x] Adapt only message extraction through Into<VisualMediaMessage>; run all operation
  tests. Cached fixtures use real downloader paths; remote stalls use an unpolled runner.

## Task 3: Measure and verify

**Files:** docs/memory.md; docs/validation/2026-11-01-media-download-coverage.md; CHANGELOG.md.

- [x] Run cargo llvm-cov --offline --locked --lib --json and report before/after operations
  coverage (baseline 39/246 lines, 15.85%) with remaining live-RPC limitations.
- [x] Run formatting, all-target Clippy, full tests and warning-free docs; inspect diff.
- [x] Obtain one independent read-only whole-branch review; fix important findings with
  failing regressions, then rerun the relevant full gate and commit the finished work.
