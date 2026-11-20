# Hosted Telegram services

Run one stateful `telegram-api` process per encrypted service data volume. It owns account sessions, grants, rate limits, native update readers and uploads. Run `telegram-mcp-http` separately to expose the HTTPS MCP endpoint. The gateway uses the API for each operation and can restart independently. Local `telegram-mcp` stdio installations keep their own configuration and session. The HTTP routes, request types and status codes are published in the [versioned OpenAPI contract](../crates/telegram-api/openapi/v1.yaml).

## Identity provider and registrations

Use an HTTPS OpenID issuer with discovery, asymmetric JWT signing keys, authorization code with S256 PKCE and OAuth token exchange. Set distinct HTTPS resource identifiers for the API and the gateway. For example, use `https://service.example/v1` and `https://mcp.example/mcp`. Access tokens use type `at+jwt`, an exclusive resource audience, and signed `iss`, `sub`, `client_id`, `scope`, `iat`, `exp` and `jti` claims. Pick `RS256`, `ES256` or `EdDSA` explicitly. Publish rotated keys under distinct `kid` values and overlap their availability until outstanding credentials expire. The signing key cache lasts five minutes and unknown-key refreshes are limited to one per five seconds. [JWT access tokens](https://www.rfc-editor.org/rfc/rfc9068.html)

| Registration | Provider configuration |
| --- | --- |
| Account page | Confidential browser client with an exact HTTPS callback at `/auth/callback`, Basic authentication at the token endpoint, an ID token for that browser client and an API-audience access token. Request `openid telegram.link telegram.read`. The page uses PKCE, state and nonce. |
| Direct API clients | Registered client IDs and their exact redirect URIs. Request the API resource and `telegram.read` or `telegram.send` as needed. |
| Remote MCP clients | Independently registered clients with their own redirect URIs. Follow the gateway's bearer challenge to `/.well-known/oauth-protected-resource/mcp`, then authorize at the listed issuer for the MCP resource. |
| Gateway | Confidential token-exchange client authenticated with HTTP Basic. Issue an API-audience bearer valid for at most five minutes, bounded by the incoming credential's expiry. Set signed `act.sub` to the configured gateway actor and `client_id` to the originating client. Grant only the requested subset of its read/send scopes. |

The service maps accounts by verified issuer and subject. If the issuer uses pairwise subjects, configure `subject_claim` with a stable, issuer-signed user identifier present in the browser ID token and both audiences' access tokens. Check the same person and originating client survive an exchange. A mismatch refuses the request before the gateway calls the API. API and MCP discovery are public, and both resources send a metadata URL in a bearer challenge. [OIDC](https://openid.net/specs/openid-connect-core-1_0.html) · [Token exchange](https://www.rfc-editor.org/rfc/rfc8693.html) · [MCP authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)

## Start the processes

Build each release binary from the workspace root using its pinned toolchain and lockfile.

```sh
cargo build --locked --release -p telegram-api --features service
cargo build --locked --release -p telegram-mcp-http
```

Create a private API JSON file using your actual issuer registrations and Telegram application credentials. `eligible_channels` is the deployment's positive list of decimal channel IDs for approved hosted content. Keep it empty until the operator has established permission for the intended use. Record the permissions for AI-facing use separately from account and client grants. [Telegram API terms](https://core.telegram.org/api/terms) · [Content terms](https://telegram.org/tos/content-licensing)

```json
{
  "listen": "127.0.0.1:3000",
  "authority": {
    "issuer": "https://issuer.example",
    "api_audience": "https://service.example/v1",
    "mcp_audience": "https://mcp.example/mcp",
    "gateway_actor": "telegram-mcp-gateway",
    "algorithms": ["RS256"]
  },
  "browser_client_id": "telegram-account",
  "browser_callback": "https://service.example/auth/callback",
  "hosted": {
    "data_root": "/srv/telegram-private",
    "telegram": {"api_id": 12345, "api_hash": "REPLACE_WITH_PRIVATE_APP_HASH"},
    "registered_clients": ["telegram-account", "personal-reader", "remote-assistant"],
    "eligible_channels": [],
    "discovery_clients": [],
    "sending_enabled": false,
    "max_accounts": 64,
    "max_in_flight": 256,
    "idle_seconds": 300,
    "request_seconds": 30
  }
}
```

Provision `data_root` on encrypted persistent storage owned by the API service user. Keep the directory private (0700) and configuration/session files private (0600). The API takes an exclusive root lock, opens its SQLite metadata database and assigns a separate private MTProto session path per link generation. Store encrypted backups under the same controls. Exclude this volume from source control, static file serving and diagnostic archives. Inject the browser client secret as `TELEGRAM_API_OIDC_SECRET` through the service manager.

Create a second private JSON file for the gateway. Use the same authority settings and inject the exchange client secret as `TELEGRAM_MCP_EXCHANGE_SECRET`. `api_origin` is an HTTPS origin or a numeric loopback HTTP origin. Both public-facing processes bind loopback behind the edge.

```json
{
  "listen": "127.0.0.1:3001",
  "authority": {
    "issuer": "https://issuer.example",
    "api_audience": "https://service.example/v1",
    "mcp_audience": "https://mcp.example/mcp",
    "gateway_actor": "telegram-mcp-gateway",
    "algorithms": ["RS256"]
  },
  "api_origin": "http://127.0.0.1:3000",
  "timeout_seconds": 30
}
```

```sh
./target/release/telegram-api /etc/telegram-private/api.json
./target/release/telegram-mcp-http /etc/telegram-private/gateway.json
```

Run the commands as separate services. `GET /readyz` on the API returns 204 after initialization and 503 while draining. `GET /livez` on the gateway returns 204 without accessing a Telegram account. Per-account sessions open on demand. `hosted.max_in_flight` bounds concurrent API handlers (1–1,024), `hosted.max_accounts` ranges from 1 to 256, `idle_seconds` from 1 to 3,600 and `request_seconds` from 1 to 120. The gateway's configurable deadline is 1–120 seconds and includes verification, exchange, capability lookup and the API operation. Only verified delegated API callers can shorten the API's local budget with `X-Telegram-Deadline-Unix-Ms`. The API also clamps it to token expiry and reserves 100 ms for clock skew. Keep both service clocks synchronized.

On SIGINT or SIGTERM the API stops admission, ends account readers and senders even when requests hold leases, and gives the HTTP listener and account tasks one shared ten-second drain deadline. Clients must open fresh streams after a restart. Restart the gateway independently when its issuer settings or exchange secret change.

### HTTPS edge

Route the API hostname to port 3000 and the MCP hostname to port 3001. Preserve the registered public Host, HTTPS Origin, Authorization, Accept and MCP headers, and forward the matching protected-resource discovery paths. Replace the incoming `X-Real-IP` header at the trusted edge with the observed client IP for link admission. Keep callback queries, stream query strings, authorization, cookies, media handles, bodies, phone numbers, codes and passwords out of access logs. Set body and header timeouts on the edge. Allow a raw upload body only up to the selected service limit. Disable automatic upstream retries for mutations.

For an NGINX API server, the following locations illustrate stream and upload settings inside a TLS virtual host. Configure downstream idle timeouts above the heartbeat interval wherever an intermediary observes the stream. Keep gzip, caching and response buffering disabled throughout the SSE path so disconnects and slow downstream writes reach the API. Configure the other API and MCP locations with the same no-retry and header policy. [NGINX proxy buffering and timeouts](https://nginx.org/en/docs/http/ngx_http_proxy_module.html) · [NGINX downstream send timeout](https://nginx.org/en/docs/http/ngx_http_core_module.html#send_timeout)

```nginx
location = /v1/events {
    proxy_pass http://127.0.0.1:3000;
    proxy_http_version 1.1;
    proxy_set_header Host $http_host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_buffering off;
    proxy_cache off;
    proxy_next_upstream off;
    proxy_ignore_client_abort off;
    proxy_read_timeout 45s;
    send_timeout 10s;
    gzip off;
}

location = /v1/uploads {
    proxy_pass http://127.0.0.1:3000;
    proxy_http_version 1.1;
    proxy_set_header Host $http_host;
    proxy_set_header X-Real-IP $remote_addr;
    client_max_body_size 100m;
    proxy_request_buffering off;
    proxy_next_upstream off;
}
```

## Link an account and grant a client access

Open `/account` and sign in via `/auth/start`. The callback establishes a Secure, HttpOnly, SameSite=Lax owner cookie and redirects to `/v1/session`. Return to `/account` to enter the Telegram phone number, code and optional two-step password. A link challenge lasts up to five minutes. The browser session lasts at most fifteen minutes and expires on process restart. Owner mutations require the HTTPS Origin, the session's `X-CSRF-Token` and `telegram.link`.

Use the joined-channel list on the account page to find IDs, then load a registered client's permissions before saving. Updates include the loaded revision. On HTTP 409, reload before applying changes. For example, a grant update at revision 0 can use the following body.

```json
{"expected_revision":0,"read":{"kind":"channels","channel_ids":["42","73"]},"send_enabled":false}
```

Use `none`, selected `channels`, or `all_joined` for the read grant. Delegated clients require an explicit channel list. Both the owner grant and deployment eligibility must admit each channel, and Telegram membership and peer eligibility are checked by the operation. A missing and a foreign channel receive the same unavailable-resource response. Only clients also listed in `discovery_clients` can use public discovery, and results still pass through their effective channel grant. Direct and delegated operations use the same linked account's limiter. The process retains its budget across idle eviction and initializes it conservatively on restart.

Unlink retires the active generation locally and invalidates its handles and streams. The API attempts remote logout while retirement is pending. If cleanup cannot complete within its one-day retention window, `manual_revocation` or `linked_with_manual_revocation` asks the owner to terminate the retired Telegram session in Telegram's Devices settings. A newer link uses a different generation.

## Read, preview and send

Use the API bearer in an Authorization header. Channel IDs are positive decimal strings and message IDs are positive channel-local integers. A history or search response can contain partial results. Inspect its continuation, timeout and per-channel errors. Batch responses distinguish missing message IDs from existing messages omitted to fit the response budget. The channel metadata, search, history, batch, stats, link and transcription contracts are in the [OpenAPI contract](../crates/telegram-api/openapi/v1.yaml).

`POST /v1/media/previews` accepts a channel, message IDs and an optional maximum dimension (default 1280 px, clamped to 64–2048). Successful items carry an opaque handle and JPEG metadata. An item can fail independently. Fetch a successful result at `GET /v1/media/{handle}` under the same caller and current grant. Handles expire within five minutes or at credential expiry. The MCP equivalents return `telegram-media://preview/{handle}` links that clients read using `resources/read`. Opening a link checks authorization again.

Sending requires `hosted.sending_enabled`, the originating client's account-generation `send_enabled` grant and an API or MCP credential with `telegram.send`. A send-only client can target any supported peer writable by that linked account, including a peer outside its read grant. Choose the recipient per call. Send plain text with `POST /v1/messages/send` or the MCP `send_message` tool.

```json
{"recipient":{"kind":"peer","namespace":"channel","id":"42"},"text":"Build complete."}
```

For media, stage raw bytes with `POST /v1/uploads` and a matching Content-Type. Supported image types are JPEG and PNG (10 MiB limit). Supported audio/video types are MPEG audio, M4A, AAC, Ogg voice and MP4 video (100 MiB limit). The API checks the observed byte count and media type, reserves capacity while receiving the stream and returns `upload_handle`. A handle belongs to the caller, originating client, account generation and grant revision. It expires after five minutes. Use the same identity to submit `POST /v1/messages/send-media` or the MCP `send_media` tool.

```json
{"recipient":{"kind":"saved_messages"},"upload_handle":"REPLACE_WITH_RETURNED_HANDLE","media":{"kind":"image"},"caption":"Latest chart"}
```

A successful send returns the chosen recipient, UTC date and message ID. Read failures can be retried according to their code and `Retry-After`. For a send failure, inspect `error.delivery` in REST or `error.delivery_state` in MCP. `not_sent` confirms no delivery. `unknown` is nonretryable. After an interrupted response, confirm delivery in Telegram before deciding whether to send again.

## Consume live posts and recover a gap

Open `GET /v1/events?channel_id=42&channel_id=73` with an API bearer and `Accept: text/event-stream`. An owner page can also open a same-origin streaming fetch using its session cookie, a read scope and a grant for the browser client. The API verifies the configured Origin or the browser’s `Sec-Fetch-Site: same-origin` signal. Each distinct requested ID must be a permitted joined broadcast channel. Send credentials in headers. The API validates the entire subscription and captures each head before sending HTTP 200. It delivers ordinary new posts, including captionless media and individual album siblings. Cross-channel events arrive in observation order.

```text
event: ready
data: {"channels":[{"channel_id":"42","head_message_id":104},{"channel_id":"73","head_message_id":null}]}

event: post
data: {"channel_id":"42","message_id":106,"published_at":"2026-11-20T01:02:03Z","text":"New post","media":null}

event: interrupted
data: {"reason":"upstream_disconnected"}

```

`null` means Telegram confirmed an empty channel. Once ready, each channel emits new posts above its captured head. An ID jump alone gives no information about missed posts. Heartbeats are SSE comments. An interrupted frame is best effort. EOF, token expiry, membership loss, a changed grant, service drain or a slow consumer can end the connection. Treat every reconnect as a fresh subscription. The API stores neither consumer positions nor an event archive. The reader can observe a reported disconnection, malformed update or an unrecoverable difference. Upstream events that Telegram or the SDK silently omits cannot be signalled individually.

1. Persist a last-processed message ID per **account and channel** after processing succeeds. For a first historical load use 0.
2. Open a fresh live subscription, read `ready` and begin buffering new posts with a client-side size bound. For each non-null head above the saved cursor, request `GET /v1/channels/{id}/messages?after_id={saved}&through_id={head}&limit=100`. Skip channels with an empty or already-processed range.
3. The range is `saved < message_id <= head`. Pages are newest first. Keep both bounds fixed and pass the response's `next_before_id` as the next page's exclusive `before_id`. The inclusive upper bound also accepts 2147483647. Process the retrieved series in chronological order per channel before processing buffered live posts. Deduplicate by account, channel and message ID.
4. If the connection closes, the live buffer fills, or a partial/timeout response leaves coverage uncertain, restart at the last successfully persisted position. A history read can recover only messages still visible to the account.

Set stream limits under `hosted.events`. Closing the connection releases its account lease and queue reservations. A paused writer is bounded at the HTTP socket and at the downstream edge.

| Setting | Default |
| --- | --- |
| `max_channels`, `max_account_subscriptions`, `max_total_subscriptions` | 20 distinct channels per feed, 4 feeds per account, 256 feeds total |
| `max_queue_items`, `max_queue_bytes` | 128 items, 1 MiB per subscriber |
| `max_event_bytes`, `max_total_queue_bytes` | 64 KiB per encoded event, 64 MiB across the service |
| `setup_seconds`, `heartbeat_seconds`, `stalled_write_seconds` | 30, 15, 10 seconds |

## Hosted MCP client

Connect with the `2026-07-28` Streamable HTTP binding at `/mcp`. Send the protocol version header, method/name mirror headers as applicable, and the matching per-request `_meta` for that binding. Browser-originated requests must match the configured public gateway origin. `tools/list` reflects current grants, and every call rechecks them before submitting its API operation. Read structured output from `structuredContent` or its matching first text block. A media preview can add resource links after that block. Continuous subscriptions use the API's SSE endpoint. [MCP Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
