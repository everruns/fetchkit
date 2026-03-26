# Web Bot Authentication

## Abstract

Optional support for the Web Bot Authentication Architecture
(draft-meunier-web-bot-auth-architecture). When enabled, FetchKit signs
outgoing HTTP requests with Ed25519 signatures per RFC 9421, allowing
origins to cryptographically verify bot identity.

## References

- [draft-meunier-web-bot-auth-architecture](https://datatracker.ietf.org/doc/html/draft-meunier-web-bot-auth-architecture)
- [RFC 9421 — HTTP Message Signatures](https://www.rfc-editor.org/rfc/rfc9421)
- [RFC 7638 — JSON Web Key (JWK) Thumbprint](https://www.rfc-editor.org/rfc/rfc7638)
- [RFC 8037 — CFRG Elliptic Curves (Ed25519) in JOSE](https://www.rfc-editor.org/rfc/rfc8037)

## Requirements

### R-BA-001: Feature-gated

Bot-auth is behind the Cargo feature `bot-auth`. When disabled, no crypto
dependencies are pulled and no signing code is compiled.

### R-BA-002: Ed25519 signing

Requests are signed with Ed25519 using the `ed25519-dalek` crate.
The signing key is provided as a 32-byte seed (raw or base64url-encoded).

### R-BA-003: Covered components

The signature covers at minimum the `@authority` derived component
(RFC 9421 Section 2.2.3). When an agent FQDN is configured, the
`signature-agent` header is also covered.

### R-BA-004: Signature parameters

Every signature includes:
- `created` — Unix timestamp of signature generation
- `expires` — `created + validity_secs` (default 300s)
- `keyid` — JWK Thumbprint (RFC 7638) of the Ed25519 public key
- `alg` — `"ed25519"`
- `nonce` — 32-byte cryptographically random value, base64url-encoded
- `tag` — `"web-bot-auth"`

### R-BA-005: HTTP headers

Three headers are added to signed requests:
- `Signature` — `sig=:<base64url signature>:`
- `Signature-Input` — `sig=(<covered components>);<params>`
- `Signature-Agent` (optional) — agent FQDN for key discovery

### R-BA-006: Key identity

The `keyid` is computed as the base64url-encoded SHA-256 hash of the
canonical JWK representation: `{"crv":"Ed25519","kty":"OKP","x":"<base64url>"}`,
following RFC 7638 member ordering and RFC 8037 for Ed25519.

### R-BA-007: Configuration surface

- `BotAuthConfig::from_seed([u8; 32])` — from raw seed
- `BotAuthConfig::from_base64_seed(&str)` — from base64url string
- `.with_agent_fqdn(fqdn)` — set Signature-Agent FQDN
- `.with_validity_secs(secs)` — set signature lifetime (default 300s)
- `ToolBuilder::bot_auth(config)` — attach to tool pipeline
- CLI: `--bot-auth-key <base64url>` and `--bot-auth-agent <fqdn>`

### R-BA-008: Graceful failure

If signing fails (e.g., clock error), the request proceeds without
signature headers and a warning is logged. Signing never causes a
request to fail.

### R-BA-009: No key discovery server

This implementation is client-side only. Serving the `.well-known`
key directory endpoint is out of scope — operators host their own
key directory if they want origins to discover keys via `Signature-Agent`.

## Test requirements

- Unit tests verify signature generation, JWK thumbprint computation,
  base64url roundtrip, validity window, and Ed25519 verification.
- Integration tests (wiremock) verify that `Signature`, `Signature-Input`,
  and `Signature-Agent` headers are present on outgoing requests.
- All tests run under `#[cfg(feature = "bot-auth")]` or with the feature
  enabled in dev-dependencies.
