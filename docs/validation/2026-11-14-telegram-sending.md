# Telegram sending validation

Implementation evidence is added as each task completes. No live sends are authorized
or performed by these checks.

## Transport feasibility

- The pinned public `SenderPoolFatHandle::thin` can be cloned before moving the fat
  handle into `Client::new`. `SenderPoolHandle::raw_invoke_in_dc` accepts serialized
  requests and returns bytes without entering `do_invoke_in_dc`'s `AutoSleep` loop.
- An unpolled retained sender runner proves the real raw API compiles and can be
  cancelled without polling the network runner. Injected final responses exercise
  the connector's actual correlation, deadline and refusal classifier.
- Source inspection of pinned `client/files.rs`: `upload_stream` consumes Tokio
  `AsyncRead`, uses 512 KiB parts and four workers for files larger than 10 MiB.
  This is not a measured account/network buffering result or a hard RSS guarantee.
- The raw one-shot seam does not regenerate requests or random IDs. Lower-level
  MTProto retransmission and Telegram's deduplication retention remain unverified
  by these offline fixtures.

## Recipient and thread validation

- Qualified dialog selection includes users and distinguishes colliding user/chat/channel IDs. The real stalled client proves resolve/topic stage caps and the shared deadline.
- Deleted/migrated peers, missing authority, inaccessible broadcast metadata and incomplete/deleted/closed/hidden or wrong-peer topic states are rejected before subsequent stages.
- [Telegram forum semantics](https://core.telegram.org/api/forum) identify General as topic 1 without an ordinary message root. Non-General replies retain the selected top thread; direct topic-root sends use the root itself. Offline TL fixtures check both forms and cross-topic/cross-peer refusal.
- Community containers and channel direct-message monoforums are refused; the initial namespace/options contract does not supply their separate routing identities. No live topic placement was tested.

## Canonical text route

- Actual framed modern and legacy-handshake requests send through the canonical contract. Disabled catalogs retain 16 tools; enabled catalogs expose 18 with strict self-contained write input schemas and non-idempotent annotations.
- Offline tests prove one charge before the client seam, no refunds, no charge for local/budget refusal, matching JSON text/structured receipts, scoped upstream diagnostic suppression and cached path-free status.
- Forced serializer failure and invalid receipt metadata produce nonretryable unknown delivery; duplicate recovery keys reject before a second send. Full macOS locked offline gate passed after integration. Media remains a pre-dispatch unavailable stub until the staging/upload tasks land.
