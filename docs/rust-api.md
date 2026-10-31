# Rust API surface and compatibility policy

The library and MCP wire protocol are separate contracts. This inventory covers the
current exported Rust surface. Prefer supported facades for new integrations; transitional
paths remain callable and compatibility-sensitive. “Internal” below means already private
or crate-visible, not public APIs silently withdrawn by this policy. There are no visibility
removals, renamed constructors, sealed traits, or new `non_exhaustive` restrictions in S3.
Public DTO field literals remain possible. Future removals, added enum variants or required
fields/type changes need an explicit Rust compatibility note even if JSON stays identical.

## Supported facade and actual consumers

`src/main.rs` is a separate Cargo crate, so its imports require public library visibility.
It consumes root `Cli`, `Config`, `RateLimiter`, `TelegramClientTrait`, and `logging`;
`mcp::server::McpServer` construction, builders, metrics and serving;
`telegram::client::TelegramClient` construction/session path; and
`telegram::auth::interactive_auth`. Authentication also needs concrete client login APIs.
The repository previously documented no additional external Rust consumer imports; that
is not evidence that downstream users do not exist. Every existing public path is retained.

| Supported surface | Scope and preferred imports |
| --- | --- |
| Crate root | `Cli`, `Config`, `ObservabilityConfig`, `ServerConfig`, `Error`, `MessageLink`, `RateLimiter`, `RateLimiterTrait`, `TelegramClient`, `TelegramClientTrait` are canonical conveniences. |
| `config` | `Config`, `TelegramConfig`, `TimeoutConfig`, `SearchConfig`, `RateLimitConfig`, `TranscriptionConfig`, `LimitsConfig`, `LoggingConfig`, `ObservabilityConfig`, `ServerConfig`; fields and validation/loading methods remain public. |
| `mcp::McpServer` and `mcp::server::McpServer` | Inert construction, six consuming `with_*` builders, metrics/replay handles, stdio/injected I/O and existing tool methods. Direct tools retain wire-facing wrappers and string errors/content blocks. |
| `telegram::{TelegramClient, TelegramClientTrait}` | Production client and intentionally substitutable, unsealed client seam. Concrete `new`, `client`, `session`, `session_path`, `request_login_code`, `check_password` stay available. |
| `telegram::types` | All domain facade exports: IDs/names, account/channel/message/album/link-preview/reaction entities, media metadata and outcomes, query/history parameters/results, stats and transcription data. This includes `MediaDownload`, metadata types and `SizeCandidate` even when not reexported directly by `telegram`. |
| `rate_limiter` | Production limiter and unsealed limiter trait, including the defaulted fallible snapshot extension for existing implementers. |
| `error` | `Error` and `StateUnavailable`; typed library failures, with fixed safe component identities for interrupted state. |
| `link` | `ChannelRef`, `parse_telegram_link`, `MessageLink`; pure parsing/building, without resolution or URL launching. |
| `cli`, `logging`, `telegram::auth` | CLI argument data, process-global tracing setup/retention/redaction, interactive terminal setup. These have application side effects described below. |
| `mcp::runtime` / `runtime::process` | `Runtime`, `RuntimeError`, `SUPPORTED_VERSIONS`, process `run` and `shutdown_signal`; configurable protocol transport/lifecycle support, subject to the bounded product bootstrap contract. |
| `mcp::observability` | `SessionMetrics`, `InFlightRequest`, `WriteObservation`, `ResponseBuffer`, `ResponseSnapshot`, `BufferedResponse`, `InstrumentedTransport`, `GET_LAST_RESPONSES_TOOL`, `OVERSIZED_PAYLOAD_STUB`; shared diagnostic handles with fallible storage access. |

## Transitional compatibility paths

| Retained path | Reason and constraints |
| --- | --- |
| `telegram::client`, `telegram::trait_def` | Original client/trait paths, including binary imports. Prefer facade imports for new callers. The mock generated under `cfg(test)` is not an external library API. |
| `telegram::types::{account,entities,ids,media,names,params,stats,transcription}` | Existing nested domain paths, including pure `stats::compute_stats`. Prefer the `types` facade where it exposes the item; nested paths are retained. |
| `mcp::tools` and `tools::types::{requests,responses}` | All request/response exports and convenience reexports remain. The deliberate `pub use types::*` is a facade exception, not a general production-import prelude. Schema and wire coercion belong at this boundary. |
| `mcp::tools::types::serde_helpers` | `flexible_opt_enum`, `flexible_opt_int`, `flexible_i64`, `flexible_string`, `flexible_opt_string`, `flexible_opt_bool`; serde coercion failures remain boundary errors, not strict domain construction. |
| `mcp::tools::helpers` and helper reexports | `parse_channel_id`, `parse_message_id`, `dedupe_and_validate_ids`, `json_response`; pure helpers retain string errors. Dedupe preserves first-seen order, then checks cap, positive sign and message wire range. Empty input is left for tool validation. JSON serialization can fail for arbitrary caller serializers. |
| `mcp::tools::image` | `MAX_BASE64_LEN`, `ProcessedImage`, `process_image`; synchronous CPU pipeline, no RPC/charge. Never upscales; fitting JPEG passes through unchanged, other inputs become JPEG and bounded shrink attempts enforce the base64 cap. Decode/encode failure is `DownloadFailed`; exhausted shrinking is `PayloadCapExceeded`. Async consumers own bounded blocking offload. |
| `telegram::converters` | `convert_peer_to_channel`, `convert_discovered_peer`, `convert_media_filter`, `convert_media_to_type`, `extract_audio_duration`, `extract_audio_info`, `extract_document_info`, `extract_poll_info`, `extract_video_info`, `select_size_candidate`, `size_candidates`. Pure helpers are coupled to pinned grammers/TL types; absence/unsupported metadata uses `Option` where specified. |
| `telegram::transcription::TranscriptionDeadline` | Required in the public client trait signature. `new` starts one absolute Tokio budget after validation/acquisition; copies do not renew it. Zero expires immediately before any production stage; `instant` is absolute and `timeout_secs` retains the requested diagnostic budget. Internal polling/guards stay private. |
| `telegram::timeout` | Retained empty public module. `with_timeout` remains crate-private; no general timeout helper is newly published. |

