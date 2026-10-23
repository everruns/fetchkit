# Security Notes

Fetchkit is intended to run in agent, server, and cluster environments where URL input may be
user-controlled.

## Updating to 0.6.0

Fetchkit 0.6.0 requires published Markkit 0.1.1 and Tokio 1.54.1 or later in
their compatible version lines. Its release lockfile also updates the PDF parser
and Rust TLS certificate-validation/root dependencies. Applications with existing
lockfiles should run `cargo update` and audit their resolved dependencies; a
library upgrade alone does not replace every transitive locked version. The default
HTTP client uses the platform certificate store via rustls-platform-verifier;
operators must also keep their OS CA packages current. Cargo root-bundle updates
do not replace system trust stores. Rebuild
installed CLI binaries with `cargo install fetchkit-cli --version 0.6.0 --locked`.

`FetchOptions` has new `document_conversion` and `content_processors` fields.
Use `..Default::default()` in struct literals when default behavior is desired.
Construct the former unit struct `DefaultFetcher` with `DefaultFetcher::new()`.
Default Office conversion now uses Markkit for DOCX/XLSX/XLS/ODS; opt into the
deprecated `docs-office` feature for PPTX/XLSB support. See the
[release notes](../CHANGELOG.md#060---2026-10-23) for the other accumulated changes.

## Safe Defaults

- Private and reserved IP ranges are blocked by default via resolve-then-check DNS validation.
- Redirects are followed manually so every hop is revalidated.
- Textual response bodies are capped at 10 MB after decompression by default. Larger responses are truncated
  and marked with `truncated: true`.
- `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY` are ignored by default.

## Multi-Tenant Deployment

For shared VMs, containers, or clusters:

- Keep private-IP blocking enabled.
- Keep proxy inheritance disabled unless outbound traffic must traverse a trusted proxy.
- Use allow-lists where possible instead of relying only on block-lists.
- Apply caller-side rate limits and concurrency limits around Fetchkit.

If you need different limits, configure them through `ToolBuilder`:

```rust
use fetchkit::ToolBuilder;

let tool = ToolBuilder::new()
    .max_body_size(1024 * 1024)
    .respect_proxy_env(false)
    .build();
```

See [`knowledge/security/threat-model.md`](../knowledge/security/threat-model.md) for the full threat inventory.

## Web Bot Authentication

Fetchkit optionally supports the [Web Bot Authentication Architecture](https://datatracker.ietf.org/doc/html/draft-meunier-web-bot-auth-architecture),
which signs outgoing requests with Ed25519 signatures per RFC 9421. This lets
origins verify bot identity cryptographically instead of relying on User-Agent
strings.

Enable the `bot-auth` Cargo feature and configure a signing key:

```rust
use fetchkit::{ToolBuilder, BotAuthConfig};

let tool = ToolBuilder::new()
    .bot_auth(
        BotAuthConfig::from_seed([/* 32-byte Ed25519 seed */; 32])
            .with_agent_fqdn("bot.example.com")
    )
    .build();
```

CLI usage:

```bash
fetchkit fetch https://example.com --bot-auth-key <base64url-seed> --bot-auth-agent bot.example.com
```

See [`knowledge/security/bot-auth.md`](../knowledge/security/bot-auth.md) for the full protocol design.
