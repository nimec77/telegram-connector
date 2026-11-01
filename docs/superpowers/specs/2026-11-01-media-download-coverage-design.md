# Production media-download coverage

Goal: exercise real media-selection, byte-stream, timeout and metadata decisions offline,
closing the documented ops_media coverage gap without changing public MCP/Rust contracts.

## Approach

Use a private snapshot of the three message inputs consumed here (media, caption, peer ID)
with an infallible conversion from the upstream Message. The production operation accepts
that snapshot through Into, so tests can supply valid raw TL media without constructing
upstream private PeerMap state. Cached photo/thumbnail fixtures use the real grammers
downloader without polling the sender runner. Extract the existing chunk loop into a
private generic Stream collector for split chunks, errors, stalled input and cancellation.

A new downloader trait would add a second abstraction where a generic stream suffices.
The whole-client mock remains appropriate for MCP tests but cannot exercise this body.
The chosen seam stays private, uses existing dependencies and retains operation names,
error categories, transfer deadlines, reported/streamed caps and returned metadata.

## Acceptance

- Byte-exact concatenation, exact cap, reported/streamed oversize, errors without retry,
  total transfer deadline, and dropping stalled work on timeout/cancellation.
- Photo and video-like cached thumbnail selection, no visual media, missing candidates,
  source/largest dimensions and sizes, captions, peer namespace and unsupported user peers.
- Meaningful before/after coverage for the original operations file; do not raise a metric
  by moving uncovered work elsewhere. Thin live RPC adapters remain explicitly identified.
- Full macOS quality gate and one independent final review. No real Telegram account,
  credentials, polling sender runner, dependency upgrade, version bump, merge or push.

The user selected this workstream. Routine design/plan approval pauses are superseded by
AGENTS.md; execute inline and retain evidence. Optional M2/M3 are outside this change.
