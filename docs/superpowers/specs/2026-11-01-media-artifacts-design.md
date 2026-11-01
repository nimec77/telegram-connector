# Optional media artifacts and local file export

## Intent and client evidence

Implement the authorized M2/M3 proposal for DeepSeek Harness Desktop while retaining
inline previews and the existing sixteen tools. Cache processed JPEG previews so a
client can retrieve bytes again or read a local copy without another Telegram RPC.
This does not add full original attachment, video, audio or document downloading.

The user selected DeepSeek Harness Desktop. Installed 0.2.1-rc.5, build
`d4db512a969fccf0c5cc7ec8fe38e9c1d27af916`, supports custom MCP resource URI reads.
Its resource renderer substitutes binary blobs with text; resource links are text
references, not automatic vision. Its actual `read_image({file_path})` tool accepts
absolute JPEG paths outside the working directory and emits native image attachments
when the routed model declares image input and the attachment service is mounted.
An isolated bundled-adapter smoke verifies these behaviors without user configuration,
Telegram or a model request. Actual configured model/UI behavior remains unverified.

## Approaches and decisions

- Resource references alone: portable deferred retrieval, but insufficient for native
  image inspection in this installed DeepSeek resource renderer.
- File exports alone: useful to DeepSeek's image reader, but ties retrieval to a shared
  host filesystem and omits portable programmatic resource access.
- **Implementer's choice, within the user's authorization:** one private bounded store
  supporting both optional modes. Always preserve inline previews. Default behavior
  stays inline, with no filesystem initialization when artifacts are disabled.

AGENTS.md authorizes execution without routine design/plan approval pauses. Work on
`feat/media-artifact-delivery`, based on the verified local documentation commit
`3e9b7ab`. Preserve both earlier documentation branches. The native worktree inventory
points to an already-deleted attachment; use the clean primary checkout on this
feature branch. All related implementers share that branch with explicit file ownership.

## Configuration and request contract

Add `[media_artifacts]`: `enabled = false`, `file_export_enabled = false`, optional
absolute `root`, `ttl_seconds = 600`, `max_entries = 100`,
`max_total_bytes = 268435456`. If enabled without root, use the platform configuration
directory's `media-artifacts` child. Validate TTL 1..=3600, entries 1..=1000, bytes
1..=1073741824, and a nonempty absolute root when provided. File export requires enabled.
Artifact functionality requires Unix ownership/mode checks; refuse enabling it on
unverified non-Unix platforms while preserving their default inline behavior.

Add defaulted, schema-inline `MediaDelivery` to both media requests: `inline`,
`resource`, `file`, `resource_and_file`. All modes retain an inline image. Non-inline
delivery requires enabled configuration; file modes additionally require file export.
Reject unavailable modes, and all non-inline legacy requests, before token acquisition
or Telegram calls. Legacy mode otherwise retains its historical payloads.

Canonical successful items gain an omitted-by-default `artifact` descriptor. It records
processed-JPEG kind, MIME type, byte size, dimensions, UTC expiry, optional resource URI
and resource block index, and optional absolute raw filesystem path. Expose paths only
for explicit file modes. Resource modes append a `resource_link` after that item's image;
calculate all image/resource content indices from actual block positions. Resource/file
metadata does not identify the output as an original Telegram attachment. Preserve
per-item errors, image budgets, complete non-binary overhead sizing and refund semantics.

## Private bounded store

Use a dedicated private root, a create-new process lock and generated filenames.
The root must be empty at initialization; refuse nonempty roots without deleting their
contents. A second process using the same root fails closed. Normal last-handle drop
removes only known owned artifacts/staging files and the lock, never recursively removes
unknown files. An abrupt crash can leave bounded data/lock; subsequent startup refuses
that root and documents explicit cleanup or choosing a fresh private root. No persistent
manifest/recovery or cross-process cache reuse is promised in this version.

Create directories 0700 and files 0600. Verify canonical ancestry's current/root ownership,
exclude unprotected group/other-writable ancestors, permit root-owned sticky ancestors
only above the private root, and reject granting macOS directory ACLs. Refuse symlinks,
hardlinks and nonregular entries on reads/cleanup. Do not widen existing permissions or
delete foreign entries. Same-UID hostile mutation is outside the established boundary.
Trusted OS ownership/ACL utilities have captured output and two-second timeouts.

Generate 192-bit opaque IDs using OS randomness. A direct `getrandom 0.4.3` declaration
is justified because std has no safe cross-platform CSPRNG; this identity is already in
Cargo.lock. No new resolved packages or unrelated dependency updates are needed.
Canonical URIs are exactly `telegram-media://artifact/<48 lowercase hex characters>`;
reject alternate casing, extra paths, percent escapes, query strings and fragments.

Serialize store mutation under a poison-refusing synchronous mutex inside bounded
blocking work. Insert only complete JPEGs (max 20 MiB), reserve byte/entry capacity,
evict oldest entries until the new one fits, write create-new staging files and atomically
rename before publishing. A too-large item cannot evict healthy entries. Deletion failure
must not free quota or permit unbounded orphan accumulation; refuse further mutations
after an unrecoverable cleanup/write failure. Reads use strict exact URI/manifest lookup,
validate current regular single-link ownership/size and bound allocations before reading.

