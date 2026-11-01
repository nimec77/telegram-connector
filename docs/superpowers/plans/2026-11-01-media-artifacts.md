# Media Artifacts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Execute continuously on the feature branch; repository authorization replaces routine approval checkpoints.

**Goal:** Add opt-in cached media resources and private temporary file export for DeepSeek Harness Desktop while retaining inline previews.

**Architecture:** One private, bounded, process-owned store backs both modes. Existing media tools add explicit delivery selection and optional descriptors; resource handlers retrieve cached processed JPEGs without Telegram calls.

**Tech Stack:** Pinned Rust nightly, rmcp 3.8, existing image/base64/Tokio workers, already-locked getrandom 0.4.3, Python and installed DeepSeek adapters.

**Spec:** `docs/superpowers/specs/2026-11-01-media-artifacts-design.md`

## Global Constraints

- Default remains inline, with no artifact filesystem initialization when disabled.
- Always retain inline previews; preserve default/legacy payloads and existing budget/refund/deadline semantics.
- No live Telegram, paid model calls, user/client configuration changes, Linux provisioning or new package versions.
- Only add getrandom 0.4.3 as a justified direct dependency; preserve the resolved graph and all grammers pins.
- Protect private roots/paths; no arbitrary-path tool, recursive foreign cleanup, public hosting or raw binary stdout.
- Preserve previous documentation branches and historical validation reports; new evidence belongs in a new report.
- Work from verified base 3e9b7ab on feat/media-artifact-delivery in the primary checkout.

## Review Focus

- Root/lock/stale-crash, symlink/hardlink/ACL checks and failure paths cannot erase foreign files or release nonexistent disk quota.
- URI decoding/traversal, cross-store identifiers, expired reads and oversize reads cannot expose bytes outside the manifest.
- Mixed successful/failed batches retain exact content indices, metadata overhead accounting, byte budgets and refunds.
- Disabled, legacy and unavailable file modes reject locally before account acquisition and RPC.
- Started blocking work retains capacity through cancellation; cleanup does not keep the store alive or promise immediate disk deletion/model rendering.

### Task 1: Private bounded artifact store

**Files:** Cargo.toml, Cargo.lock, src/mcp.rs; create src/mcp/artifacts.rs and focused src/mcp/artifacts/ helpers/tests.

**Interfaces:** Implement the exact StoreConfig/StoredArtifact/ArtifactRead/StoreSnapshot/StoreError/ArtifactStore interface in the spec. Store module is crate-private. Byte insertion receives an internally verified processed JPEG; it need not decode it a second time. Keep all filesystem/cleanup work in bounded blocking jobs; async initialization alone runs trusted deadline-bounded OS utilities.

- [ ] Add failing behavior tests for insert/read byte equality, dimensions/expiry, 192-bit generated IDs, strict URI rejection and cross-store isolation; observe failure before implementing storage.
- [ ] Implement private empty-root verification, create-new ownership lock, atomic complete writes and safe manifest lookup using OS randomness. Promote only the already-locked getrandom identity into direct dependencies.
- [ ] Add and observe failing tests for exact byte/entry caps, oldest eviction, an oversize insert preserving healthy entries, poisoned state, missing files and symlink/hardlink/foreign-entry refusal; implement safe accounting/failure behavior.
- [ ] Add paused-time tests proving TTL never extends on reads, expired reads fail immediately and periodic cleanup removes owned files; test normal drop removes only known files and a stale/nonempty root refuses initialization without deleting data.
- [ ] Check macOS 0700/0600 and granting ACL rejection, shared sticky root refusal/private child acceptance, and invalid StoreConfig bounds. Exercise TTL/strict-lookup/quota guard mutations individually, restore source after each, and record failing assertions.
- [ ] Run cargo fmt, strict all-target Clippy, cargo test and warning-free rustdoc offline/locked; inspect lock diff for only root getrandom reference, then commit focused store implementation.

### Task 2: Media delivery, configuration and MCP resources