Already internal: config defaults/environment expansion, MCP fanout/media-budget/shaping,
Telegram albums/envelope, client operations/raw pagers/guards/search-budget/walk/resolve
helpers, runtime state/transport/processing and converter implementation modules. No field
visibility is widened for later module moves. Test-only helpers and mocks are not supported
external APIs.

## Errors, deadlines and partial results

The production client returns typed `Error`; the MCP boundary preserves established strings
and tool content. Configuration/application setup keep their contextual `anyhow` edge.
String payloads in `Error` are not a universal redaction mechanism; callers must exclude
secrets. Authentication discards arbitrary upstream payloads. No implicit retries are added.
Custom trait implementations own their own timeout/storage behavior: the generic server
does not impose production client RPC budgets on an arbitrary replacement implementation.

| Operation | Production bounds and failure/partial contract |
| --- | --- |
| Channel resolution/basic identity | Resolve/dialog stage budgets; invalid/missing references, RPC errors and stage timeouts fail. No join or membership mutation. |
| Full channel info | Basic resolution failures fail. Optional full-info (resolve budget) and newest-message peek (history budget) degrade to basic info on errors/timeouts. Non-channel peers skip channel-specific full-info. |
| Subscribed channels | One history budget for the entire dialog walk, even for small pages. `total` is the full count; failure discards the page rather than inventing a partial total. Last-message dates come from dialog envelopes without extra RPCs. |
| Public directory search | Search budget; empty query, timeout or API failure errors. `is_subscribed: false` is best effort because Telegram caps own-dialog matches. |
| Message search | Outer search timeout plus cooperative accumulation deadline. Cooperative expiry returns gathered messages with `timed_out/partial`; outer timeout or API failure errors. A deadline does not prove `has_more`. |
| History / specific message / message batch | Separate bounded resolution and history fetch stages. History has no cooperative search deadline. Deleted, absent or unrepresentable single messages error; batch slots go to `missing_ids`, preserving request order. Whole RPC/timeout failure errors. |
| Visual media | Bounded resolution/history stages and per-download timeout; reported and streamed byte caps both apply. Single failures error. Batch shared resolve/fetch failure errors; each requested ID otherwise gets an ordered outcome, including timeout, missing-media and oversize failures. No aggregate download-batch deadline is promised. |
| Transcription | One explicit absolute deadline across resolution, validation, initial RPC and polling, plus internal stage bounds. Before any state, expiry errors; afterwards expiry or transient poll failure returns latest partial text. Telegram quota/Premium/length failures remain typed; transcription may consume upstream quota. |
| Batch channel resolution | One bounded dialog walk, then separately bounded unmatched username RPCs. Shared walk failure errors; missing, ambiguous and per-username RPC failures stay in ordered entries. |
| Stats | Bounded resolution/history sweep; cap returns `sample.complete = false`, timeout/API failure errors. Caller clamps days, and zero days is invalid. |
| Account refresh | Authorization and Premium use separate resolve budgets. Authorization failure preserves cached observations and errors. Premium failure retains the last observation and still returns status. Cached status is local and free. |

Pagination uses channel-scoped exclusive bounds (`after_id < id < before_id`); a message ID
is not globally unique. Global search rejects cursors. Multi-channel MCP calls preserve
per-channel cursors/errors. Message count/byte truncation proves a continuation only when
a qualifying message is known beyond the retained page; deadline partial metadata is a
separate signal. Albums describe only siblings present in the result window, not a claim
of whole-album completeness.

MCP input validation precedes account acquisition, and every RPC follows acquisition.
Protocol in-flight admission is independent and rejects overload before tool dispatch.
Fan-out charges every deduplicated channel attempted and never refunds failed channels.
Media batches refund every unproduced image after a successful shared fetch, potentially
the entire charge. A whole shared resolution/fetch failure instead retains at most one
token of the original charge; this is distinct from fan-out. The limiter trait's refund
is best effort and cannot reopen unavailable state. Direct client calls bypass account charging. MCP
message output and media base64 budgets remain distinct from input frame and download caps.

