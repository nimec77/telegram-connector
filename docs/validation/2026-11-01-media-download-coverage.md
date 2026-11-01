# Scoped Task 10 media-download coverage — 2026-11-01

**Subsequent closure:** Original Tasks 9/10 were still open at this scoped record's
boundary. They are now accepted on the final branch with separate
[final evidence](2026-11-01-final-acceptance.md); historical figures below are preserved.

This is one work package under the original roadmap's Task 10. Task 9 acceptance and
complete Task 10 closure remain open. The prior claim that the entire roadmap was complete
was incorrect; the original roadmap/audit and accurate status have been restored.

## Scope and implementation

A private VisualMediaMessage snapshot converts the upstream Message's media/caption/peer
values without exposing PeerMap or changing public APIs. Raw TL cached photo/thumbnail
fixtures exercise the actual production operation and grammers download iterator without
polling the sender runner. A private generic chunk-stream collector retains the original
size checks and one transfer timeout, allowing deterministic error/cancellation tests.
Iterator construction is lazy, inside that timeout. No dependency/schema/version change.

The fixtures exposed an actual classifier bug: a Video attribute before Animated caused
an animated MP4 to be reported as Video. Telegram specifies the Animated marker for GIF
animations ([primary documentation](https://core.telegram.org/api/gifs)). The marker now
wins independently of attribute ordering, preserving duration/dimensions and thumbnail
handling. A pure converter regression checks both orders.

## Verification

- Baseline: 926 library tests + 3 doctests. Final gate: 940 library tests +
  3 doctests, formatting, all-target Clippy with warnings denied, warning-free rustdoc.
- Fourteen new tests: six transfer cases, seven source/selection cases, one converter case.
- Explicit mutations separately disabled the streamed cap, reported cap and animation
  marker guard. Each relevant test failed behaviorally, and original code was restored.
- Healthy cases use real cached photos, video, round-video and animation thumbnails. Tests
  cover exact bytes, source/largest metadata, captions, channel/chat identity, absent media,
  unavailable variants, nonvisual documents and unsupported user peers.
- Transfer tests cover exact cap, underreported streamed oversize, stopping after failure,
  one total deadline despite progress, and dropping work on timeout/cancellation. A real
  remote iterator with an unpolled sender verifies the download stage timeout with no RPC
  transport running. No credentials or live Telegram account were used.

## Coverage

Matching nightly LLVM 23.1.4, same `cargo llvm-cov --offline --locked --lib --json` command:

| Area | Before | After |
| --- | ---: | ---: |
| Original ops_media.rs | 39/246 lines, 15.85% | 143/277 lines, 51.62% |
| Library, unfiltered | 9,176/11,230, 81.71% | 9,301/11,263, 82.58% |

Policy remains in the original operations file. The increase is executable transfer,
selection and assembly coverage, rather than file relocation. Unfiltered totals can
include inline test code. Thin live resolution/fetch adapters, upstream Message extraction
and some defensive/unreachable branches remain unexercised; this is not live account or
complete network-path coverage. Those limitations remain part of Task 10 acceptance.

## Status and decisions

The restoration distinguishes implementation from acceptance: Tasks 1–8 are integrated in
0.24.0; Task 9 automation exists but its acceptance checklist is open; Task 10 has verified
subsets and remains open. Optional M2/M3 are separate product decisions.

One scoped deviation was necessary: fixing the demonstrated animation classification bug
exposed by the new fixture. Callers previously seeing Video for those GIFs now receive
Animation; payload shape and public interfaces are unchanged. No version bump, merge or
push is included in this work package. One independent whole-branch review found no Critical or Important findings. Planned
memory finalization and a historical Task 7 wording correction resolved its documentation
notes. This report does not close the overall roadmap.

## Review boundaries

Live resolution/fetch, DC migration and actual transport cancellation remain outside the
offline fixtures. The thin upstream Message accessor mapping was inspected rather than
constructed through private PeerMap state. Contradictory raw attributes with no established
Telegram semantics, deliberate OOM injection and malformed upstream peer internals were
not claimed tested. Existing defensive behavior is retained. Full Task 9/10 acceptance,
fresh security/dependency re-audits, host rendering and Linux/Windows validation are not
closed by this review. These limits do not weaken the specific cached-download, byte-cap,
error, deadline and cancellation evidence above. No blocking or deferred code findings.