**Files:** src/config.rs and src/config/*; src/error.rs; src/main.rs; src/mcp/server.rs and server/impl_media.rs, impl_status.rs, impl_legacy.rs; create server/impl_resources.rs as needed; src/mcp/tools/types/{requests,responses,media_contract,contract}.rs, affected reexports; src/mcp/tests/*, src/config/tests/* and necessary literal fixture updates.

**Interfaces:** Consume Task 1's crate-private store API. Produce MediaArtifactsConfig, schema-inline MediaDelivery, omitted-by-default artifact metadata, async fallible server builder, resource handlers, production startup wiring and optional status metadata. Use the spec's exact modes, defaults, validation ranges and resource read limits.

- [ ] Add failing configuration/default and request-schema tests, including all unsupported delivery cases rejecting before mock acquisition/RPC, then implement configuration/builder/production startup and safe artifact errors.
- [ ] Add failing single/mixed batch tests proving unchanged default output, opt-in resource/file/both fields, raw path only for file delivery, processed-JPEG byte equality, actual resource/image content indices, per-item failures, quota eviction and refunds. Implement decoration inside existing deadline-bounded blocking workers.
- [ ] Add failing wire tests across modern/legacy protocol: resource capability only when enabled, empty listing/templates, safe malformed/missing/expired/too-large/storage errors, <=6 MiB read allocations/8 MiB base64, private remaining-TTL cache hints only in modern responses. Implement handlers outside the tool_router methods while keeping all tool entry points in its single impl.
- [ ] Verify metadata-overflow refunds and schema self-containment with artifact metadata; preserve direct Rust legacy adapters and reject non-inline direct adapter requests before acquisition. Update only request literals that require the new default field; do not weaken old assertions.
- [ ] Add paused deadline/cancellation cases for artifact work and state the started-worker retention boundary accurately. Confirm default constructor performs no filesystem/RPC work and normal clones share the store.
- [ ] Run targeted tests, then full offline fmt/Clippy/tests/rustdoc; inspect diff and commit the integrated feature.

### Task 3: Independent clients, DeepSeek proof and current documentation

**Files:** examples/offline_mcp_fixture.rs; scripts/test_media_clients.py and a focused new artifact-client script if appropriate; README.md, CHANGELOG.md, config.example.toml, docs/tool-contract.md, docs/codex.md or new docs/deepseek.md, docs/rust-api.md, docs/dependency-status.md, docs/memory.md, active media proposal; create docs/validation/2026-11-01-media-artifacts.md.

**Interfaces:** Consume actual feature configuration and wire contracts. Extend existing mock-backed process fixture to opt into a disposable artifact root without credentials. Installed adapter probe remains in /private/tmp/deepseek-inspect; do not commit third-party bundled code or machine-specific paths as portable test dependencies.

- [ ] Add failing independent Python fixture checks for every delivery mode, resource byte equality, exported file bytes, mixed item indices/errors, expiry and modern/legacy cache metadata. Implement fixture opt-in and client checks while keeping default fixture behavior unchanged.
- [ ] Run actual connector fixture outputs through the installed DeepSeek adapter. Verify M2 canonical bytes plus placeholder projection and M3 read_image({file_path}) durable native attachment; assert text-only-route refusal. Record installed build/revision and distinguish adapter proof from real model/UI execution.
- [ ] Document setup, modes, private-root/stale-lock ownership, TTL/quota/cleanup, no silent redownloads, processed-preview scope, per-process limits, client attachment persistence and read_image prerequisites. Update Rust API compatibility and the new direct dependency disposition; retain historical acceptance records.
- [ ] Run full offline macOS quality gate, production release and --version, independent existing/new wire/process clients, dependency policy fixtures and complete policy check; verify metadata/grammers pins and local docs/TOML links.
- [ ] Obtain independent task review and final whole-branch security/code review, address real findings with regression evidence, commit and report exact results/limits. Keep feature branch local; no version bump/push/publication is included.

## Design and preflight evidence

- Builder check: nothing important left.
- Installed DeepSeek smoke rerun by root: PASS for custom resource URI/canonical bytes,
  resource placeholder, outside-workspace file image attachment and text-only refusal.
- Task 1 -> Task 2: store API/types are pinned in the spec; no shared file ownership.
- Task 2 -> Task 3: fixture/tests consume actual serialized schemas/config, not guessed output.
- Task 1 may add only the direct already-locked RNG dependency; later tasks must not alter the graph.