## Unavailable state and legacy information loss

Poisoned protected contents are never recovered/read. Atomic counters and independent
healthy locks remain useful. Prefer fallible/explicit observations for new consumers:

| API | Preferred explicit result | Legacy fallback retained |
| --- | --- | --- |
| `RateLimiter` / `RateLimiterTrait` | `try_available_tokens -> Result<f64, StateUnavailable>` | `available_tokens` returns conservative `0.0`, conflating exhaustion and unavailable. Trait default wraps the old getter; fallible external stores must override it. Production acquire fails closed, including zero cost. |
| `SessionMetrics` recording | `try_record_request`; `observe_response_written -> WriteObservation` with independent request/timestamp results | `record_request` discards correlation failure but advances the atomic count. `record_response_written` omits unavailable identity, conflating it with ordinary unmatched response. |
| `SessionMetrics` snapshots | `try_last_write_age_secs -> Result<Option<u64>, _>`; `try_abandoned_requests -> Result<Vec<_>, _>` | `last_write_age_secs` returns `None` on unavailable, conflating no writes with failure. `abandoned_requests` returns empty on unavailable. |
| `ResponseBuffer` | `try_push`, `try_snapshot -> ResponseSnapshot`, `try_len` | `push` refuses unavailable storage without a result; `last` returns empty, `len` zero, `is_empty` true. Those omit untrusted replay and conflate unavailable with empty. |

`WriteObservation.request` is `Ok(None)` for ordinary unmatched responses and `Err` for
unavailable correlation. Timestamp failure does not prevent independent correlation, or
vice versa. `ResponseSnapshot.responses` and `buffered` come from one lock acquisition.
Disabled replay is successfully empty; poisoned enabled replay returns the fixed typed
unavailable identity, including from the replay tool. Recovery excludes its own responses,
stubs oversized bodies, and omits binary blocks unless explicitly requested.

The S2 Rust compatibility changes remain intentional: `Error::Unavailable` is a new enum
variant; `StateUnavailable` and the explicit observation APIs are additive.
`RateLimiterStatus.tokens` changed from `f64` to `Option<f64>` and gained
`unavailable: Option<String>`; `StatusResponse` gained
`last_response_write_unavailable: Option<String>`. Downstream exhaustive matches and DTO
struct literals may need changes. Healthy token JSON remains numeric and new reasons are
omitted when absent. Unavailable tokens are null with a reason. Null last-write age without
a reason means before-first-write; with a reason it means unavailable. Rust compatibility
and healthy MCP wire compatibility must be reviewed separately.

## Side effects, examples and enforcement

`Config::load_from` selects the explicit path, otherwise `TELEGRAM_MCP_CONFIG`, otherwise
the platform default. Failure to read that selected path errors without fallback. It expands
`${VAR}` once without changing process environment and validates operational sub-configs
without authenticating. Parsing TOML directly supplies defaults but skips
expansion/validation. The loader discards TOML parser payloads and non-Unicode environment
values from every error chain, keeping safe position/variable identity; read paths may
appear in errors. `auth_credentials` intentionally exposes borrowed secret strings to the
caller, who must not log them. CLI overrides only mutate the in-memory session path.

`TelegramClient::new` creates directories, opens/migrates SQLite storage and spawns the sender
runner; it has no aggregate construction timeout and does not prompt. Low-level `client()`
and `session()` are upstream-coupled escape hatches that bypass connector wrappers.
Interactive auth waits for user input without a timer and can write terminal prompts/hints;
each login RPC has a resolve deadline. Keep setup separate from MCP stdout serving.
`logging::init` reads `RUST_LOG`, creates optional daily files and installs global tracing;
already-installed tracing is accepted. Retention deletes only owned dated regular log files
by modification age, best effort, and leaves foreign files untouched.

Server builders are inert and consume `self`; validate configuration before applying them.
`with_observability` creates a new empty replay buffer, so old cloned handles remain old;
metrics stay shared. Serving reserves stdout for MCP, owns protocol cleanup, and reports
setup, I/O, SDK/task and signal-trigger errors after cleanup. Shutdown has one drain budget;
started blocking CPU work cannot be force-aborted. Generic custom blocking/asynchronous
bootstrap handlers are outside the production guarantee. The precise contract, bootstrap
write stall guard and runtime/process ownership are in [stdio-runtime.md](stdio-runtime.md).

Crate rustdoc contains runnable domain and in-memory configuration examples and a compiled
generic composition function. Run `cargo run --offline --locked --example inert_server`
for an external-crate example that actually composes every builder with an example-local
inert trait implementation and reads status without RPCs, session files or credentials.
`offline_mcp_fixture` remains the separate protocol/process fixture. No fake client is added
to the production API. The old ignored `parse_channel_id` doctest now runs.

`missing_docs` is enabled on completed leaves: config, error, link, domain IDs/names and
observability buffer/metrics. It is intentionally not a blanket crate-wide deny. Existing
public field construction, constructor names and builder signatures stay compatible.
Documentation is checked with `cargo doc --offline --locked --no-deps` and
`cargo test --offline --locked --doc`, alongside all-target clippy and the full offline suite.
