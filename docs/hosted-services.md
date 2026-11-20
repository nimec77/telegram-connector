# Hosted Telegram services

The hosted API can authenticate direct or delegated callers and create a
first-party browser session for the account owner. Its current HTTP surface is
readiness, resource metadata, OpenID sign-in, and session inspection and logout.
The [OpenAPI document](../crates/telegram-api/openapi/v1.yaml) describes these routes.

## Provider registration

Configure an HTTPS OpenID issuer that publishes its exact issuer identifier,
authorization and token endpoints, and a signing key set through discovery.
Register a confidential browser client using HTTP Basic at the token endpoint,
authorization code with S256 PKCE, and an exact HTTPS `/auth/callback` URI. The
provider must issue both an ID token for the browser registration and a
JWT access token for the API resource. The requested browser scopes are
`openid` and `telegram.link`. [OpenID Connect](https://openid.net/specs/openid-connect-core-1_0.html)

Use distinct HTTPS API and MCP resource identifiers. Access tokens follow the
JWT access-token profile with `at+jwt` type and signed `sub`, `aud`, `exp`,
`iat`, `jti`, and `client_id` claims. The resource audience must be exclusive.
For delegated API credentials, include an `act.sub` matching the configured
gateway and preserve the originating registered client in `client_id`. Use at
most five minutes of validity and issue only applicable read and send scopes.
The optional `subject_claim` must be present and identical across that user's
browser ID token, API tokens, and MCP tokens when provider subject IDs are
pairwise. Use the normal `sub` claim when it is stable across all registrations.
[JWT access tokens](https://www.rfc-editor.org/rfc/rfc9068.html) · [OAuth token exchange](https://www.rfc-editor.org/rfc/rfc8693.html)

Configure the issuer's asymmetric algorithms explicitly using `RS256`, `ES256`,
or `EdDSA`. Publish each rotated signing key with a distinct `kid`, and overlap
key publication while credentials signed with the previous key may be in use.
The service refreshes unknown keys with a five-second minimum refresh interval
and uses a five-minute cache. The issuer and discovered URLs must use HTTPS
without credentials, query strings, or fragments.

## Run behind HTTPS

Build with `cargo build --locked --release -p telegram-api`. Create a private
JSON configuration containing the issuer, resource identifiers, browser
registration, and listener. A deployment might use the following values.

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
  "browser_callback": "https://service.example/auth/callback"
}
```

Inject the confidential client secret as `TELEGRAM_API_OIDC_SECRET` through the
service manager, then run `telegram-api /path/to/settings.json`. Terminate
public HTTPS at the trusted reverse proxy and forward to the loopback listener.
Keep authorization and cookie headers and callback query strings out of proxy
access logs. Forward the protected-resource metadata endpoint as well as the
`/auth` and `/v1` routes. The API sends `Cache-Control: no-store` and
`Referrer-Policy: no-referrer` on these responses.

In a browser, open `/auth/start` to sign in. On completion the API redirects to
`/v1/session`, which returns the owner session's CSRF token. Browser logout
requires that token in `X-CSRF-Token` and the HTTPS origin in `Origin`. Session
cookies use `Secure`, `HttpOnly`, and `SameSite=Lax`. Browser sessions expire
within fifteen minutes and are held in process memory.

The loopback issuer fixture exercises signed tokens, rotation, PKCE, nonce,
browser state and logout using `cargo test --locked -p telegram-api identity_http`.
`GET /readyz` returns 204 once the configured service has initialized.