TTL is monotonic and never refreshed by reads. Reject expired entries immediately.
Run periodic cleanup at most every 60 seconds while the store exists, plus cleanup on
operations; explain that physical expiry deletion can lag by that interval and worker
admission. Filesystem paths are not revocable handles; logical URI expiry does not
guarantee immediate physical deletion or deletion of client attachment copies. Keep the
cleanup task weakly owned so it cannot keep the store alive. Quotas include pending and
failed-cleanup reservations. Cache links are sensitive handles, not multi-user authentication.

### Store interface for independent implementation

Create private `src/mcp/artifacts.rs` and focused sibling helpers/tests as needed:

```rust
StoreConfig { root: PathBuf, ttl: Duration, max_entries: usize, max_bytes: u64 }
StoredArtifact { uri: String, path: PathBuf, size_bytes: usize,
                 width: u32, height: u32, expires_at: chrono::DateTime<chrono::Utc> }
ArtifactRead { bytes: Vec<u8>, expires_at: tokio::time::Instant }
StoreSnapshot { entries: usize, bytes: u64 }
enum StoreError { InvalidUri, Missing, TooLarge, Unavailable }
ArtifactStore::try_new(StoreConfig) -> async Result<Arc<ArtifactStore>, StoreError>
ArtifactStore::insert(&self, bytes: &[u8], width: u32, height: u32)
    -> Result<StoredArtifact, StoreError>
ArtifactStore::read(&self, uri: &str, max_bytes: usize) -> Result<ArtifactRead, StoreError>
ArtifactStore::remove(&self, uri: &str) -> Result<(), StoreError>
ArtifactStore::cleanup(&self) -> Result<(), StoreError>
ArtifactStore::snapshot(&self) -> Result<StoreSnapshot, StoreError>
ArtifactStore::close(&self) -> Result<(), StoreError>
```

Types and fields are `pub(crate)` for integration, not public Rust API. Except async
initialization/periodic scheduling, methods are synchronous and must be called within
the existing bounded `runtime::image_job` mechanism. Shared clones bind entries to the
same server/client instance; newly initialized servers cannot reuse prior URIs. The
store module knows no Telegram credentials and exposes no arbitrary filesystem API.
`close` is idempotent, removes only owned files/lock, and makes all live clones refuse
further insertion/reads. It runs in bounded blocking work, not directly on the executor.

## Server and resource integration

Add an async fallible consuming server builder and invoke it from production startup.
Store optional shared Arc state; default constructor remains inert. Status exposes
enabled/file-export flags, quota/TTL/read limits and a fallible reserved usage snapshot,
without paths or artifact enumeration. Preserve unavailable-state meaning.

Cache only successfully processed JPEG bytes, decoded from the already bounded preview
inside an existing blocking-work slot. This avoids new public ProcessedImage fields or
retained raw download buffers. Keep worker ownership through cancellation. The original
media deadline includes admission, decoding and cache writes. Canceled started writes
may finish as bounded unreferenced entries, reclaimed by TTL/quota or shutdown; do not
claim a started blocking write was canceled. Cache failure is a safe typed per-item tool
error; preserve token refunds for items with no delivered image. If final metadata exceeds
the existing 65,536-byte cap, fail small and refund as before; artifacts remain bounded.

Advertise Resources only when the store is enabled. Resources/list and templates/list
return empty lists; no subscriptions or listChanged. `resources/read` uses strict lookup
and no Telegram RPC/account charge, retaining runtime admission/cancellation. Limit JPEG
reads to 6 MiB so base64 is at most 8 MiB; reject larger stored files before allocation.
Use a five-second cache-only read deadline starting before blocking-worker admission;
a started worker retains its slot until completion even after timeout/cancellation.
Safe malformed-URI, missing/expired, oversized and storage errors remain distinct protocol
errors. Modern read/list cache hints are private and no longer than remaining TTL;
omit version-specific fields for legacy clients. Resource bytes remain base64.
Check both the read deadline and artifact expiry after worker completion, so a ready
result observed at or beyond expiry cannot override either boundary.
Diagnostic replay with `include_binary = false` must also stub `resources/read`
`contents[].blob` and embedded binary resource payloads, preserving safe MIME/size
metadata; explicit `include_binary = true` may retain them under existing replay limits.

Production and fixture `run_io` must explicitly await store closure on ordinary EOF or
signal completion. Retain a crate-private handle to the protocol runtime's first absolute
drain deadline across `serve`; closure gets only that remaining time, never a fresh grace
period. Preserve zero-grace immediate exit. If the original deadline is exhausted, an I/O
failure prevents coordination, or the process crashes, bounded stale-root fallback is
documented; Drop's asynchronous finalizer is best effort during immediate Tokio teardown.

## Verification and delivery

Use TDD for production behavior: store isolation/quota/TTL/permissions/path rejection,
single and mixed batch delivery/indexing/refunds/deadlines, schema self-containment,
default/legacy compatibility, both protocol revisions, and resource error/read bounds.
Mutation checks should exercise TTL, strict lookup and a quota boundary. Extend the
independent Python process client against the actual server fixture. Use actual exported
JPEG paths in the installed DeepSeek offline image-reader smoke; verify resource retrieval
and its placeholder projection separately. Do not call a model, live Telegram or change
the user's client configuration. Run the full offline macOS quality gate, release build,
dependency policy and an independent whole-branch security/code review. Update docs,
examples, changelog, Rust API inventory and durable memory with exact limits and evidence.
No package bump, push or release publication is included in this feature task.
